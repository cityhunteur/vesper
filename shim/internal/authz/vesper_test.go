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
		_ = os.WriteFile(filepath.Join(dir, "config.v2.json"), []byte(`{"ID":"`+r[0]+`","Name":"/`+r[1]+`","State":{"StartedAt":"2026-11-23T10:00:00Z"}}`), 0o600)
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
		"volume mount":   `{"HostConfig":{"Runtime":"vesper","Mounts":[{"Type":"volume","Source":"data","Target":"/t"}]}}`,
		"system mount":   `{"HostConfig":{"Runtime":"vesper","Mounts":[{"Type":"bind","Source":"/etc","Target":"/t"}]}}`,
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

// ADR 0011 slice 4: a bind from a shared macOS directory reaches a vesper container; any other source stays refused.
func TestVesperShareBindsAllowed(t *testing.T) {
	store(t)
	old, oldMissing := OnShare, MissingSourceOK
	OnShare = func(p string) bool { return p == "/Users/me/src" || strings.HasPrefix(p, "/Users/me/src/") }
	MissingSourceOK = func(p string) (bool, bool) { return OnShare(p), false }
	t.Cleanup(func() { OnShare, MissingSourceOK = old, oldMissing })
	for _, b := range []string{
		`{"Image":"alpine","HostConfig":{"Runtime":"vesper","Binds":["/Users/me/src:/src"]}}`,
		`{"Image":"alpine","HostConfig":{"Runtime":"vesper","Binds":["/Users/me/src/app:/app:ro"]}}`,
		`{"Image":"alpine","HostConfig":{"Runtime":"vesper","Mounts":[{"Type":"bind","Source":"/Users/me/src","Target":"/src"}]}}`,
	} {
		if d := decide("POST", create, b); !d.Allow {
			t.Errorf("%s refused: %s", b, d.Reason)
		}
	}
	for _, b := range []string{
		`{"Image":"alpine","HostConfig":{"Runtime":"vesper","Binds":["/Users/me/src:/src","/tmp:/x"]}}`,
		`{"Image":"alpine","HostConfig":{"Runtime":"vesper","Binds":["/Users/me/other:/x"]}}`,
	} {
		if d := decide("POST", create, b); d.Allow || !strings.Contains(d.Reason, "shared macOS directory") {
			t.Errorf("%s: %+v", b, d)
		}
	}
}

// ADR 0011 slice 4b: a microVM runs arm64 only, so another --platform is refused by name; arm64 passes.
func TestVesperPlatform(t *testing.T) {
	store(t)
	b := `{"Image":"alpine","HostConfig":{"Runtime":"vesper"}}`
	if d := decide("POST", create+"&platform=linux/amd64", b); d.Allow || !strings.Contains(d.Reason, "linux/arm64 only") {
		t.Errorf("amd64: %+v", d)
	}
	for _, p := range []string{"", "&platform=linux/arm64", "&platform=linux/arm64/v8"} {
		if d := decide("POST", create+p, b); !d.Allow {
			t.Errorf("%q refused: %s", p, d.Reason)
		}
	}
	if d := decide("POST", create+"&platform=linux/amd64", `{"Image":"alpine","HostConfig":{"Runtime":"runc"}}`); !d.Allow {
		t.Errorf("runc amd64 refused: %s", d.Reason)
	}
}

