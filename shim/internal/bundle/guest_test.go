package bundle

import (
	"encoding/json"
	"reflect"
	"testing"

	specs "github.com/opencontainers/runtime-spec/specs-go"
)

func TestGuestConfig(t *testing.T) {
	p := &specs.Process{Args: []string{"/app", "a b", "'$(touch /bad)'"}, Env: []string{"X=a\nb", "PATH=/bin"}, Cwd: "/work", ApparmorProfile: "host-profile", SelinuxLabel: "host-label", NoNewPrivileges: true}
	original, _ := json.Marshal(p)
	raw, err := GuestConfig(p)
	if err != nil {
		t.Fatal(err)
	}
	var got specs.Spec
	if err := json.Unmarshal(raw, &got); err != nil {
		t.Fatal(err)
	}
	if got.Root.Path != "/newroot" || len(got.Linux.Namespaces) != 1 || got.Linux.Namespaces[0].Type != specs.NetworkNamespace || got.Linux.Namespaces[0].Path != "/proc/1/ns/net" || got.Linux.Seccomp != nil || len(got.Mounts) != 0 || got.Hooks != nil {
		t.Fatalf("unexpected isolation config: %s", raw)
	}
	if !reflect.DeepEqual(got.Process.Args, p.Args) || !reflect.DeepEqual(got.Process.Env, p.Env) || got.Process.Cwd != p.Cwd {
		t.Fatalf("process changed: %s", raw)
	}
	if got.Process.ApparmorProfile != "" || got.Process.SelinuxLabel != "" || got.Process.NoNewPrivileges || len(got.Process.Capabilities.Bounding) != 41 {
		t.Fatalf("guest security config: %+v", got.Process)
	}
	after, _ := json.Marshal(p)
	if string(original) != string(after) {
		t.Fatal("changed caller's process")
	}
}

func TestGuestProcessValidation(t *testing.T) {
	for _, p := range []*specs.Process{nil, {}, {Args: []string{"a\x00b"}}, {Args: []string{"true"}, Cwd: "relative"}, {Args: []string{"true"}, Env: []string{"BAD"}}} {
		if _, err := GuestProcess(p); err == nil {
			t.Errorf("accepted invalid process: %+v", p)
		}
	}
}
