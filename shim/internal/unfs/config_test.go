package unfs

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
)

func TestParseSource(t *testing.T) {
	good := map[string]Source{
		".:rw":         {Rel: ".", ReadOnly: false},
		"app:ro":       {Rel: "app", ReadOnly: true},
		"a b/c:d:rw":   {Rel: "a b/c:d", ReadOnly: false},
		"x/$(id)/;:ro": {Rel: "x/$(id)/;", ReadOnly: true},
	}
	for in, want := range good {
		got, err := ParseSource(in)
		if err != nil || got != want {
			t.Errorf("ParseSource(%q) = %+v, %v; want %+v", in, got, err, want)
		}
	}
	for in, want := range map[string]Source{"a.txt:rof": {Rel: "a.txt", ReadOnly: true, File: true}, "a.txt:rwf": {Rel: "a.txt", File: true}} {
		if got, err := ParseSource(in); err != nil || got != want || got.Arg() != in {
			t.Errorf("ParseSource(%q) = %+v, %v", in, got, err)
		}
	}
	for _, in := range []string{".:rwf", ".:rof", "a:rf", "app", "app:", "app:rwx", ":ro", "/abs:rw", "../up:rw", "a/../b:ro", "a//b:rw", "a/./b:rw", "./a:rw", "a/:rw", "a\x00b:rw"} {
		if _, err := ParseSource(in); err == nil {
			t.Errorf("ParseSource(%q) accepted", in)
		}
	}
	if err := CheckRel(strings.Repeat("a/", 3000) + "a"); err == nil {
		t.Error("an over-long path was accepted")
	}
}

func TestNests(t *testing.T) {
	srcs := []Source{
		{Rel: "proj"},                        // 0 rw
		{Rel: "proj/conf", ReadOnly: true},   // 1 ro inside 0
		{Rel: "proj/conf/x", ReadOnly: true}, // 2 ro inside 0 (and inside ro 1, which needs nothing)
		{Rel: "projector", ReadOnly: true},   // 3 a name prefix, not inside 0
		{Rel: "proj", ReadOnly: true},        // 4 the same directory as 0: 0 stays writable
		{Rel: "."},                           // 5 rw, the whole share
		{Rel: "other", ReadOnly: true},       // 6 inside 5 only
	}
	// Order: shallowest Sub first, then by outer.
	want := []Nest{
		{Outer: 0, Inner: 1, Sub: "conf"},
		{Outer: 5, Inner: 3, Sub: "projector"},
		{Outer: 5, Inner: 4, Sub: "proj"},
		{Outer: 5, Inner: 6, Sub: "other"},
		{Outer: 0, Inner: 2, Sub: "conf/x"},
		{Outer: 5, Inner: 1, Sub: "proj/conf"},
		{Outer: 5, Inner: 2, Sub: "proj/conf/x"},
	}
	if got := Nests(srcs); !reflect.DeepEqual(got, want) {
		t.Errorf("Nests:\n got %+v\nwant %+v", got, want)
	}
	if got := Nests([]Source{{Rel: "a.txt", File: true}, {Rel: "a.txt", ReadOnly: true, File: true}}); len(got) != 0 {
		t.Errorf("a file is never the outer of a nest, got %+v", got)
	}
	if got := Nests([]Source{{Rel: "a", ReadOnly: true}, {Rel: "a/b"}}); len(got) != 0 {
		t.Errorf("a rw source inside a ro one needs no nest, got %+v", got)
	}
}

func TestExports(t *testing.T) {
	srcs := []Source{{Rel: "a b"}, {Rel: "c;rm -rf /", ReadOnly: true}}
	want := "/s/0 169.254.77.2(rw,no_root_squash)\n/s/1 169.254.77.2(ro,no_root_squash)\n"
	if got := Exports(srcs, false); got != want {
		t.Errorf("Exports root:\n%s", got)
	}
	want = "/s/0 169.254.77.2(rw,all_squash,anonuid=65534,anongid=65534)\n/s/1 169.254.77.2(ro,all_squash,anonuid=65534,anongid=65534)\n"
	if got := Exports(srcs, true); got != want {
		t.Errorf("Exports squash:\n%s", got)
	}
}

