package shares

import (
	"errors"
	"reflect"
	"strings"
	"testing"

	specs "github.com/opencontainers/runtime-spec/specs-go"

	"github.com/cityhunteur/vesper/shim/internal/unfs"
)

var fake = Resolver{
	Resolve: func(p string) (string, error) {
		switch p {
		case "/Users/me/proj-link":
			return "/Users/me/proj", nil
		case "/missing":
			return "", errors.New("no such file")
		}
		return p, nil
	},
	IsDir: func(p string) (bool, error) { return !strings.HasSuffix(p, ".txt"), nil },
}

func TestMakePlan(t *testing.T) {
	mounts := []specs.Mount{
		{Destination: "/proc", Type: "proc", Source: "proc"},
		{Destination: "/app", Type: "bind", Source: "/Users/me/proj", Options: []string{"rbind", "rw"}},
		{Destination: "/app/conf", Type: "bind", Source: "/Users/me/proj/conf", Options: []string{"rbind", "ro"}},
		{Destination: "/again", Source: "/Users/me/proj-link", Options: []string{"bind"}},       // same source, same mode
		{Destination: "/etc/hosts", Type: "bind", Source: "/var/lib/docker/containers/x/hosts"}, // not a share
		{Destination: "/gone", Type: "bind", Source: "/missing"},
	}
	p, err := MakePlan(mounts, []string{"/Users/me", "/Volumes/other"}, fake)
	if err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(p.Mounts, []string{"/Users/me"}) {
		t.Errorf("mounts %q", p.Mounts)
	}
	wantSrc := []unfs.Source{{Rel: "proj"}, {Rel: "proj/conf", ReadOnly: true}}
	if !reflect.DeepEqual(p.Sources, wantSrc) {
		t.Errorf("sources %+v", p.Sources)
	}
	wantShares := []Share{{Dest: "/app", Export: "/s/0"}, {Dest: "/app/conf", Export: "/s/1", ReadOnly: true}, {Dest: "/again", Export: "/s/0"}}
	if !reflect.DeepEqual(p.Shares, wantShares) {
		t.Errorf("shares %+v", p.Shares)
	}
	got := strings.Join(p.LaunchArgs(), " ")
	if got != "--share-net --unfs-mount /Users/me --unfs-src proj:rw --unfs-src proj/conf:ro" {
		t.Errorf("launch args %s", got)
	}
}

func TestMakePlanRefusals(t *testing.T) {
	cases := map[string][]specs.Mount{
		"root dest": {{Destination: "/", Type: "bind", Source: "/Users/me/a"}},
	}
	for name, m := range cases {
		if _, err := MakePlan(m, []string{"/Users/me", "/Volumes/other"}, fake); err == nil {
			t.Errorf("%s: accepted", name)
		}
	}
	p, err := MakePlan([]specs.Mount{{Destination: "/x", Type: "bind", Source: "/tmp/x"}}, []string{"/Users/me"}, fake)
	if err != nil || len(p.Sources) != 0 || p.LaunchArgs() != nil {
		t.Errorf("a bind outside every share must leave no plan: %+v %v", p, err)
	}
	p, err = MakePlan([]specs.Mount{{Destination: "/w", Type: "bind", Source: "/Users/me"}}, []string{"/Users/me"}, fake)
	if err != nil || len(p.Sources) != 1 || p.Sources[0].Rel != "." {
		t.Errorf("the share's own root: %+v %v", p, err)
	}
}

