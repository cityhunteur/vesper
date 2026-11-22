package authz

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// store makes a dockerd container store with the given (id, name, runtime) rows.
func store(t *testing.T, rows ...[3]string) {
	t.Helper()
	root := filepath.Join(t.TempDir(), "containers")
	for _, r := range rows {
		dir := filepath.Join(root, r[0])
		if err := os.MkdirAll(dir, 0o700); err != nil {
			t.Fatal(err)
		}
		_ = os.WriteFile(filepath.Join(dir, "config.v2.json"), []byte(`{"ID":"`+r[0]+`","Name":"/`+r[1]+`"}`), 0o600)
		_ = os.WriteFile(filepath.Join(dir, "hostconfig.json"), []byte(`{"Runtime":"`+r[2]+`"}`), 0o600)
	}
	old := StoreRoots
	StoreRoots = func() []string { return []string{root} }
	t.Cleanup(func() { StoreRoots = old })
}

const create = "/v1.51/containers/create?name=x"

func TestVesperRuntimeAllowedAlone(t *testing.T) {
	store(t)
	for _, b := range []string{
		`{"Image":"alpine","HostConfig":{"Runtime":"vesper"}}`,
		`{"Image":"alpine","HostConfig":{"Runtime":"vesper","NetworkMode":"bridge","PortBindings":{"80/tcp":[{"HostIp":"127.0.0.1","HostPort":"8080"}]}}}`,
		`{"Image":"alpine","HostConfig":{"Runtime":"vesper","NetworkMode":"none","PidMode":"private"}}`,
		`{"Image":"alpine","HostConfig":{"Runtime":"vesper"},"NetworkingConfig":{"EndpointsConfig":{"bridge":{}}}}`,
		`{"Image":"alpine","HostConfig":{"Runtime":"vesper","NetworkMode":"mynet"},"NetworkingConfig":{"EndpointsConfig":{"mynet":{}}}}`,
	} {
		if d := decide("POST", create, b); !d.Allow {
			t.Errorf("%s refused: %s", b, d.Reason)
		}
	}
}

func TestVesperRefusals(t *testing.T) {
	store(t)
	for name, b := range map[string]string{
		"privileged":     `{"HostConfig":{"Runtime":"vesper","Privileged":true}}`,
		"device":         `{"HostConfig":{"Runtime":"vesper","Devices":[{"PathOnHost":"/dev/kvm"}]}}`,
		"gpus":           `{"HostConfig":{"Runtime":"vesper","DeviceRequests":[{"Count":-1}]}}`,
		"read-only":      `{"HostConfig":{"Runtime":"vesper","ReadonlyRootfs":true}}`,
		"volumes-from":   `{"HostConfig":{"Runtime":"vesper","VolumesFrom":["a"]}}`,
		"bind":           `{"HostConfig":{"Runtime":"vesper","Binds":["/tmp:/x"]}}`,
		"named volume":   `{"HostConfig":{"Runtime":"vesper","Binds":["data:/x"]}}`,
		"mount":          `{"HostConfig":{"Runtime":"vesper","Mounts":[{"Type":"tmpfs","Target":"/t"}]}}`,
		"host network":   `{"HostConfig":{"Runtime":"vesper","NetworkMode":"host"}}`,
		"container net":  `{"HostConfig":{"Runtime":"vesper","NetworkMode":"container:abc"}}`,
		"host pid":       `{"HostConfig":{"Runtime":"vesper","PidMode":"host"}}`,
		"host ipc":       `{"HostConfig":{"Runtime":"vesper","IpcMode":"host"}}`,
		"host uts":       `{"HostConfig":{"Runtime":"vesper","UTSMode":"host"}}`,
		"two networks":   `{"HostConfig":{"Runtime":"vesper"},"NetworkingConfig":{"EndpointsConfig":{"a":{},"b":{}}}}`,
		"host endpoint":  `{"HostConfig":{"Runtime":"vesper"},"NetworkingConfig":{"EndpointsConfig":{"host":{}}}}`,
		"lower-case key": `{"hostconfig":{"runtime":"vesper","privileged":true}}`,
	} {
		d := decide("POST", create, b)
		if d.Allow {
			t.Errorf("%s allowed", name)
		} else if !strings.HasSuffix(d.Reason, "(ADR 0011)") {
			t.Errorf("%s: reason %q does not name ADR 0011", name, d.Reason)
		}
	}
}

func TestJoinsToVesperContainers(t *testing.T) {
	store(t, [3]string{"aaaa1111", "web", "vesper"}, [3]string{"bbbb2222", "db", "runc"}, [3]string{"aaaa3333", "x", "runc"})
	refused := []string{
		`{"HostConfig":{"NetworkMode":"container:web"}}`,                   // by name
		`{"HostConfig":{"Runtime":"runc","PidMode":"container:aaaa1111"}}`, // by full id
		`{"HostConfig":{"IpcMode":"container:aaaa1"}}`,                     // by unique prefix
		`{"HostConfig":{"NetworkMode":"container:aaaa"}}`,                  // ambiguous prefix
		`{"HostConfig":{"NetworkMode":"container:nothing"}}`,               // no such container
	}
	for _, b := range refused {
		if d := decide("POST", create, b); d.Allow {
			t.Errorf("%s allowed", b)
		}
	}
	allowed := []string{
		`{"HostConfig":{"NetworkMode":"container:db"}}`,
		`{"HostConfig":{"PidMode":"container:bbbb"}}`,
		`{"HostConfig":{"NetworkMode":"bridge","PidMode":"host"}}`,
	}
	for _, b := range allowed {
		if d := decide("POST", create, b); !d.Allow {
			t.Errorf("%s refused: %s", b, d.Reason)
		}
	}
}

func TestJoinRefusedWhenARuntimeCannotBeRead(t *testing.T) {
	store(t, [3]string{"bbbb2222", "db", "runc"})
	root := StoreRoots()[0]
	if err := os.Remove(filepath.Join(root, "bbbb2222", "hostconfig.json")); err != nil {
		t.Fatal(err)
	}
	if d := decide("POST", create, `{"HostConfig":{"NetworkMode":"container:db"}}`); d.Allow {
		t.Error("a join was allowed to a container whose runtime is unknown")
	}
}

func TestJoinRefusedWhenTheStoreCannotBeRead(t *testing.T) {
	old := StoreRoots
	StoreRoots = func() []string { return []string{filepath.Join(t.TempDir(), "absent")} }
	defer func() { StoreRoots = old }()
	if d := decide("POST", create, `{"HostConfig":{"NetworkMode":"container:db"}}`); d.Allow {
		t.Error("a join was allowed with no store to read")
	}
}

func TestResolveOrder(t *testing.T) {
	all := []storedContainer{{id: "abc123", name: "abc"}, {id: "abc999", name: "other"}, {id: "fff000", name: "abc123x"}}
	if c, ok := resolveContainer("abc", all); !ok || c.id != "abc123" {
		t.Errorf("exact name beats prefix: got %v %v", c, ok)
	}
	if c, ok := resolveContainer("abc999", all); !ok || c.id != "abc999" {
		t.Errorf("full id: got %v %v", c, ok)
	}
	if _, ok := resolveContainer("abc1", all); !ok {
		t.Error("unique prefix not found")
	}
	if _, ok := resolveContainer("ab", all); ok {
		t.Error("ambiguous prefix resolved")
	}
}