// ADR 0011 slice 5: cp, export, diff, commit and network connect/disconnect are refused for a vesper container, by
// name, id or unique id prefix, and left to dockerd for any other.
func TestVesperHonestyRefusals(t *testing.T) {
	vid, rid := strings.Repeat("a", 64), strings.Repeat("b", 64)
	store(t, [3]string{vid, "vm", "vesper"}, [3]string{rid, "rc", "runc"})
	for _, ref := range []string{"vm", vid, "aaaa"} {
		for _, req := range [][3]string{
			{"GET", "/v1.51/containers/" + ref + "/archive?path=/etc", ""},
			{"PUT", "/v1.51/containers/" + ref + "/archive?path=/tmp", ""},
			{"HEAD", "/v1.51/containers/" + ref + "/archive?path=/etc", ""},
			{"GET", "/v1.51/containers/" + ref + "/export", ""},
			{"GET", "/v1.51/containers/" + ref + "/changes", ""},
			{"POST", "/v1.51/commit?container=" + ref + "&repo=x", ""},
			{"POST", "/v1.51/networks/n1/connect", `{"Container":"` + ref + `"}`},
			{"POST", "/v1.51/networks/n1/disconnect", `{"container":"` + ref + `","Force":true}`},
		} {
			d := decide(req[0], req[1], req[2])
			if d.Allow || !strings.Contains(d.Reason, "vesper container") || !strings.HasSuffix(d.Reason, "(ADR 0011)") {
				t.Errorf("%s %s %s: %+v", req[0], req[1], req[2], d)
			}
		}
	}
	for _, req := range [][3]string{
		{"GET", "/v1.51/containers/rc/archive?path=/etc", ""},
		{"GET", "/containers/" + rid + "/export", ""},
		{"GET", "/v1.51/containers/rc/changes", ""},
		{"POST", "/v1.51/commit?container=rc", ""},
		{"POST", "/v1.51/networks/n1/connect", `{"Container":"rc"}`},
		{"GET", "/v1.51/containers/nosuch/changes", ""},
	} {
		if d := decide(req[0], req[1], req[2]); !d.Allow {
			t.Errorf("%s %s: refused: %s", req[0], req[1], d.Reason)
		}
	}
}

// Review of D-090: an empty connect body is refused (dockerd decodes a body the plugin never saw); duplicate keys
// resolve as dockerd's struct decoding does; a half-made container directory does not make the store unreadable; a
// vesper container that never started keeps docker cp (its drive is built from dockerd's tree at the first start).
func TestVesperHonestyEdges(t *testing.T) {
	vid, rid := strings.Repeat("a", 64), strings.Repeat("b", 64)
	store(t, [3]string{vid, "vm", "vesper"}, [3]string{rid, "rc", "runc"})
	root := StoreRoots()[0]
	if d := decide("POST", "/v1.51/networks/n1/connect", ""); d.Allow {
		t.Error("an empty connect body was allowed")
	}
	if d := decide("POST", "/v1.51/networks/n1/connect", `{"Container":"rc","container":"vm"}`); d.Allow {
		t.Error("a duplicate key naming the vesper container last was allowed")
	}
	if d := decide("POST", "/v1.51/networks/n1/connect", `{"container":"vm","Container":"rc"}`); !d.Allow {
		t.Errorf("a duplicate key naming runc last was refused: %s", d.Reason)
	}
	if err := os.MkdirAll(filepath.Join(root, strings.Repeat("c", 64)), 0o700); err != nil {
		t.Fatal(err)
	}
	if d := decide("GET", "/v1.51/containers/rc/changes", ""); !d.Allow {
		t.Errorf("a half-made container directory refused a runc request: %s", d.Reason)
	}
	nid := strings.Repeat("d", 64)
	dir := filepath.Join(root, nid)
	_ = os.MkdirAll(dir, 0o700)
	_ = os.WriteFile(filepath.Join(dir, "config.v2.json"), []byte(`{"ID":"`+nid+`","Name":"/fresh","State":{"StartedAt":"0001-01-01T00:00:00Z"}}`), 0o600)
	_ = os.WriteFile(filepath.Join(dir, "hostconfig.json"), []byte(`{"Runtime":"vesper"}`), 0o600)
	if d := decide("PUT", "/v1.51/containers/fresh/archive?path=/tmp", ""); !d.Allow {
		t.Errorf("docker cp into a never-started vesper container refused: %s", d.Reason)
	}
	if d := decide("POST", "/v1.51/networks/n1/connect", `{"Container":"fresh"}`); d.Allow {
		t.Error("network connect of a never-started vesper container was allowed")
	}
}