// Boundaries the plan must keep: a sibling whose name only starts with a share's, a share mounted inside another
// (in either mount-table order), a link that resolves out of the share, and one directory bound both ways.
func TestMakePlanBoundaries(t *testing.T) {
	r := Resolver{
		Resolve: func(p string) (string, error) {
			if p == "/Users/me/out-link" {
				return "/etc", nil
			}
			return p, nil
		},
		IsDir: func(string) (bool, error) { return true, nil },
	}
	p, err := MakePlan([]specs.Mount{{Destination: "/x", Type: "bind", Source: "/Users/mex/a"}}, []string{"/Users/me"}, r)
	if err != nil || len(p.Sources) != 0 {
		t.Errorf("a sibling with a longer name is not in the share: %+v %v", p, err)
	}
	p, err = MakePlan([]specs.Mount{{Destination: "/x", Type: "bind", Source: "/Users/me/out-link"}}, []string{"/Users/me"}, r)
	if err != nil || len(p.Sources) != 0 {
		t.Errorf("a link out of the share is not a share: %+v %v", p, err)
	}
	for _, order := range [][]string{{"/Users/me", "/Users/me/inner"}, {"/Users/me/inner", "/Users/me"}} {
		p, err = MakePlan([]specs.Mount{{Destination: "/x", Type: "bind", Source: "/Users/me/inner/a"}}, order, r)
		if err != nil || !reflect.DeepEqual(p.Mounts, []string{"/Users/me/inner"}) || p.Sources[0].Rel != "a" {
			t.Errorf("nested shares %v: %+v %v", order, p, err)
		}
	}
	p, err = MakePlan([]specs.Mount{
		{Destination: "/rw", Type: "bind", Source: "/Users/me/d", Options: []string{"rw"}},
		{Destination: "/ro", Type: "bind", Source: "/Users/me/d", Options: []string{"ro"}},
	}, []string{"/Users/me"}, r)
	want := []Share{{Dest: "/rw", Export: "/s/0"}, {Dest: "/ro", Export: "/s/0", ReadOnly: true}}
	if err != nil || !reflect.DeepEqual(p.Sources, []unfs.Source{{Rel: "d"}}) || !reflect.DeepEqual(p.Shares, want) {
		t.Errorf("one directory bound both ways must be one read-write export, read-only in the guest at /ro: %+v %v", p, err)
	}
	p, err = MakePlan([]specs.Mount{
		{Destination: "/ro", Type: "bind", Source: "/Users/me/d", Options: []string{"ro"}},
		{Destination: "/rw", Type: "bind", Source: "/Users/me/d", Options: []string{"rw"}},
	}, []string{"/Users/me"}, r)
	if err != nil || !reflect.DeepEqual(p.Sources, []unfs.Source{{Rel: "d"}}) || !p.Shares[0].ReadOnly || p.Shares[1].ReadOnly {
		t.Errorf("read-only first, then read-write: %+v %v", p, err)
	}
	if err := CheckDest("/a\nb"); err == nil {
		t.Error("a newline in a destination was accepted")
	}
	if err := CheckDest("/my data"); err != nil {
		t.Errorf("a space in a destination is valid: %v", err)
	}
}

// GH #207: binds from two shares are one server's sources, keyed by mount, grouped by mount with the guest's exports
// renumbered to match; the same relative path in two shares is two sources (council SEC-4); a source that holds
// another share is refused (JDG-1, ARC-9).
func TestMakePlanTwoShares(t *testing.T) {
	shares := []string{"/Users/me", "/Volumes/other", "/Users/me/inner"}
	p, err := MakePlan([]specs.Mount{
		{Destination: "/a", Type: "bind", Source: "/Users/me/x", Options: []string{"rw"}},
		{Destination: "/b", Type: "bind", Source: "/Volumes/other/x", Options: []string{"rw"}},
		{Destination: "/c", Type: "bind", Source: "/Users/me/y", Options: []string{"ro"}},
		{Destination: "/d", Type: "bind", Source: "/Volumes/other/x/z", Options: []string{"ro"}},
	}, shares, fake)
	if err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(p.Mounts, []string{"/Users/me", "/Volumes/other"}) {
		t.Errorf("mounts %q", p.Mounts)
	}
	wantSrc := []unfs.Source{{Rel: "x"}, {Rel: "y", ReadOnly: true}, {Mount: 1, Rel: "x"}, {Mount: 1, Rel: "x/z", ReadOnly: true}}
	if !reflect.DeepEqual(p.Sources, wantSrc) {
		t.Errorf("sources %+v", p.Sources)
	}
	wantShares := []Share{{Dest: "/a", Export: "/s/0"}, {Dest: "/b", Export: "/s/2"}, {Dest: "/c", Export: "/s/1", ReadOnly: true}, {Dest: "/d", Export: "/s/3", ReadOnly: true}}
	if !reflect.DeepEqual(p.Shares, wantShares) {
		t.Errorf("shares %+v", p.Shares)
	}
	got := strings.Join(p.LaunchArgs(), " ")
	if got != "--share-net --unfs-mount /Users/me --unfs-src x:rw --unfs-src y:ro --unfs-mount /Volumes/other --unfs-src x:rw --unfs-src x/z:ro" {
		t.Errorf("launch args %s", got)
	}
	if err := (unfs.Args{Mounts: p.Mounts, Sources: p.Sources}).Check(); err != nil {
		t.Errorf("the plan is not a valid server record: %v", err)
	}
	if n := unfs.Nests(p.Sources); !reflect.DeepEqual(n, []unfs.Nest{{Outer: 2, Inner: 3, Sub: "z"}}) {
		t.Errorf("nests %+v", n)
	}
	for _, src := range []string{"/Users/me"} {
		if _, err := MakePlan([]specs.Mount{{Destination: "/w", Type: "bind", Source: src}}, shares, fake); err == nil || !strings.Contains(err.Error(), "holds another share") {
			t.Errorf("a source holding another share: %v", err)
		}
	}
	if p, err := MakePlan([]specs.Mount{{Destination: "/w", Type: "bind", Source: "/Users/me/x"}}, shares, fake); err != nil || len(p.Sources) != 1 {
		t.Errorf("a source beside the inner share: %+v %v", p, err)
	}
}

