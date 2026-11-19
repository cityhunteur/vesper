package unfs

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"golang.org/x/sys/unix"
)

func anchorTree(t *testing.T) (top string, mfd int, id func(string) fileID) {
	t.Helper()
	top = t.TempDir()
	for _, d := range []string{"a/b/c", "a/conf", "x"} {
		if err := os.MkdirAll(filepath.Join(top, d), 0o755); err != nil {
			t.Fatal(err)
		}
	}
	if err := os.WriteFile(filepath.Join(top, "a/b/c/f"), []byte("f"), 0o644); err != nil {
		t.Fatal(err)
	}
	mfd, err := unix.Open(top, unix.O_PATH|unix.O_DIRECTORY|unix.O_CLOEXEC, 0)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { unix.Close(mfd) })
	id = func(rel string) fileID {
		t.Helper()
		var st unix.Stat_t
		if err := unix.Lstat(filepath.Join(top, rel), &st); err != nil {
			t.Fatal(err)
		}
		return fileID{uint64(st.Dev), st.Ino}
	}
	return top, mfd, id
}

// The chain of a read-only source is found by identity: it starts below the outermost directory that is the root of
// a read-write source, whatever the sources' paths say; the chain of a directory source includes the directory at
// its path (TD-080).
func TestAnchors(t *testing.T) {
	top, mfd, id := anchorTree(t)
	chain := func(rel string, leaf bool) ([]fileID, []int) {
		t.Helper()
		ids, fds, err := chainOf(mfd, rel, leaf)
		if err != nil {
			t.Fatalf("chainOf(%q): %v", rel, err)
		}
		return ids, fds
	}
	closeFds := func(fds []int) {
		for _, fd := range fds {
			unix.Close(fd)
		}
	}
	ids, fds := chain("a/b/c/f", false)
	if len(ids) != 4 || ids[0] != id(".") || ids[1] != id("a") || ids[2] != id("a/b") || ids[3] != id("a/b/c") {
		t.Fatalf("the chain of a file = %v; want the mount's root, a, a/b and a/b/c", ids)
	}
	// The read-write source is "a": the task can rename a/b and a/b/c through it, and not "a" itself, its root.
	got, held := anchorsOf(ids, fds, map[fileID]bool{id("a"): true})
	if len(got) != 2 || got[0] != id("a/b") || got[1] != id("a/b/c") || len(held) != 2 {
		t.Errorf("anchors below a = %v (%d held); want a/b and a/b/c", got, len(held))
	}
	closeFds(held)
	// A directory source: the directory at its path is on the chain too, also right under the read-write root.
	ids, fds = chain("a/conf", true)
	got, held = anchorsOf(ids, fds, map[fileID]bool{id("a"): true})
	if len(got) != 1 || got[0] != id("a/conf") {
		t.Errorf("anchors of a directory source right under the read-write root = %v; want the directory itself", got)
	}
	closeFds(held)
	// Two read-write sources on the chain: the outermost counts, so the root of the inner one is an anchor too.
	ids, fds = chain("a/b/c/f", false)
	got, held = anchorsOf(ids, fds, map[fileID]bool{id("a/b"): true, id("."): true})
	if len(got) != 3 || got[0] != id("a") {
		t.Errorf("anchors below the mount's root = %v; want a, a/b and a/b/c", got)
	}
	closeFds(held)
	// No read-write source holds the path: no anchors. A file right under a read-write root: none either.
	ids, fds = chain("a/b/c/f", false)
	if got, _ := anchorsOf(ids, fds, map[fileID]bool{id("x"): true}); len(got) != 0 {
		t.Errorf("anchors with no read-write source on the way: %v", got)
	}
	ids, fds = chain("a/b/c/f", false)
	if got, _ := anchorsOf(ids, fds, map[fileID]bool{id("a/b/c"): true}); len(got) != 0 {
		t.Errorf("anchors of a file right under a read-write root: %v", got)
	}
	// A symbolic link on the way is refused, and the mount's root has no chain.
	if err := os.Symlink("a", filepath.Join(top, "l")); err != nil {
		t.Fatal(err)
	}
	if _, _, err := chainOf(mfd, "l/b/c/f", false); err == nil {
		t.Error("a chain was walked through a symbolic link")
	}
	if ids, _, err := chainOf(mfd, ".", true); err != nil || len(ids) != 0 {
		t.Errorf("the chain of the mount's root: %v, %v", ids, err)
	}
	// A start: two sources share the directory "a/b"; it is held once. A chain that cannot be walked fails it.
	srcs := []Source{{Rel: "a"}, {Rel: "a/b/c/f", ReadOnly: true, File: true}, {Rel: "a/b/c", ReadOnly: true}, {Rel: "x", ReadOnly: true}}
	a, out, err := startAnchors([]string{top}, srcs, map[fileID]bool{id("a"): true})
	if err != nil || len(out) != 2 || len(a.chains[1]) != 2 || len(a.chains[2]) != 2 || len(a.chains[3]) != 0 || len(a.held) != 2 {
		t.Fatalf("startAnchors = %v, %v", out, err)
	}
	// The directory at a read-only directory source's path is told as a read-only object too (TD-081): the leaf of
	// its chain. A file source has none, nor has a source that no read-write source holds; one directory is told once.
	if leaves := leafList(srcs, a.chains); len(leaves) != 1 || leaves[0] != (ProtectID{Dev: id("a/b/c").dev, Ino: id("a/b/c").ino}) {
		t.Errorf("leafList = %v; want a/b/c alone", leaves)
	}
	if leaves := leafList([]Source{{Rel: "p", ReadOnly: true}, {Rel: "P", ReadOnly: true}, {Rel: "f", File: true}}, [][]fileID{{{1, 2}, {1, 3}}, {{1, 3}}, {{1, 4}}, {{1, 5}}}); len(leaves) != 1 || leaves[0].Ino != 3 {
		t.Errorf("leafList of two sources at one directory, a file source and a chain with no source = %v; want one", leaves)
	}
	if !a.watched(srcs) || (*anchorState)(nil).watched(srcs) || a.watched([]Source{{Rel: "a"}}) {
		t.Error("watched: want it for read-only sources under a read-write root only")
	}
	a.close()
	gone := []Source{{Rel: "a"}, {Rel: "a/none/f", ReadOnly: true, File: true}}
	if _, _, err := startAnchors([]string{top}, gone, map[fileID]bool{id("a"): true}); err == nil {
		t.Error("a start with a chain that cannot be walked did not fail")
	}
	if !sameChain([]fileID{{1, 2}}, []fileID{{1, 2}}) || sameChain([]fileID{{1, 2}}, []fileID{{1, 3}}) || sameChain(nil, []fileID{{1, 2}}) {
		t.Error("sameChain")
	}
}

