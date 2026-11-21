package unfs

import (
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	"golang.org/x/sys/unix"
)

// The cases of TD-082: the walk of a chain, the bound of the anchors, a symbolic link or a file that the host puts
// on a chain, a watcher that cannot start, and sources that spell one directory in two ways.

func openFDs(t *testing.T) int {
	t.Helper()
	ents, err := os.ReadDir("/proc/self/fd")
	if err != nil {
		t.Fatal(err)
	}
	return len(ents)
}

func lowerMaxAnchors(t *testing.T, n int) {
	t.Helper()
	was := maxAnchors
	maxAnchors = n
	t.Cleanup(func() { maxAnchors = was })
}

// A chain is one walk: each directory is opened in the one before it. A name on the way that is a regular file, a
// symbolic link or nothing fails the walk, the error names the place, and no descriptor stays open. A chain longer
// than the bound of the anchors is not walked.
func TestChainWalk(t *testing.T) {
	top, mfd, id := anchorTree(t)
	if err := os.Symlink("b", filepath.Join(top, "a/l")); err != nil {
		t.Fatal(err)
	}
	before := openFDs(t)
	for rel, want := range map[string]string{
		"a/b/c/f/g":   "a/b/c/f: ", // a regular file where a directory must be
		"a/l/c/f":     "a/l: ",     // a symbolic link, also one that stays beneath the mount
		"a/none/c/f":  "a/none: ",
		"a/b/c/f/g/h": "a/b/c/f: ",
	} {
		if _, _, err := chainOf(mfd, rel, false); err == nil || !strings.HasPrefix(err.Error(), want) {
			t.Errorf("chainOf(%q) = %v; want an error that begins %q", rel, err, want)
		}
	}
	// With leaf, the source's own directory is the last of the walk, and must be a directory.
	if _, _, err := chainOf(mfd, "a/b/c/f", true); err == nil || !strings.HasPrefix(err.Error(), "a/b/c/f: ") {
		t.Errorf("chainOf of a file with leaf = %v; want an error at the file", err)
	}
	if after := openFDs(t); after != before {
		t.Errorf("walks that failed left %d descriptors open", after-before)
	}
	ids, fds, err := chainOf(mfd, "a/b/c", true)
	if err != nil || len(ids) != 4 || ids[0] != id(".") || ids[3] != id("a/b/c") {
		t.Fatalf("chainOf(a/b/c, leaf) = %v, %v", ids, err)
	}
	// Each descriptor is the directory itself: a name looked up in it is found.
	var st unix.Stat_t
	if err := unix.Fstatat(fds[3], "f", &st, unix.AT_SYMLINK_NOFOLLOW); err != nil || st.Ino != id("a/b/c/f").ino {
		t.Errorf("the last directory of the walk does not hold the file: %v", err)
	}
	for _, fd := range fds {
		unix.Close(fd)
	}
	lowerMaxAnchors(t, 2)
	if _, _, err := chainOf(mfd, "a/b/c/f", false); err == nil || !strings.Contains(err.Error(), "at most 2") {
		t.Errorf("a chain of 3 directories with a bound of 2 = %v; want it refused", err)
	}
	if _, _, err := chainOf(mfd, "a/b/f", false); err != nil {
		t.Errorf("a chain of 2 directories with a bound of 2: %v", err)
	}
}

