package unfs

import (
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
	for _, in := range []string{"app", "app:", "app:rwx", ":ro", "/abs:rw", "../up:rw", "a/../b:ro", "a//b:rw", "a/./b:rw", "./a:rw", "a/:rw", "a\x00b:rw"} {
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
