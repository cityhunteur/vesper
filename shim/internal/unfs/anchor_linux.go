package unfs

import (
	"errors"
	"fmt"
	"os"
	"strings"

	"golang.org/x/sys/unix"
)

// Anchors keep a task from changing what the path of a read-only source names (TD-080, D-058).
//
// A read-only source that lies inside a read-write directory source is reached by a chain of directories from that
// source's root. A task that renames one of them aside and puts another at the name makes the source's path name
// an object of its own choosing: the watcher then attaches it (a file source), a start after that serves it, and so
// does the next launch. The server therefore refuses a rename or a removal of a directory on such a chain, an
// anchor, by any name (unfs3 patch 0006, "a DEV INO" lines of its list).
//
// The chain of a read-only FILE source ends at the directory that holds the file: the file itself is the source,
// which the server protects, and a file that replaces it is told and attached by the watcher. The chain of a
// read-only DIRECTORY source includes the directory now at the source's path. That directory is the source itself
// at a start, which the server protects already. But a directory source is attached once: when macOS puts another
// directory at the path, the server goes on serving the old one, and the new one would be an ordinary directory
// that the task could rename aside to put its own at the user's path for the next start and the next launch.
//
// The chain is found by identity, not by the text of the paths: each directory on the way is opened beneath the
// share mount, and the chain starts below the first of them that is the root of a read-write directory source. So a
// read-write source given under another spelling of the path (a volume that ignores letter case) holds the chain
// too. Every replacement of what the path names is then made on macOS, by the user. The watcher walks the chains
// again at every check; when one is not what it was, the server is told of every chain as it is now.

// maxAnchors bounds the different anchors of one server. The server remembers twice as many, newest first, and the
// watcher tells it of all the current ones whenever one changed, so the current ones are never among those it drops.
const maxAnchors = 1024

// anchorState is what Run finds at a start and the watcher keeps current. After the watcher has started, only the
// watcher's goroutine uses it. Its descriptors are held until the process ends: Run does not wait for the watcher,
// so it cannot close them without a race, and the process ends when Run returns.
type anchorState struct {
	roots  map[fileID]bool // the roots of the read-write directory sources
	chains [][]fileID      // by source: the anchors last told for it
	held   map[fileID]int  // a descriptor of each anchor, so that its inode number stays its own
}

func (a *anchorState) close() {
	for _, fd := range a.held {
		unix.Close(fd)
	}
	a.held = nil
}

// watched reports whether there is anything for the watcher to keep current: a read-write directory source, and a
// source with a protected place that could lie in it.
func (a *anchorState) watched(srcs []Source) bool {
	if a == nil || len(a.roots) == 0 {
		return false
	}
	for _, s := range srcs {
		if hasPlace(srcs, s) {
			return true
		}
	}
	return false
}

// chainOf opens the directories on the way to rel beneath the share mount mfd, from the mount's root down to the
// directory that holds the source, and, with leaf, the directory at rel itself. Each is opened beneath the mount
// with no symbolic link on the way. It returns their identities and a descriptor of each, which the caller closes.
func chainOf(mfd int, rel string, leaf bool) ([]fileID, []int, error) {
	var ids []fileID
	var fds []int
	fail := func(err error) ([]fileID, []int, error) {
		for _, fd := range fds {
			unix.Close(fd)
		}
		return nil, nil, err
	}
	if rel == "." || rel == "" {
		return nil, nil, nil
	}
	parts := strings.Split(rel, "/")
	last := len(parts) - 1
	if leaf {
		last = len(parts)
	}
	for n := 0; n <= last; n++ {
		sub := "."
		if n > 0 {
			sub = strings.Join(parts[:n], "/")
		}
		fd, err := unix.Openat2(mfd, sub, &unix.OpenHow{
			Flags:   unix.O_PATH | unix.O_DIRECTORY | unix.O_CLOEXEC,
			Resolve: unix.RESOLVE_BENEATH | unix.RESOLVE_NO_SYMLINKS | unix.RESOLVE_NO_MAGICLINKS | unix.RESOLVE_NO_XDEV,
		})
		if err != nil {
			return fail(fmt.Errorf("%s: %w", sub, err))
		}
		fds = append(fds, fd)
		id, err := idOf(fd)
		if err != nil {
			return fail(fmt.Errorf("%s: %w", sub, err))
		}
		ids = append(ids, id)
	}
	return ids, fds, nil
}

// anchorsOf returns the part of a chain that lies below the outermost root of a read-write directory source: the
// directories a task could rename through that source. The results are the tails of ids and fds; the caller closes
// the returned descriptors, and the descriptors of the part above are closed here. It returns nothing, and closes
// everything, when no such root is on the chain: the task cannot reach the path at all.
func anchorsOf(ids []fileID, fds []int, roots map[fileID]bool) ([]fileID, []int) {
	for k, id := range ids {
		if roots[id] {
			for _, fd := range fds[:k+1] {
				unix.Close(fd)
			}
			return ids[k+1:], fds[k+1:]
		}
	}
	for _, fd := range fds {
		unix.Close(fd)
	}
	return nil, nil
}