func TestCheck(t *testing.T) {
	if Check(nil) == nil {
		t.Error("no sources accepted")
	}
	many := make([]Source, MaxSources+1)
	for i := range many {
		many[i] = Source{Rel: "a"}
	}
	if Check(many) == nil {
		t.Error("too many sources accepted")
	}
	if Check([]Source{{Rel: "../x"}}) == nil {
		t.Error("a .. source accepted")
	}
}

func TestUnfsdArgs(t *testing.T) {
	got := strings.Join(UnfsdArgs("/etc/exports"), " ")
	if got != "-d -p -t -l 169.254.77.1 -n 2049 -m 2050 -e /etc/exports" {
		t.Errorf("UnfsdArgs = %s", got)
	}
}

// GH #207: a version 2 record round-trips; a version 1 record still reads; bad records are refused whole.
func TestArgs(t *testing.T) {
	a := Args{Mounts: []string{"/m", "/n x"}, Squash: true, Sources: []Source{{Rel: "a"}, {Rel: "b:c", ReadOnly: true}, {Mount: 1, Rel: "."}}}
	raw, err := a.Format()
	if err != nil {
		t.Fatal(err)
	}
	if raw != ArgsHeader+"\nsquash 1\nmount /m\nsrc a:rw\nsrc b:c:ro\nmount /n x\nsrc .:rw\n" {
		t.Errorf("format %q", raw)
	}
	if got, err := ParseArgs(raw); err != nil || !reflect.DeepEqual(got, a) {
		t.Errorf("round trip %+v %v", got, err)
	}
	if got, err := ParseArgs("/m\n0\na:rw\n"); err != nil || !reflect.DeepEqual(got, Args{Mounts: []string{"/m"}, Sources: []Source{{Rel: "a"}}}) {
		t.Errorf("v1 %+v %v", got, err)
	}
	for name, bad := range map[string]string{
		"v1 bad source":     "/m\n0\na:rw\nnope\n",
		"v1 relative mount": "m\n0\na:rw\n",
		"no squash":         ArgsHeader + "\nmount /m\nsrc a:rw\n",
		"src before mount":  ArgsHeader + "\nsquash 0\nsrc a:rw\nmount /m\n",
		"unused mount":      ArgsHeader + "\nsquash 0\nmount /m\nsrc a:rw\nmount /n\n",
		"same mount twice":  ArgsHeader + "\nsquash 0\nmount /m\nsrc a:rw\nmount /m\nsrc b:rw\n",
		"unknown line":      ArgsHeader + "\nsquash 0\nmount /m\nsrc a:rw\nexec /bin/sh\n",
		"bad source":        ArgsHeader + "\nsquash 0\nmount /m\nsrc ../a:rw\n",
	} {
		if _, err := ParseArgs(bad); err == nil {
			t.Errorf("%s: accepted", name)
		}
	}
	if err := (Args{Mounts: []string{"/m", "/n"}, Sources: []Source{{Mount: 1, Rel: "a"}, {Rel: "b"}}}).Check(); err == nil {
		t.Error("sources out of mount order were accepted")
	}
	if err := CheckRel("a\nb"); err == nil {
		t.Error("a newline in a source path was accepted")
	}
}

// Sources on two mounts never nest, whatever their relative paths (council ARC-8).
func TestNestsSameMountOnly(t *testing.T) {
	if got := Nests([]Source{{Rel: "a"}, {Mount: 1, Rel: "a/b", ReadOnly: true}}); len(got) != 0 {
		t.Errorf("a nest across mounts: %+v", got)
	}
}