// More anchors than the bound: a start fails and says so, and the watcher keeps what it last told, says so once,
// and tells the server when the anchors are within the bound again. Nothing stays open after either.
func TestAnchorBound(t *testing.T) {
	top, mfd, id := anchorTree(t)
	if err := os.Mkdir(filepath.Join(top, "a/third"), 0o755); err != nil {
		t.Fatal(err)
	}
	// Three anchors, each two directories from the mount's root: the bound of a chain's length is not what is passed.
	srcs := []Source{{Rel: "a"}, {Rel: "a/b", ReadOnly: true}, {Rel: "a/third", ReadOnly: true}, {Rel: "a/conf", ReadOnly: true}}
	roots := map[fileID]bool{id("a"): true}
	before := openFDs(t)
	lowerMaxAnchors(t, 2)
	if _, _, err := startAnchors([]string{top}, srcs, roots); err == nil || !strings.Contains(err.Error(), "more than 2 directories") {
		t.Fatalf("a start with 3 anchors and a bound of 2 = %v; want it refused", err)
	}
	if after := openFDs(t); after != before {
		t.Fatalf("the refused start left %d descriptors open", after-before)
	}
	maxAnchors = 3
	a, told, err := startAnchors([]string{top}, srcs, roots)
	if err != nil || len(told) != 3 {
		t.Fatalf("a start with 3 anchors and a bound of 3 = %v, %v", told, err)
	}
	defer a.close()
	listFile := filepath.Join(t.TempDir(), "list")
	if err := WriteList(listFile, ""); err != nil {
		t.Fatal(err)
	}
	lw, err := OpenList(listFile)
	if err != nil {
		t.Fatal(err)
	}
	defer lw.Close()
	var logged []string
	w := &fileWatch{srcs: srcs, mfds: []int{mfd}, anchors: a, list: lw, said: map[string]bool{},
		logf: func(format string, args ...any) { logged = append(logged, fmt.Sprintf(format, args...)) }, end: func() { t.Error("the server was ended") }}
	text := func() string { raw, _ := os.ReadFile(listFile); return string(raw) }
	// The host replaces a directory while the bound is passed (the bound is lowered here in place of 1,025 directories).
	maxAnchors = 2
	if err := os.Rename(filepath.Join(top, "a/conf"), filepath.Join(top, "a/conf.old")); err != nil {
		t.Fatal(err)
	}
	if err := os.Mkdir(filepath.Join(top, "a/conf"), 0o755); err != nil {
		t.Fatal(err)
	}
	old := append([]fileID(nil), a.chains[3]...)
	open := openFDs(t)
	w.checkChains()
	w.checkChains()
	if text() != "" || len(logged) != 1 || !strings.Contains(logged[0], "more than 2 directories") || !sameChain(a.chains[3], old) || len(a.held) != 3 {
		t.Fatalf("past the bound: told %q, logged %v, chain %v, held %d; want nothing told, one line, and what was last told kept", text(), logged, a.chains[3], len(a.held))
	}
	if after := openFDs(t); after != open {
		t.Fatalf("the checks past the bound left %d descriptors open", after-open)
	}
	// Within the bound again: the change is told, and a later passing of the bound is news again.
	maxAnchors = 3
	w.checkChains()
	if !strings.Contains(text(), fmt.Sprintf("a %d %d\n", id("a/conf").dev, id("a/conf").ino)) || a.chains[3][0] != id("a/conf") || w.said["anchors"] {
		t.Fatalf("within the bound again: told %q, chain %v", text(), a.chains[3])
	}
}