// findAnchors walks the chain of every source that has a protected place. It returns the anchors by source, one
// descriptor of each different anchor (the caller closes them), and, by source, the error of a chain that could not
// be walked now.
func findAnchors(mfds []int, srcs []Source, roots map[fileID]bool) ([][]fileID, map[fileID]int, map[int]error) {
	chains, held, failed := make([][]fileID, len(srcs)), map[fileID]int{}, map[int]error{}
	if len(roots) == 0 {
		return chains, held, failed
	}
	for i, s := range srcs {
		if !hasPlace(srcs, s) || s.Mount < 0 || s.Mount >= len(mfds) {
			continue
		}
		ids, fds, err := chainOf(mfds[s.Mount], s.Rel, !s.File)
		if err != nil {
			failed[i] = err
			continue
		}
		ids, fds = anchorsOf(ids, fds, roots)
		chains[i] = ids
		for k, id := range ids {
			if _, have := held[id]; have {
				unix.Close(fds[k])
			} else {
				held[id] = fds[k]
			}
		}
	}
	return chains, held, failed
}

func sameChain(a, b []fileID) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}

func closeAll(fds map[fileID]int) {
	for _, fd := range fds {
		unix.Close(fd)
	}
}

// startAnchors finds the anchors of a start. roots are the identities of the read-write directory sources as
// OpenSources opened them. A chain that cannot be walked fails the start: OpenSources has just opened the source at
// that path, and a server that started without the chain would leave the path to the task.
func startAnchors(mounts []string, srcs []Source, roots map[fileID]bool) (*anchorState, []ProtectID, error) {
	a := &anchorState{roots: roots}
	var mfds []int
	defer func() {
		for _, fd := range mfds {
			unix.Close(fd)
		}
	}()
	for _, m := range mounts {
		mfd, err := unix.Open(m, unix.O_PATH|unix.O_DIRECTORY|unix.O_NOFOLLOW|unix.O_CLOEXEC, 0)
		if err != nil {
			return nil, nil, fmt.Errorf("share mount %s: %w", m, err)
		}
		mfds = append(mfds, mfd)
	}
	var failed map[int]error
	a.chains, a.held, failed = findAnchors(mfds, srcs, roots)
	for i, err := range failed {
		a.close()
		return nil, nil, fmt.Errorf("source %q: the directories on the way to it: %w", srcs[i].Rel, err)
	}
	if len(a.held) > maxAnchors {
		n := len(a.held)
		a.close()
		return nil, nil, fmt.Errorf("%d directories lie on the way to the read-only sources inside read-write ones; at most %d", n, maxAnchors)
	}
	return a, anchorList(a.chains), nil
}

// anchorList is every different anchor of chains, in the order of the sources.
func anchorList(chains [][]fileID) []ProtectID {
	var out []ProtectID
	seen := map[fileID]bool{}
	for _, chain := range chains {
		for _, id := range chain {
			if !seen[id] {
				seen[id] = true
				out = append(out, ProtectID{Dev: id.dev, Ino: id.ino})
			}
		}
	}
	return out
}

// checkChains walks the chains again. When a chain is not what was last told (macOS replaced a directory on it, or
// the directory at a read-only directory source's path), the server is told of every anchor of every chain as it
// is now: the new ones, and the others again, so that all the current ones are its newest. The new set is held in
// place of the old. A chain that cannot be walked now keeps what was last told for it, and that is logged once.
func (w *fileWatch) checkChains() {
	a := w.anchors
	if a == nil || len(a.roots) == 0 || w.list == nil {
		return
	}
	chains, held, failed := findAnchors(w.mfds, w.srcs, a.roots)
	changed := false
	for i := range w.srcs {
		key := fmt.Sprintf("chain %d", i)
		if err, bad := failed[i]; bad {
			w.once(key, "source %q: the directories on the way to it cannot be looked at now (%v); the server keeps what it was last told", w.srcs[i].Rel, err)
			chains[i] = a.chains[i]
			for _, id := range chains[i] { // what was last told stays held
				if _, have := held[id]; !have {
					if fd, ok := a.held[id]; ok {
						if dup, err := unix.FcntlInt(uintptr(fd), unix.F_DUPFD_CLOEXEC, 0); err == nil {
							held[id] = dup
						}
					}
				}
			}
			continue
		}
		delete(w.said, key)
		if !sameChain(chains[i], a.chains[i]) {
			changed = true
		}
	}
	if !changed {
		closeAll(held)
		return
	}
	if len(held) > maxAnchors {
		w.once("anchors", "%d directories lie on the way to the read-only sources now, more than the %d the server is told of; it keeps what it was last told", len(held), maxAnchors)
		closeAll(held)
		return
	}
	for _, line := range anchorList(chains) {
		if err := AppendAnchor(w.list, line); err != nil {
			closeAll(held)
			if errors.Is(err, os.ErrClosed) {
				return // the server has ended and Run closed the list
			}
			// A directory that the server cannot be told of could be renamed by the task. The server is ended,
			// and the next start writes the list anew, with the chains as they are then.
			w.logf("a directory on the way to a read-only source was replaced, and the server could not be told of it (%v); the server is ended and starts again with a list written anew", err)
			w.end()
			return
		}
		w.grown += len(anchorLine(line))
	}
	w.logf("a directory on the way to a read-only source was replaced; the server was told of the %d directories now on the way", len(held))
	closeAll(a.held)
	a.chains, a.held = chains, held
	if w.grown > maxListGrowth {
		w.logf("the server's list has grown by %d bytes; the server is ended and starts again with a list written anew", w.grown)
		w.end()
	}
}
