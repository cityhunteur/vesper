//go:build linux

package unfs

import (
	"fmt"
	"sort"
	"strings"

	"golang.org/x/sys/unix"
)

// Sources that name one directory under two spellings (TD-082, D-061).
//
// A macOS volume can take two spellings of a name for one directory: another letter case, and another form of an
// accented letter. Which source lies inside which is judged by the text of the paths (Nests, Holders, MovedUnder,
// alsoReadWrite, hasPlace). Two binds of one task that spelled a directory in two ways were therefore taken for two
// trees: a read-only bind got no mount on its name inside the read-write bind that holds it (the server still
// refused a change there, by the object's number), a directory that was bound both ways was read-only in both, and
// a read-write bind inside a read-only one was served read-only after the server started again.
//
// So a start first gives such sources one spelling. The directories on the way to each source are opened as the
// anchors are (chainOf): one walk beneath the share mount, no symbolic link and no other mount on the way. Where
// one of them is the directory of a directory source on the same share mount, that part of the path is written as
// that source writes it. The rest of the start, the sandbox and the watcher then go by the text, as before. Both
// spellings name the same object, so nothing else changes; a source whose way cannot be walked now is left as it
// was given, and the start's own checks judge it.
//
// Not covered: a file that is bound both ways under two spellings of its own name (the file's name is not walked;
// the file is then read-only in both binds, as before).

// rootKey is the directory of a directory source: the share mount and the directory's identity.
type rootKey struct {
	mount int
	id    fileID
}

// spelledAs returns rel with the part that leads to the deepest root on its way written as that root's source
// writes it, and whether that changed rel. ids are the directories of rel's way beneath share mount mount: the
// mount's root, then the directory at each leading part of rel (chainOf). roots gives each root's own spelling.
func spelledAs(mount int, rel string, ids []fileID, roots map[rootKey]string) (string, bool) {
	parts := strings.Split(rel, "/")
	for k := len(ids) - 1; k >= 0 && k <= len(parts); k-- {
		as, ok := roots[rootKey{mount, ids[k]}]
		if !ok {
			continue
		}
		out := as // ids[k] is the directory at parts[:k]
		if tail := strings.Join(parts[k:], "/"); tail != "" {
			if as == "." {
				out = tail
			} else {
				out = as + "/" + tail
			}
		}
		return out, out != rel
	}
	return rel, false
}

// spellOrder is the order in which the sources take their spelling: the directory sources, the shallowest first
// (as given among those of one depth), then the file sources. So each takes the spelling of the directory sources
// that hold it, and the first of two that are the same directory keeps its own.
func spellOrder(srcs []Source) (dirs, files []int) {
	for i, s := range srcs {
		if s.File {
			files = append(files, i)
		} else {
			dirs = append(dirs, i)
		}
	}
	depth := func(i int) int {
		if srcs[i].Rel == "." {
			return 0
		}
		return 1 + strings.Count(srcs[i].Rel, "/")
	}
	sort.SliceStable(dirs, func(a, b int) bool { return depth(dirs[a]) < depth(dirs[b]) })
	return dirs, files
}

// spellAll is the rule of respell without the walks: ways[i] are the directories on the way to source i as given
// (for a directory source, the directory itself is the last), or nil when its way could not be walked. It returns
// the sources with one spelling.
func spellAll(srcs []Source, ways [][]fileID) []Source {
	out := append([]Source(nil), srcs...)
	roots := map[rootKey]string{}
	take := func(i int) {
		if rel, changed := spelledAs(out[i].Mount, out[i].Rel, ways[i], roots); changed && CheckRel(rel) == nil {
			out[i].Rel = rel
		}
	}
	dirs, files := spellOrder(srcs)
	for _, i := range dirs {
		if len(ways[i]) == 0 {
			continue
		}
		take(i)
		if key := (rootKey{out[i].Mount, ways[i][len(ways[i])-1]}); roots[key] == "" {
			roots[key] = out[i].Rel
		}
	}
	for _, i := range files {
		if len(ways[i]) > 0 {
			take(i)
		}
	}
	return out
}

// respell returns srcs with one spelling for the sources that name one directory tree (see above), and a line for
// each source it wrote anew. It walks the way of each source, in the order of spellOrder, and holds the directory
// of each directory source until the spellings are settled. A new spelling is kept only when it is shown to name
// the same directory as the given one: both ways are walked again and held together, and their last directories
// (the source's own, or the one that holds a file source) must be one. So a number that the System VM gave to two
// directories one after the other can never make a source name another place than the user gave.
func respell(mounts []string, srcs []Source) ([]Source, []string) {
	var mfds, held []int
	defer func() {
		for _, fd := range append(mfds, held...) {
			unix.Close(fd)
		}
	}()
	for _, m := range mounts {
		mfd, err := unix.Open(m, unix.O_PATH|unix.O_DIRECTORY|unix.O_NOFOLLOW|unix.O_CLOEXEC, 0)
		if err != nil {
			return append([]Source(nil), srcs...), nil // the start's own open of the share mounts reports it
		}
		mfds = append(mfds, mfd)
	}
	// way opens the directories on the way to source s, and the directory itself when the source is one. The
	// caller closes fds.
	way := func(s Source) (ids []fileID, fds []int) {
		if s.Mount < 0 || s.Mount >= len(mfds) || s.check() != nil {
			return nil, nil
		}
		if s.Rel == "." { // the share mount itself: chainOf has nothing to walk
			fd, err := openDirIn(mfds[s.Mount], ".")
			if err != nil {
				return nil, nil
			}
			id, err := idOf(fd)
			if err != nil {
				unix.Close(fd)
				return nil, nil
			}
			return []fileID{id}, []int{fd}
		}
		ids, fds, err := chainOf(mfds[s.Mount], s.Rel, !s.File)
		if err != nil {
			return nil, nil
		}
		return ids, fds
	}
	ways := make([][]fileID, len(srcs))
	dirs, files := spellOrder(srcs)
	for _, i := range dirs {
		ids, fds := way(srcs[i])
		if len(fds) == 0 {
			continue
		}
		ways[i] = ids
		last := len(fds) - 1
		held = append(held, fds[last])
		for _, fd := range fds[:last] {
			unix.Close(fd)
		}
	}
	closeFds := func(fds []int) {
		for _, fd := range fds {
			unix.Close(fd)
		}
	}
	for _, i := range files {
		ids, fds := way(srcs[i])
		ways[i] = ids
		closeFds(fds)
	}
	out := spellAll(srcs, ways)
	var notes []string
	for i := range out {
		if out[i].Rel == srcs[i].Rel {
			continue
		}
		given, gfds := way(srcs[i])
		taken, tfds := way(out[i])
		same := len(given) > 0 && len(taken) > 0 && given[len(given)-1] == taken[len(taken)-1]
		closeFds(gfds)
		closeFds(tfds)
		if !same {
			out[i].Rel = srcs[i].Rel
			continue
		}
		notes = append(notes, fmt.Sprintf("source %q names a place in another source's directory under another spelling of the path; it is taken as %q", srcs[i].Rel, out[i].Rel))
	}
	return out, notes
}
