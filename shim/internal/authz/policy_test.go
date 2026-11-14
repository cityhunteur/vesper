package authz

import (
	"strings"
	"testing"
)

func decide(method, uri, body string) Decision {
	return Decide(Request{Method: method, URI: uri, Body: []byte(body)})
}

func TestRoutes(t *testing.T) {
	allowed := []struct{ method, uri string }{
		{"GET", "/_ping"}, {"HEAD", "/_ping"}, {"GET", "/v1.51/version"}, {"GET", "/v1.51/info"},
		{"GET", "/v1.51/containers/json?all=1"}, {"POST", "/v1.51/containers/abc/start"},
		{"POST", "/v1.51/containers/abc/exec"}, {"POST", "/v1.51/exec/abc/start"},
		{"DELETE", "/v1.51/containers/abc?force=1"}, {"GET", "/v1.51/containers/abc/attach/ws"},
		{"PUT", "/v1.51/containers/abc/archive?path=/x"}, {"POST", "/v1.51/images/create?fromImage=alpine"},
		{"GET", "/v1.51/images/public.ecr.aws/firecracker/fcuvm:v90/json"},
		{"DELETE", "/v1.51/images/vesper/trivial:guest"}, {"POST", "/v1.51/images/a/b/tag?repo=c"},
		{"GET", "/v1.51/distribution/docker.io/library/alpine/json"},
		{"POST", "/v1.51/networks/create"}, {"POST", "/v1.51/networks/n/connect"}, {"DELETE", "/v1.51/networks/n"},
		{"GET", "/v1.51/volumes"}, {"DELETE", "/v1.51/volumes/v"}, {"POST", "/v1.51/build?t=x"},
		{"POST", "/v1.51/build?version=1&networkmode=bridge"}, {"GET", "/v1.51/events"}, {"POST", "/auth"},
		{"GET", "/v1/version"},
	}
	for _, c := range allowed {
		if d := decide(c.method, c.uri, ""); !d.Allow {
			t.Errorf("%s %s refused: %s", c.method, c.uri, d.Reason)
		}
	}
	refused := []struct{ method, uri string }{
		{"POST", "/v1.51/plugins/pull?remote=x"}, {"GET", "/v1.51/plugins"}, {"POST", "/plugins/x/enable"},
		{"POST", "/v1.51/swarm/init"}, {"POST", "/v1.51/services/create"}, {"GET", "/v1.51/nodes"},
		{"GET", "/v1.51/tasks"}, {"POST", "/v1.51/secrets/create"}, {"POST", "/v1.51/configs/create"},
		{"POST", "/v1.51/session"}, {"POST", "/v1.51/grpc"}, {"POST", "/session"}, {"POST", "/grpc"},
		{"POST", "/v1.51/containers/abc/checkpoints"},
		{"POST", "/v1.51/build?version=2"}, {"POST", "/v1.51/build?networkmode=host"},
		{"POST", "/v1.51/build?networkmode=HOST"}, {"POST", "/v1.51/build?networkmode=container:abc"},
		// Not canonical: the router and this table must never read a path two ways.
		{"POST", "//plugins/pull"}, {"POST", "/v1.51//containers/create"}, {"GET", "/v1.51/containers/../plugins"},
		{"GET", "/v1.51/./version"}, {"GET", "/v1.51/version/"}, {"GET", "/%2e%2e/version"},
		// Not a plain path, a wrong method, a wrong case, an unknown route, a bare version.
		{"GET", "http://x/version"}, {"GET", "version"}, {"GET", ""}, {"POST", "/v1.51/version"},
		{"GET", "/v1.51/Version"}, {"get", "/v1.51/version"}, {"GET", "/v1.51/nothing"}, {"GET", "/v1.51"},
		{"GET", "/v1.51/"}, {"GET", "/vx/version"}, {"DELETE", "/v1.51/containers/a/b"},
		{"CONNECT", "/v1.51/version"},
	}
	for _, c := range refused {
		if d := decide(c.method, c.uri, ""); d.Allow {
			t.Errorf("%s %s allowed", c.method, c.uri)
		} else if !strings.HasPrefix(d.Reason, "vesper: ") || !strings.HasSuffix(d.Reason, "(ADR 0010)") {
			t.Errorf("%s %s: reason %q does not name its source", c.method, c.uri, d.Reason)
		}
	}
}