// A read-write directory source that holds a file source's path lets a task change what the path names (D-052).
func TestHolders(t *testing.T) {
	srcs := []Source{
		{Rel: "proj"},                                      // 0 read-write directory
		{Rel: "proj/x/secret", File: true},                 // 1 read-write file inside it
		{Rel: "proj/y/secret", File: true, ReadOnly: true}, // 2 read-only file inside it
		{Rel: "proj/ro", ReadOnly: true},                   // 3 read-only directory
		{Rel: "proj/ro/f", File: true},                     // 4 a file inside the read-only directory too
		{Rel: "other/f", File: true},                       // 5 held by nothing
		{Mount: 1, Rel: "proj/x/secret", File: true},       // 6 the same path on another share
		{Rel: "."}, // 7 the whole share, read-write
	}
	for i, want := range map[int]string{1: "0:x/secret 7:proj/x/secret", 2: "0:y/secret 7:proj/y/secret", 4: "0:ro/f 7:proj/ro/f", 5: "7:other/f", 6: "", 0: "7:proj"} {
		var got []string
		for _, h := range Holders(srcs, i) {
			if h.Inner != i {
				t.Errorf("source %d: holder names source %d", i, h.Inner)
			}
			got = append(got, fmt.Sprintf("%d:%s", h.Outer, h.Sub))
		}
		if g := strings.Join(got, " "); g != want {
			t.Errorf("source %d: holders %q, want %q", i, g, want)
		}
	}
}

// The server's protect file names the root of each read-only source and the objects remembered by number (TD-073).
func TestProtectList(t *testing.T) {
	srcs := []Source{{Rel: "proj"}, {Rel: "proj/conf", ReadOnly: true}, {Rel: "proj/key", File: true, ReadOnly: true}, {Rel: "proj/a", File: true}}
	got := ProtectList(srcs, []ProtectID{{Dev: 34, Ino: 1460}, {Dev: 36, Ino: 5, Swapped: true}}, nil)
	if want := "p /s/1\np /s/2/f\ni 34 1460\ni 36 5\n"; got != want {
		t.Errorf("protect list %q, want %q", got, want)
	}
	if got := ProtectList(srcs, nil, []ProtectID{{Dev: 40, Ino: 9}}); !strings.HasSuffix(got, "\ns 40 9\n") || strings.Contains(got, "\ni ") {
		t.Errorf("a list with the object at a place: %q", got)
	}
	if got := ProtectList([]Source{{Rel: "proj"}}, nil, nil); got != "" {
		t.Errorf("nothing read-only, list %q", got)
	}
}

// The record keeps each object once across restarts of the server. What a start saw is never pushed out by later
// entries: a task can add entries (it renames directories, and a file replaces a read-only file), and the first
// ones are what the user bound (council SEC-4).
// recordText is the record as RecordProtectIDs writes it.
func recordText(ids []ProtectID) string {
	var b strings.Builder
	for _, id := range ids {
		fmt.Fprintf(&b, "%d %d", id.Dev, id.Ino)
		if id.Swapped {
			b.WriteString(" r")
		}
		b.WriteString("\n")
	}
	return b.String()
}