// The host puts a symbolic link, or a regular file, where a directory of a chain was: the chain cannot be walked,
// so the watcher tells nothing of what the link names, keeps and holds what it last told, and says so once. When a
// directory is there again, the server is told of it.
func TestCheckChainsWithALinkOrAFileOnTheWay(t *testing.T) {
	for _, kind := range []string{"link", "file"} {
		t.Run(kind, func(t *testing.T) {
			top, mfd, id := anchorTree(t)
			srcs := []Source{{Rel: "a"}, {Rel: "a/b/c/f", ReadOnly: true, File: true}}
			a, _, err := startAnchors([]string{top}, srcs, map[fileID]bool{id("a"): true})
			if err != nil {
				t.Fatal(err)
			}
			defer a.close()
			listFile := filepath.Join(t.TempDir(), "list")
			if err := WriteList(listFile, ""); err != nil {
				t.Fatal(err)
			}
			lw, err := OpenList(listFile)
			if err != nil {
				t.Fatal(err)
			}
			defer lw.Close()
			var logged []string
			w := &fileWatch{srcs: srcs, mfds: []int{mfd}, anchors: a, list: lw, said: map[string]bool{},
				logf: func(format string, args ...any) { logged = append(logged, fmt.Sprintf(format, args...)) }, end: func() { t.Error("the server was ended") }}
			text := func() string { raw, _ := os.ReadFile(listFile); return string(raw) }
			kept := append([]fileID(nil), a.chains[1]...)
			// A decoy tree that the link names: it must never be told as the chain.
			if err := os.MkdirAll(filepath.Join(top, "x/c"), 0o755); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(filepath.Join(top, "x/c/f"), []byte("decoy"), 0o644); err != nil {
				t.Fatal(err)
			}
			if err := os.Rename(filepath.Join(top, "a/b"), filepath.Join(top, "a/b.real")); err != nil {
				t.Fatal(err)
			}
			if kind == "link" {
				err = os.Symlink("../x", filepath.Join(top, "a/b"))
			} else {
				err = os.WriteFile(filepath.Join(top, "a/b"), []byte("not a directory"), 0o644)
			}
			if err != nil {
				t.Fatal(err)
			}
			w.checkChains()
			w.checkChains()
			if text() != "" || len(logged) != 1 || !strings.Contains(logged[0], "cannot be looked at now") || !sameChain(a.chains[1], kept) {
				t.Fatalf("with a %s on the way: told %q, logged %v, chain %v; want nothing told, one line, and the chain kept", kind, text(), logged, a.chains[1])
			}
			for _, k := range kept {
				var st unix.Stat_t
				if fd, ok := a.held[k]; !ok || unix.Fstat(fd, &st) != nil || st.Ino != k.ino {
					t.Fatalf("the anchor %v is not held while a %s is on the way", k, kind)
				}
			}
			// A directory is at the name again, another one than before: the server is told of the chain as it is.
			if err := os.Remove(filepath.Join(top, "a/b")); err != nil {
				t.Fatal(err)
			}
			if err := os.MkdirAll(filepath.Join(top, "a/b/c"), 0o755); err != nil {
				t.Fatal(err)
			}
			w.checkChains()
			want := fmt.Sprintf("a %d %d\na %d %d\n", id("a/b").dev, id("a/b").ino, id("a/b/c").dev, id("a/b/c").ino)
			if text() != want {
				t.Fatalf("with a directory there again: told %q, want %q", text(), want)
			}
		})
	}
}

// A server that needs a watcher and cannot have one is ended, and the start fails; a server that needs none goes
// on with nothing to stop.
func TestWatchServer(t *testing.T) {
	start := func() *exec.Cmd {
		t.Helper()
		cmd := exec.Command("sleep", "60")
		if err := cmd.Start(); err != nil {
			t.Skipf("no sleep command to stand for the server: %v", err)
		}
		return cmd
	}
	cmd := start()
	cannot := errors.New("the sandbox's process: gone")
	var gotPid int
	stop, err := watchServer(Config{}, nil, cmd, func(_ Config, _ []*os.File, pid int) (*fileWatch, error) {
		gotPid = pid
		return nil, cannot
	})
	if stop != nil || !errors.Is(err, cannot) || !strings.Contains(err.Error(), "the watcher of the sources cannot start") || gotPid != cmd.Process.Pid {
		t.Fatalf("watchServer with a watcher that cannot start = %v (pid %d); want the start's error", err, gotPid)
	}
	if cmd.ProcessState == nil || cmd.ProcessState.Success() {
		t.Fatalf("the server was not ended: %v", cmd.ProcessState)
	}
	cmd = start()
	defer func() { _ = cmd.Process.Kill(); _ = cmd.Wait() }()
	stop, err = watchServer(Config{}, nil, cmd, func(Config, []*os.File, int) (*fileWatch, error) { return nil, nil })
	if err != nil || stop == nil {
		t.Fatalf("watchServer with no watcher needed = %v", err)
	}
	stop()
	if err := cmd.Process.Signal(unix.Signal(0)); err != nil {
		t.Fatalf("a server that needs no watcher was ended: %v", err)
	}
}