// The watcher tells the server of every current chain when one of them changed, tells it nothing otherwise, keeps
// what it last told for a chain that cannot be walked, and ends the server when the list cannot be written (TD-080).
// With the chains it tells the directory now at each read-only directory source's path, as a read-only object, and
// before the anchors (TD-081).
func TestCheckChains(t *testing.T) {
	top, mfd, id := anchorTree(t)
	srcs := []Source{{Rel: "a"}, {Rel: "a/b/c/f", ReadOnly: true, File: true}, {Rel: "a/conf", ReadOnly: true}}
	a, _, err := startAnchors([]string{top}, srcs, map[fileID]bool{id("a"): true})
	if err != nil {
		t.Fatal(err)
	}
	listFile := filepath.Join(t.TempDir(), "list")
	if err := WriteList(listFile, ""); err != nil {
		t.Fatal(err)
	}
	lw, err := OpenList(listFile)
	if err != nil {
		t.Fatal(err)
	}
	var logged []string
	ended := 0
	w := &fileWatch{srcs: srcs, mfds: []int{mfd}, anchors: a, list: lw, said: map[string]bool{},
		logf: func(format string, args ...any) { logged = append(logged, fmt.Sprintf(format, args...)) },
		end:  func() { ended++ }}
	lines := func() []string {
		raw, _ := os.ReadFile(listFile)
		return strings.Fields(strings.ReplaceAll(string(raw), "\n", " | "))
	}
	text := func() string { raw, _ := os.ReadFile(listFile); return string(raw) }

	// Nothing changed: nothing is told.
	w.checkChains()
	if text() != "" || len(logged) != 0 {
		t.Fatalf("an unchanged chain was told: %q %v", text(), logged)
	}
	// The host replaces a/b, and with it a/b/c and the file: the server is told of every anchor as it is now, the
	// one of the other source too.
	if err := os.Rename(filepath.Join(top, "a/b"), filepath.Join(top, "a/b.old")); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(filepath.Join(top, "a/b/c"), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(top, "a/b/c/f"), []byte("new"), 0o644); err != nil {
		t.Fatal(err)
	}
	w.checkChains()
	want := fmt.Sprintf("s %d %d\na %d %d\na %d %d\na %d %d\n", id("a/conf").dev, id("a/conf").ino, id("a/b").dev, id("a/b").ino, id("a/b/c").dev, id("a/b/c").ino, id("a/conf").dev, id("a/conf").ino)
	if text() != want || w.grown != len(want) || len(a.held) != 3 || a.chains[1][0] != id("a/b") || ended != 0 {
		t.Fatalf("after a replaced directory: list %v, want %q; grown %d, held %d, ended %d", lines(), want, w.grown, len(a.held), ended)
	}
	// The directory at a read-only directory source's path is replaced: that is a change too.
	if err := os.Rename(filepath.Join(top, "a/conf"), filepath.Join(top, "a/conf.old")); err != nil {
		t.Fatal(err)
	}
	if err := os.Mkdir(filepath.Join(top, "a/conf"), 0o755); err != nil {
		t.Fatal(err)
	}
	before := len(text())
	w.checkChains()
	newConf := fmt.Sprintf("%d %d\n", id("a/conf").dev, id("a/conf").ino)
	if got := text()[before:]; !strings.HasPrefix(got, "s "+newConf) || !strings.HasSuffix(got, "a "+newConf) || strings.Count(got, "\n") != 4 || strings.Count(got, "s ") != 1 {
		t.Fatalf("after a replaced source directory: %q; want the new directory as an object, then the three anchors", got)
	}
	// The directory is told again with every file that the watcher tells, so that it stays among the newest.
	before, grown := len(text()), w.grown
	if err := w.tellLeaves(); err != nil || text()[before:] != "s "+newConf || w.grown != grown+len("s "+newConf) {
		t.Fatalf("tellLeaves: %v, told %q, grown by %d", err, text()[before:], w.grown-grown)
	}
	if err := (&fileWatch{}).tellLeaves(); err != nil {
		t.Fatalf("tellLeaves with no anchors: %v", err)
	}
	// A chain that cannot be walked keeps what was last told, is logged once, and tells nothing.
	if err := os.Rename(filepath.Join(top, "a/b"), filepath.Join(top, "a/b.gone")); err != nil {
		t.Fatal(err)
	}
	before, logged = len(text()), nil
	w.checkChains()
	w.checkChains()
	if len(text()) != before || len(logged) != 1 || len(a.chains[1]) != 2 || len(a.held) != 3 {
		t.Fatalf("a chain that cannot be walked: %d bytes added, logged %v, chain %v, held %d", len(text())-before, logged, a.chains[1], len(a.held))
	}
	// Another chain changes while that one cannot be walked: the server is told of the new directory and, again,
	// of what was last told for the chain that cannot be walked, which stays held.
	kept := append([]fileID(nil), a.chains[1]...)
	replaceConf := func(old string) {
		t.Helper()
		if err := os.Rename(filepath.Join(top, "a/conf"), filepath.Join(top, old)); err != nil {
			t.Fatal(err)
		}
		if err := os.Mkdir(filepath.Join(top, "a/conf"), 0o755); err != nil {
			t.Fatal(err)
		}
	}
	replaceConf("a/conf.old2")
	before = len(text())
	w.checkChains()
	want = fmt.Sprintf("s %d %d\na %d %d\na %d %d\na %d %d\n", id("a/conf").dev, id("a/conf").ino, kept[0].dev, kept[0].ino, kept[1].dev, kept[1].ino, id("a/conf").dev, id("a/conf").ino)
	if got := text()[before:]; got != want || !sameChain(a.chains[1], kept) || len(a.held) != 3 || ended != 0 {
		t.Fatalf("a change beside a chain that cannot be walked: told %q, want %q; chain %v, held %d", got, want, a.chains[1], len(a.held))
	}
	for _, id := range kept {
		var st unix.Stat_t
		if fd, ok := a.held[id]; !ok || unix.Fstat(fd, &st) != nil || st.Ino != id.ino {
			t.Fatalf("the anchor %v of the chain that cannot be walked is not held", id)
		}
	}
	if err := os.Rename(filepath.Join(top, "a/b.gone"), filepath.Join(top, "a/b")); err != nil {
		t.Fatal(err)
	}
	// A list that has grown past its limit ends the server, after the server was told.
	w.grown = maxListGrowth
	replaceConf("a/conf.old3")
	before = len(text())
	w.checkChains()
	if ended != 1 || len(text()) == before {
		t.Fatalf("a list grown past its limit: the server was ended %d times, want once, and %d bytes were told", ended, len(text())-before)
	}
	ended, w.grown = 0, 0
	// A closed list (the server has ended) is passed over; a list that cannot be written ends the server.
	replaceConf("a/conf.old4")
	lw.Close()
	w.checkChains()
	if ended != 0 {
		t.Fatal("a closed list ended the server")
	}
	ro, err := os.Open(listFile)
	if err != nil {
		t.Fatal(err)
	}
	defer ro.Close()
	w.list = ro
	w.checkChains()
	if ended != 1 {
		t.Fatalf("a list that cannot be written: the server was ended %d times, want once", ended)
	}
	a.close()
	// The same with no read-only directory source, so that the first line that fails is an anchor's.
	fileOnly := srcs[:2]
	a2, _, err := startAnchors([]string{top}, fileOnly, map[fileID]bool{id("a"): true})
	if err != nil {
		t.Fatal(err)
	}
	defer a2.close()
	if leaves := leafList(fileOnly, a2.chains); len(leaves) != 0 {
		t.Fatalf("leaves of a server with no read-only directory source: %v", leaves)
	}
	ended = 0
	w2 := &fileWatch{srcs: fileOnly, mfds: []int{mfd}, anchors: a2, list: ro, said: map[string]bool{}, logf: func(string, ...any) {}, end: func() { ended++ }}
	if err := os.Rename(filepath.Join(top, "a/b"), filepath.Join(top, "a/b.old5")); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(filepath.Join(top, "a/b/c"), 0o755); err != nil {
		t.Fatal(err)
	}
	w2.checkChains()
	if ended != 1 {
		t.Fatalf("an anchor line that cannot be written: the server was ended %d times, want once", ended)
	}
}