func TestRecordProtectIDs(t *testing.T) {
	file := filepath.Join(t.TempDir(), "x.prot")
	id := func(dev, ino uint64) ProtectID { return ProtectID{Dev: dev, Ino: ino} }
	swap := func(dev, ino uint64) ProtectID { return ProtectID{Dev: dev, Ino: ino, Swapped: true} }
	if all, err := RecordProtectIDs(file, id(1, 2), id(1, 3)); err != nil || len(all) != 2 {
		t.Fatalf("first record: %v, %v", all, err)
	}
	all, err := RecordProtectIDs(file, id(1, 3), swap(9, 9), swap(1, 2))
	if err != nil || !reflect.DeepEqual(all, []ProtectID{id(1, 2), id(1, 3), swap(9, 9)}) {
		t.Fatalf("second record: %v, %v", all, err)
	}
	if again, _ := ReadProtectIDs(file); !reflect.DeepEqual(again, all) {
		t.Errorf("read back %v, want %v", again, all)
	}
	if st, err := os.Stat(file); err != nil || st.Mode().Perm() != 0o600 {
		t.Errorf("the record's mode: %v, %v", st, err)
	}
	if err := os.WriteFile(file, []byte("1 2\njunk\n3\n-4 5\n7 8 r\n7 9 x\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	if all, _ := RecordProtectIDs(file); !reflect.DeepEqual(all, []ProtectID{id(1, 2), swap(7, 8)}) {
		t.Errorf("a record with bad lines: %v", all)
	}
	// The originals stay, however many come after.
	var many []ProtectID
	for i := 0; i < MaxProtectStarts+40; i++ {
		many = append(many, id(1, uint64(i)))
	}
	for i := 0; i < MaxProtectSwaps+40; i++ {
		many = append(many, swap(2, uint64(i)))
	}
	got := MergeProtectIDs(nil, many...)
	if len(got) != MaxProtectStarts+MaxProtectSwaps || !got[0].same(id(1, 0)) || !got[MaxProtectStarts-1].same(id(1, uint64(MaxProtectStarts-1))) {
		t.Errorf("%d kept, first %v, last start %v", len(got), got[0], got[MaxProtectStarts-1])
	}
	if last := got[len(got)-1]; !last.same(swap(2, uint64(MaxProtectSwaps+39))) || !got[MaxProtectStarts].same(swap(2, 40)) {
		t.Errorf("replaced files kept from %v to %v, want the newest %d", got[MaxProtectStarts], last, MaxProtectSwaps)
	}
	// A replaced file that is recorded again is the newest again.
	if got := MergeProtectIDs([]ProtectID{swap(5, 5), swap(6, 6), swap(7, 7)}, swap(5, 5)); !reflect.DeepEqual(got, []ProtectID{swap(6, 6), swap(7, 7), swap(5, 5)}) {
		t.Errorf("a replaced file recorded again: %v", got)
	}
	// With the start numbers full, a replaced file that a start finds as a source stays a replaced file, through
	// this merge and the next.
	full := append([]ProtectID(nil), many[:MaxProtectStarts]...)
	full = append(full, swap(5, 5))
	once := MergeProtectIDs(full, id(5, 5))
	twice := MergeProtectIDs(ParseProtectIDs(recordText(once)), id(1, 0))
	for _, got := range [][]ProtectID{once, twice} {
		if len(got) != MaxProtectStarts+1 || !reflect.DeepEqual(got[len(got)-1], swap(5, 5)) {
			t.Errorf("a replaced file found as a source with the start numbers full: %d kept, last %v", len(got), got[len(got)-1])
		}
	}
	// A replaced file that a later start finds as a source is kept as a source.
	if got := MergeProtectIDs([]ProtectID{id(1, 1), swap(5, 5), swap(6, 6)}, id(5, 5)); !reflect.DeepEqual(got, []ProtectID{id(1, 1), id(5, 5), swap(6, 6)}) {
		t.Errorf("a replaced file seen at a start: %v", got)
	}
	// Nothing new: no file is written.
	none := filepath.Join(t.TempDir(), "none.prot")
	if all, err := RecordProtectIDs(none); err != nil || len(all) != 0 {
		t.Errorf("an empty record: %v, %v", all, err)
	}
	if _, err := os.Stat(none); !os.IsNotExist(err) {
		t.Errorf("an empty record made a file: %v", err)
	}
	if all, err := RecordProtectIDs("", id(4, 4)); err != nil || len(all) != 1 {
		t.Errorf("no record file: %v, %v", all, err)
	}
	if all, err := ReadProtectIDs(filepath.Join(t.TempDir(), "absent")); err != nil || len(all) != 0 {
		t.Errorf("an absent record: %v, %v", all, err)
	}
}

// The server's list: written whole at a start, added to through one descriptor while the server runs (TD-078).
func TestListFile(t *testing.T) {
	if ListFile("") != "" || ListFile("/x/a.prot") != "/x/a.prot.list" {
		t.Errorf("ListFile: %q, %q", ListFile(""), ListFile("/x/a.prot"))
	}
	file := filepath.Join(t.TempDir(), "a.prot.list")
	if _, err := OpenList(file); err == nil {
		t.Error("a list that is not there opened")
	}
	if err := AppendSeen(nil, ProtectID{Dev: 1, Ino: 2}); err == nil {
		t.Error("a line was added to no list")
	}
	// A temporary file left by a start that did not finish, with a mode a squashing server could not read.
	if err := os.WriteFile(filepath.Join(filepath.Dir(file), ".a.prot.list.tmp"), []byte("old"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := WriteList(file, "p /s/1/f\ni 7 8\n"); err != nil {
		t.Fatal(err)
	}
	st, err := os.Stat(file)
	if err != nil || st.Mode().Perm() != 0o644 {
		t.Fatalf("the list's mode: %v, %v", st, err)
	}
	lw, err := OpenList(file)
	if err != nil {
		t.Fatal(err)
	}
	defer lw.Close()
	for _, id := range []ProtectID{{Dev: 40, Ino: 1506}, {Dev: 40, Ino: 1507, Swapped: true}} {
		if err := AppendSeen(lw, id); err != nil {
			t.Fatal(err)
		}
	}
	// An object that is told again gets a line again: the server makes it the newest by that line (TD-079).
	if err := AppendSeen(lw, ProtectID{Dev: 40, Ino: 1506}); err != nil {
		t.Fatal(err)
	}
	raw, _ := os.ReadFile(file)
	if string(raw) != "p /s/1/f\ni 7 8\ns 40 1506\ns 40 1507\ns 40 1506\n" {
		t.Errorf("the list after three lines were added: %q", raw)
	}
	after, _ := os.Stat(file)
	if !os.SameFile(st, after) {
		t.Error("adding a line made another file: a server that has the list attached would not see the line")
	}
	// A start writes a new file: a server that still has the old one attached does not see it change, and the
	// descriptor of the old one does not write into the new one.
	if err := WriteList(file, "p /s/1/f\n"); err != nil {
		t.Fatal(err)
	}
	if again, _ := os.Stat(file); os.SameFile(st, again) {
		t.Error("a start wrote into the file an earlier server may still have attached")
	}
	if err := AppendSeen(lw, ProtectID{Dev: 1, Ino: 1}); err != nil {
		t.Fatal(err)
	}
	if raw, _ := os.ReadFile(file); string(raw) != "p /s/1/f\n" {
		t.Errorf("a line for the old list went into the new one: %q", raw)
	}
	// A symbolic link at the list's name is not followed, and a closed list reports that it is closed.
	link := filepath.Join(t.TempDir(), "link.list")
	if err := os.Symlink(file, link); err != nil {
		t.Fatal(err)
	}
	if f, err := OpenList(link); err == nil {
		f.Close()
		t.Error("a symbolic link at the list's name was followed")
	}
	lw.Close()
	if err := AppendSeen(lw, ProtectID{Dev: 1, Ino: 1}); !errors.Is(err, os.ErrClosed) {
		t.Errorf("a line added to a closed list: %v", err)
	}
}

// A read-write source the user put inside a read-only directory source stays writable; any other read-write source
// is compared with the record at a restart (council SEC-3).
func TestMovedUnder(t *testing.T) {
	srcs := []Source{
		{Rel: "proj", ReadOnly: true},               // 0
		{Rel: "proj/out"},                           // 1 inside the read-only directory by the user's own paths
		{Rel: "work"},                               // 2
		{Rel: "work/x/key", File: true},             // 3
		{Rel: "proj/f", File: true, ReadOnly: true}, // 4 read-only itself
		{Mount: 1, Rel: "proj/out"},                 // 5 the same path on another share
		{Rel: "cfg", File: true, ReadOnly: true},    // 6 a read-only file holds nothing
		{Rel: "cfg/x"},                              // 7
	}
	if got := MovedUnder(srcs); !reflect.DeepEqual(got, []int{2, 3, 5, 7}) {
		t.Errorf("sources to compare with the record: %v, want [2 3 5 7]", got)
	}
}