// The part of a path that leads to the root of a read-write directory source is written as that source writes it.
func TestSpelledAs(t *testing.T) {
	r, m6, proj, conf, sub := fileID{1, 1}, fileID{1, 2}, fileID{1, 3}, fileID{1, 4}, fileID{1, 5}
	roots := map[rootKey]string{{0, proj}: "m6/proj"}
	for _, c := range []struct {
		name  string
		mount int
		rel   string
		ids   []fileID
		roots map[rootKey]string
		want  string
	}{
		{"a directory inside the root", 0, "M6/PROJ/conf", []fileID{r, m6, proj, conf}, roots, "m6/proj/conf"},
		{"a file inside the root", 0, "M6/PROJ/conf/app.env", []fileID{r, m6, proj, conf}, roots, "m6/proj/conf/app.env"},
		{"a file right in the root", 0, "M6/PROJ/f", []fileID{r, m6, proj}, roots, "m6/proj/f"},
		{"the root's own directory", 0, "M6/PROJ", []fileID{r, m6, proj}, roots, "m6/proj"},
		{"one spelling already", 0, "m6/proj/conf", []fileID{r, m6, proj, conf}, roots, "m6/proj/conf"},
		{"no root on the way", 0, "M6/other/conf", []fileID{r, m6, sub, conf}, roots, "M6/other/conf"},
		{"a root of another share mount", 1, "M6/PROJ/conf", []fileID{r, m6, proj, conf}, roots, "M6/PROJ/conf"},
		{"the deepest root counts", 0, "M6/PROJ/CONF/x", []fileID{r, m6, proj, conf, sub}, map[rootKey]string{{0, proj}: "m6/proj", {0, conf}: "m6/proj/conf"}, "m6/proj/conf/x"},
		{"the share itself is the root", 0, "a/b", []fileID{r, m6, proj}, map[rootKey]string{{0, r}: "."}, "a/b"},
		{"a root inside the share that is a root", 0, "M6/PROJ/x", []fileID{r, m6, proj, sub}, map[rootKey]string{{0, r}: ".", {0, proj}: "m6/proj"}, "m6/proj/x"},
	} {
		got, changed := spelledAs(c.mount, c.rel, c.ids, c.roots)
		if got != c.want || changed != (c.want != c.rel) {
			t.Errorf("%s: spelledAs(%q) = %q, %v; want %q", c.name, c.rel, got, changed, c.want)
		}
	}
}