func TestCreateBody(t *testing.T) {
	const uri = "/v1.51/containers/create?name=x"
	allowed := []string{
		`{"Image":"alpine"}`,
		`{"Image":"alpine","HostConfig":{}}`,
		`{"Image":"alpine","HostConfig":null}`,
		`{"Image":"alpine","HostConfig":{"UsernsMode":"","Runtime":"runc","Privileged":true,"PidMode":"host"}}`,
		`{"Image":"alpine","HostConfig":{"Binds":["/:/host"],"CapAdd":["ALL"],"VolumeDriver":"local"}}`,
		`{"HostConfig":{"Mounts":[{"Type":"volume","Source":"v","Target":"/v","VolumeOptions":{"DriverConfig":{"Name":"local"}}}]}}`,
		`{"HostConfig":{"Mounts":[{"Type":"bind","Source":"/","Target":"/host"}]}}`,
		// The last of several equal keys wins, here and in dockerd.
		`{"HostConfig":{"UsernsMode":"host","UsernsMode":""}}`,
	}
	for _, b := range allowed {
		if d := decide("POST", uri, b); !d.Allow {
			t.Errorf("create %s refused: %s", b, d.Reason)
		}
	}
	refused := map[string]string{
		"no body":                     ``,
		"host userns":                 `{"Image":"alpine","HostConfig":{"UsernsMode":"host"}}`,
		"any userns value":            `{"HostConfig":{"UsernsMode":"private"}}`,
		"lower-case keys":             `{"hostconfig":{"usernsmode":"host"}}`,
		"upper-case keys":             `{"HOSTCONFIG":{"USERNSMODE":"host"}}`,
		"folded letters":              "{\"HostConfig\":{\"UſernſMode\":\"host\"}}",
		"last key wins":               `{"HostConfig":{"UsernsMode":"","UsernsMode":"host"}}`,
		"second HostConfig object":    `{"HostConfig":{"UsernsMode":""},"hostconfig":{"UsernsMode":"host"}}`,
		"host userns kept by a merge": `{"HostConfig":{"UsernsMode":"host"},"hostconfig":{"Runtime":"runc"}}`,
		"another runtime":             `{"HostConfig":{"Runtime":"io.containerd.vesper.v2"}}`,
		"runtime by path":             `{"HostConfig":{"Runtime":"/usr/bin/runc"}}`,
		"volume driver":               `{"HostConfig":{"VolumeDriver":"nfs"}}`,
		"mount driver options":        `{"HostConfig":{"Mounts":[{"Type":"volume","Target":"/v","VolumeOptions":{"DriverConfig":{"Options":{"type":"ext4","device":"/dev/vdb"}}}}]}}`,
		"mount driver name":           `{"HostConfig":{"Mounts":[{"Type":"volume","Target":"/v","VolumeOptions":{"DriverConfig":{"Name":"x"}}}]}}`,
		"not JSON":                    `Image=alpine`,
		"content after the JSON":      `{"Image":"alpine"} {"HostConfig":{"UsernsMode":"host"}}`,
		"wrong type for HostConfig":   `{"HostConfig":"host"}`,
		"wrong type for UsernsMode":   `{"HostConfig":{"UsernsMode":["host"]}}`,
		"truncated":                   `{"HostConfig":{"UsernsMode":"host"`,
	}
	for name, b := range refused {
		if d := decide("POST", uri, b); d.Allow {
			t.Errorf("create with %s allowed: %s", name, b)
		}
	}
	// What dockerd does with a create padded to 1 MiB and more: the plugin sees no body.
	if d := decide("POST", uri, ""); d.Allow || !strings.Contains(d.Reason, "not visible") {
		t.Errorf("create without a visible body: %+v", d)
	}
}

func TestVolumeCreateBody(t *testing.T) {
	const uri = "/v1.51/volumes/create"
	for _, b := range []string{`{"Name":"v"}`, `{"Name":"v","Driver":"local"}`, `{"Name":"v","DriverOpts":{}}`, `{}`} {
		if d := decide("POST", uri, b); !d.Allow {
			t.Errorf("volume create %s refused: %s", b, d.Reason)
		}
	}
	for _, b := range []string{``, `x`, `{"Driver":"nfs"}`, `{"driveropts":{"type":"none","o":"bind","device":"/run"}}`,
		`{"DriverOpts":{"type":"tmpfs"}}`, `{"Name":"v"}{}`} {
		if d := decide("POST", uri, b); d.Allow {
			t.Errorf("volume create %q allowed", b)
		}
	}
}
