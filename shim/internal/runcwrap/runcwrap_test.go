package runcwrap

import (
	"reflect"
	"testing"

	specs "github.com/opencontainers/runtime-spec/specs-go"
)

func TestParse(t *testing.T) {
	cases := map[string]struct {
		args []string
		want Call
	}{
		// The line containerd's runc shim sends for dockerd (go-runc), global flags first.
		"containerd create": {
			[]string{"--root", "/run/docker/runtime-runc/moby", "--log", "/x/log.json", "--log-format", "json", "create",
				"--bundle", "/run/containerd/b/abc", "--pid-file", "/x/init.pid", "--console-socket", "/tmp/c.sock", "abc"},
			Call{Command: "create", Bundle: "/run/containerd/b/abc"},
		},
		"inline values and one dash":     {[]string{"-root=/r", "--systemd-cgroup", "create", "-b=/b", "id"}, Call{Command: "create", Bundle: "/b"}},
		"short bundle flag":              {[]string{"create", "-b", "/b", "id"}, Call{Command: "create", Bundle: "/b"}},
		"bundle after the id":            {[]string{"create", "id", "--bundle", "/b"}, Call{Command: "create", Bundle: "/b"}},
		"no bundle":                      {[]string{"create", "id"}, Call{Command: "create"}},
		"preserved descriptors":          {[]string{"create", "--preserve-fds", "2", "--bundle", "/b", "id"}, Call{Command: "create", Bundle: "/b", PreserveFDs: 2}},
		"bad preserved count":            {[]string{"create", "--preserve-fds", "x", "id"}, Call{Command: "create"}},
		"a value that looks like create": {[]string{"--root", "create", "start", "id"}, Call{Command: "start"}},
		"start":                          {[]string{"--root", "/r", "start", "id"}, Call{Command: "start"}},
		"exec is not create":             {[]string{"exec", "--process", "/p.json", "id"}, Call{Command: "exec"}},
		"features":                       {[]string{"features"}, Call{Command: "features"}},
		"version flag only":              {[]string{"--version"}, Call{}},
		"nothing":                        {nil, Call{}},
		"double dash":                    {[]string{"--debug", "--", "create"}, Call{}},
		"bundle flag without a value":    {[]string{"create", "--bundle"}, Call{Command: "create"}},
	}
	for name, c := range cases {
		if got := Parse(c.args); got != c.want {
			t.Errorf("%s: Parse(%q) = %+v, want %+v", name, c.args, got, c.want)
		}
	}
}

func remapped(uid, gid uint32, mounts ...specs.Mount) *specs.Spec {
	m := []specs.LinuxIDMapping{{ContainerID: 0, HostID: 100000, Size: 65536}}
	return &specs.Spec{
		Process: &specs.Process{User: specs.User{UID: uid, GID: gid}},
		Mounts:  mounts,
		Linux:   &specs.Linux{UIDMappings: m, GIDMappings: m},
	}
}

func TestTargetOf(t *testing.T) {
	share := specs.Mount{Destination: "/src", Type: "bind", Source: "/Users/me/repo", Options: []string{"rbind", "rprivate"}}
	optOnly := specs.Mount{Destination: "/data", Source: "/Users/me/data/", Options: []string{"bind"}}
	proc := specs.Mount{Destination: "/proc", Type: "proc", Source: "proc"}
	relative := specs.Mount{Destination: "/r", Type: "bind", Source: "relative/path"}

	got, ok := TargetOf(remapped(0, 0, proc, share, optOnly, share, relative))
	want := Target{Paths: []string{"/Users/me/repo", "/Users/me/data"}, UID: 100000, GID: 100000}
	if !ok || !reflect.DeepEqual(got, want) {
		t.Errorf("container root: got %+v %v, want %+v", got, ok, want)
	}
	if got, ok := TargetOf(remapped(1000, 50, share)); !ok || got.UID != 101000 || got.GID != 100050 {
		t.Errorf("user 1000: got %+v %v", got, ok)
	}
	if _, ok := TargetOf(remapped(70000, 0, share)); ok {
		t.Error("a uid outside the mapping must not be asked for")
	}
	if _, ok := TargetOf(remapped(0, 0, proc)); ok {
		t.Error("no bind mounts: nothing to ask")
	}
	unmapped := &specs.Spec{Process: &specs.Process{User: specs.User{UID: 0}}, Mounts: []specs.Mount{share}}
	if _, ok := TargetOf(unmapped); ok {
		t.Error("root in the System VM already holds the cached answer")
	}
	unmapped.Process.User = specs.User{UID: 1000, GID: 1000}
	if got, ok := TargetOf(unmapped); !ok || got.UID != 1000 || got.GID != 1000 {
		t.Errorf("unmapped user 1000: got %+v %v", got, ok)
	}
	if _, ok := TargetOf(&specs.Spec{}); ok {
		t.Error("no process")
	}
	if _, ok := TargetOf(nil); ok {
		t.Error("nil spec")
	}
}