// Sources with one spelling stay as they were given, and nothing stays open. On a volume that ignores letter case
// (a macOS share; the test is skipped where the two spellings are two names), sources that spell one directory in
// two ways get the spelling of the read-write source that holds them, so that the nests, the holders and a
// directory bound both ways are found by the text.
func TestRespell(t *testing.T) {
	top := t.TempDir()
	for _, d := range []string{"m6/proj/conf/deep", "m6/other"} {
		if err := os.MkdirAll(filepath.Join(top, d), 0o755); err != nil {
			t.Fatal(err)
		}
	}
	if err := os.WriteFile(filepath.Join(top, "m6/proj/conf/app.env"), []byte("x"), 0o644); err != nil {
		t.Fatal(err)
	}
	given := []Source{{Rel: "m6/proj"}, {Rel: "m6/proj/conf", ReadOnly: true}, {Rel: "m6/proj/conf/app.env", ReadOnly: true, File: true}, {Rel: "m6/other", ReadOnly: true}, {Rel: "m6/none", ReadOnly: true}}
	before := openFDs(t)
	got, notes := respell([]string{top}, given)
	if len(notes) != 0 || fmt.Sprint(got) != fmt.Sprint(given) {
		t.Fatalf("sources with one spelling: %v, notes %v", got, notes)
	}
	if after := openFDs(t); after != before {
		t.Fatalf("respell left %d descriptors open", after-before)
	}
	if got, notes := respell([]string{filepath.Join(top, "absent")}, given); len(notes) != 0 || fmt.Sprint(got) != fmt.Sprint(given) {
		t.Fatalf("a share mount that cannot be opened: %v, notes %v", got, notes)
	}
	if _, err := os.Stat(filepath.Join(top, "M6/PROJ")); err != nil {
		t.Skip("this volume keeps letter case apart: the two spellings are two names")
	}
	two := []Source{
		{Rel: "M6/Proj/Conf/Deep"},                                // read-write, inside the next one: it takes that one's spelling
		{Rel: "m6/proj"},                                          // the outermost read-write source keeps its own
		{Rel: "M6/PROJ/conf", ReadOnly: true},                     // a read-only directory inside it
		{Rel: "M6/PROJ/CONF/app.env", ReadOnly: true, File: true}, // a read-only file inside it
		{Rel: "M6/PROJ", ReadOnly: true},                          // the same directory, read-only
		{Rel: "M6/PROJ/CONF/DEEP/x", File: true},                  // inside the inner read-write source
		{Rel: "M6/other", ReadOnly: true},                         // inside no read-write source: as given
		{Rel: "m6/PROJ", Mount: 1, ReadOnly: true},                // on a share mount that is not there
	}
	want := []string{"m6/proj/Conf/Deep", "m6/proj", "m6/proj/conf", "m6/proj/CONF/app.env", "m6/proj", "m6/proj/Conf/Deep/x", "M6/other", "m6/PROJ"}
	got, notes = respell([]string{top}, two)
	for i, s := range got {
		if s.Rel != want[i] || s.ReadOnly != two[i].ReadOnly || s.File != two[i].File || s.Mount != two[i].Mount {
			t.Errorf("source %d %q: taken as %+v; want %q and nothing else changed", i, two[i].Rel, s, want[i])
		}
	}
	if len(notes) != 5 {
		t.Errorf("notes = %q; want one for each of the 5 sources written anew", notes)
	}
	// By the text, as the rest of the start goes: the read-only directory nests in the read-write one, the file has
	// a holder, and the directory bound both ways has no protected place.
	if n := Nests(got); len(n) == 0 || n[0].Outer != 1 || n[0].Inner != 2 || n[0].Sub != "conf" {
		t.Errorf("Nests after the respelling = %+v; want the read-only directory in the read-write one", n)
	}
	if h := Holders(got, 3); len(h) != 1 || h[0].Outer != 1 {
		t.Errorf("Holders of the read-only file after the respelling = %+v; want the read-write source", h)
	}
	if hasPlace(got, got[4]) || !hasPlace(got, got[2]) {
		t.Error("hasPlace after the respelling: want none for the directory bound both ways, one for the read-only directory")
	}
	if after := openFDs(t); after != before {
		t.Fatalf("respell left %d descriptors open", after-before)
	}
}

// A limit of open files that covers the need is left alone; a need that no limit can cover fails, and says both
// numbers. The need grows with the trees and the sources, and holds three sets of anchors.
func TestEnsureFiles(t *testing.T) {
	var before, after unix.Rlimit
	if err := unix.Getrlimit(unix.RLIMIT_NOFILE, &before); err != nil {
		t.Fatal(err)
	}
	if err := ensureFiles(1); err != nil {
		t.Fatalf("a need of 1: %v", err)
	}
	if err := ensureFiles(before.Cur); err != nil {
		t.Fatalf("a need of the limit itself: %v", err)
	}
	err := ensureFiles(1 << 40)
	if err == nil || !strings.Contains(err.Error(), fmt.Sprintf("can need %d open files; the limit is %d", uint64(1)<<40, before.Cur)) {
		t.Fatalf("a need that no limit covers = %v; want an error with both numbers", err)
	}
	if err := unix.Getrlimit(unix.RLIMIT_NOFILE, &after); err != nil || after != before {
		t.Fatalf("the limit changed from %+v to %+v (%v)", before, after, err)
	}
	if got, want := filesNeeded(128, 64), uint64(128+3*1024+1+256+64+64); got != want {
		t.Errorf("filesNeeded(128, 64) = %d; want %d", got, want)
	}
	if filesNeeded(129, 64) != filesNeeded(128, 64)+1 || filesNeeded(128, 65) != filesNeeded(128, 64)+1 {
		t.Error("filesNeeded does not grow by one with a tree or a source")
	}
}
