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
	if p.Mount != "/Users/me" {
		t.Errorf("mount %q", p.Mount)
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
		"file":       {{Destination: "/f", Type: "bind", Source: "/Users/me/a.txt"}},
		"two shares": {{Destination: "/a", Type: "bind", Source: "/Users/me/a"}, {Destination: "/b", Type: "bind", Source: "/Volumes/other/b"}},
		"root dest":  {{Destination: "/", Type: "bind", Source: "/Users/me/a"}},
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
