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
// accented letter. Which source lies inside which is judged by the text of the paths (Nests, Holders,
// alsoReadWrite, hasPlace). Two binds of one task that spelled a directory in two ways were therefore taken for two
// trees: a read-only bind got no mount on its name inside the read-write bind that holds it (the server still
// refused a change there, by the object's number), and a directory that was bound both ways was read-only in both.
//
// So a start first gives such sources one spelling. The directories on the way to each source are opened as the
// anchors are (chainOf): one walk beneath the share mount, no symbolic link and no other mount on the way. Where
// one of them is the root of a read-write directory source on the same share mount, that part of the path is
// written as that source writes it. The rest of the start, the sandbox and the watcher then go by the text, as
// before. Both spellings name the same object, so nothing else changes; a source whose way cannot be walked now is
// left as it was given, and the start's own checks judge it.

// rootKey is the root of a read-write directory source: the share mount and the directory's identity.
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

// respell returns srcs with one spelling for the sources that name one directory tree (see above), and a line for
// each source it wrote anew. The read-write directory sources go first, the shallowest first, so that each takes
// the spelling of the ones that hold it; the first of two that are the same directory keeps its own.
func respell(mounts []string, srcs []Source) ([]Source, []string) {
	out := append([]Source(nil), srcs...)
	var notes []string
	var mfds, held []int
	defer func() {
		for _, fd := range append(mfds, held...) {
			unix.Close(fd)
		}
	}()
	for _, m := range mounts {
		mfd, err := unix.Open(m, unix.O_PATH|unix.O_DIRECTORY|unix.O_NOFOLLOW|unix.O_CLOEXEC, 0)
		if err != nil {
			return out, nil // the start's own open of the share mounts reports it
		}
		mfds = append(mfds, mfd)
	}
	// way opens the directories on the way to source i, and the directory itself when the source is one. The
	// caller closes fds.
	way := func(i int) (ids []fileID, fds []int, ok bool) {
		s := out[i]
		if s.Mount < 0 || s.Mount >= len(mfds) || s.check() != nil {
			return nil, nil, false
		}
		if s.Rel == "." { // the share mount itself: chainOf has nothing to walk
			fd, err := unix.Openat2(mfds[s.Mount], ".", &unix.OpenHow{Flags: unix.O_PATH | unix.O_DIRECTORY | unix.O_CLOEXEC})
			if err != nil {
				return nil, nil, false
			}
			id, err := idOf(fd)
			if err != nil {
				unix.Close(fd)
				return nil, nil, false
			}
			return []fileID{id}, []int{fd}, true
		}
		ids, fds, err := chainOf(mfds[s.Mount], s.Rel, !s.File)
		return ids, fds, err == nil
	}
	closeFds := func(fds []int) {
		for _, fd := range fds {
			unix.Close(fd)
		}
	}
	take := func(i int, roots map[rootKey]string, ids []fileID) {
		rel, changed := spelledAs(out[i].Mount, out[i].Rel, ids, roots)
		if !changed || CheckRel(rel) != nil {
			return
		}
		notes = append(notes, fmt.Sprintf("source %q names a place in a read-write source under another spelling of the path; it is taken as %q", out[i].Rel, rel))
		out[i].Rel = rel
	}
	var rw, rest []int
	for i, s := range out {
		if !s.ReadOnly && !s.File {
			rw = append(rw, i)
		} else {
			rest = append(rest, i)
		}
	}
	depth := func(i int) int {
		if out[i].Rel == "." {
			return 0
		}
		return 1 + strings.Count(out[i].Rel, "/")
	}
	sort.SliceStable(rw, func(a, b int) bool { return depth(rw[a]) < depth(rw[b]) })
	// A root stays held while its identity is in the map, so that the identity stays its own.
	roots := map[rootKey]string{}
	for _, i := range rw {
		ids, fds, ok := way(i)
		if !ok {
			continue
		}
		take(i, roots, ids)
		last := len(fds) - 1
		if key := (rootKey{out[i].Mount, ids[last]}); roots[key] == "" {
			roots[key] = out[i].Rel
			held = append(held, fds[last])
			fds = fds[:last]
		}
		closeFds(fds)
	}
	if len(roots) == 0 {
		return out, notes
	}
	for _, i := range rest {
		ids, fds, ok := way(i)
		if !ok {
			continue
		}
		take(i, roots, ids)
		closeFds(fds)
	}
	return out, notes
}