// GH #208: a file bind is a file source and a file share; a read-only file inside a read-write directory is nested.
func TestMakePlanFiles(t *testing.T) {
	p, err := MakePlan([]specs.Mount{
		{Destination: "/app", Type: "bind", Source: "/Users/me/proj", Options: []string{"rw"}},
		{Destination: "/app/conf.txt", Type: "bind", Source: "/Users/me/proj/conf.txt", Options: []string{"ro"}},
		{Destination: "/etc/app.txt", Type: "bind", Source: "/Users/me/app.txt", Options: []string{"rw"}},
	}, []string{"/Users/me"}, fake)
	if err != nil {
		t.Fatal(err)
	}
	wantSrc := []unfs.Source{{Rel: "proj"}, {Rel: "proj/conf.txt", ReadOnly: true, File: true}, {Rel: "app.txt", File: true}}
	if !reflect.DeepEqual(p.Sources, wantSrc) {
		t.Errorf("sources %+v", p.Sources)
	}
	wantShares := []Share{{Dest: "/app", Export: "/s/0"}, {Dest: "/app/conf.txt", Export: "/s/1", ReadOnly: true, File: true}, {Dest: "/etc/app.txt", Export: "/s/2", File: true}}
	if !reflect.DeepEqual(p.Shares, wantShares) {
		t.Errorf("shares %+v", p.Shares)
	}
	if got := strings.Join(p.LaunchArgs(), " "); got != "--share-net --unfs-mount /Users/me --unfs-src proj:rw --unfs-src proj/conf.txt:rof --unfs-src app.txt:rwf" {
		t.Errorf("launch args %s", got)
	}
	if n := unfs.Nests(p.Sources); !reflect.DeepEqual(n, []unfs.Nest{{Outer: 0, Inner: 1, Sub: "conf.txt"}}) {
		t.Errorf("nests %+v", n)
	}
	v, err := Encode(p.Shares)
	if got, err2 := FromAnnotations(map[string]string{Annotation: v}); err != nil || err2 != nil || !reflect.DeepEqual(got, p.Shares) {
		t.Errorf("annotation round trip %+v %v %v", got, err, err2)
	}
	// Events: a file source's own path gives its one entry; nothing below it.
	srv := Server{Mounts: p.Mounts, Sources: p.Sources}
	if got := srv.Map("/Users/me/app.txt"); !reflect.DeepEqual(got, []Event{{Export: 2, Rest: unfs.FileName}}) {
		t.Errorf("file event %+v", got)
	}
	if got := srv.Map("/Users/me/app.txt/x"); got != nil {
		t.Errorf("below a file %+v", got)
	}
}

func TestAnnotationRoundTrip(t *testing.T) {
	list := []Share{{Dest: "/a/b", Export: "/s/1", ReadOnly: true}, {Dest: "/a", Export: "/s/0"}}
	v, err := Encode(list)
	if err != nil {
		t.Fatal(err)
	}
	got, err := FromAnnotations(map[string]string{Annotation: v})
	if err != nil || !reflect.DeepEqual(got, list) {
		t.Fatalf("round trip: %+v %v", got, err)
	}
	if s := Sorted(got); s[0].Dest != "/a" || s[1].Dest != "/a/b" {
		t.Errorf("sorted %+v", s)
	}
	for _, bad := range []string{`[{"dest":"a","export":"/s/0"}]`, `[{"dest":"/a/../b","export":"/s/0"}]`, `[{"dest":"/a","export":"/etc"}]`, `[{"dest":"/","export":"/s/0"}]`, `nope`} {
		if _, err := FromAnnotations(map[string]string{Annotation: bad}); err == nil {
			t.Errorf("accepted %s", bad)
		}
	}
	if l, err := FromAnnotations(nil); l != nil || err != nil {
		t.Error("no annotation must be an empty list")
	}
}

func TestShareMounts(t *testing.T) {
	text := "share0 /Users/me virtiofs rw 0 0\ntmpfs /run tmpfs rw 0 0\nshare1 /Users/me/My\\040Project virtiofs ro 0 0\n"
	got := ShareMounts(text)
	if !reflect.DeepEqual(got, []string{"/Users/me", "/Users/me/My Project"}) {
		t.Errorf("%q", got)
	}
}

func TestMountOptions(t *testing.T) {
	if MountOptions() != "addr=169.254.77.1,vers=3,proto=tcp,port=2049,mountport=2050,mountproto=tcp,nolock,hard,actimeo=1,lookupcache=positive" {
		t.Error(MountOptions())
	}
}

// Docker 25 and later write rro for a recursive read-only bind (--mount ...,readonly, -v ...:ro): it is read-only.
func TestRecursiveReadOnlyIsReadOnly(t *testing.T) {
	p, err := MakePlan([]specs.Mount{{Destination: "/data", Type: "bind", Source: "/Users/me/proj", Options: []string{"rbind", "rro"}}}, []string{"/Users/me"}, fake)
	if err != nil {
		t.Fatal(err)
	}
	if len(p.Shares) != 1 || !p.Shares[0].ReadOnly || !p.Sources[0].ReadOnly {
		t.Errorf("rro was not read-only: %+v %+v", p.Shares, p.Sources)
	}
}
