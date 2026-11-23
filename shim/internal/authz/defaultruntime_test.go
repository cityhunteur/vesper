package authz

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// daemonDefault points DaemonConfig at a file with this content for one test ("" leaves no file).
func daemonDefault(t *testing.T, content string) {
	t.Helper()
	saved := DaemonConfig
	t.Cleanup(func() { DaemonConfig = saved })
	DaemonConfig = filepath.Join(t.TempDir(), "daemon.json")
	if content != "" {
		if err := os.WriteFile(DaemonConfig, []byte(content), 0o600); err != nil {
			t.Fatal(err)
		}
	}
}

func TestDefaultRuntime(t *testing.T) {
	for _, c := range []struct{ config, want string }{
		{`{"default-runtime":"vesper"}`, "vesper"},
		{`{"default-runtime":"vesper-runc","runtimes":{}}`, "vesper-runc"},
		{`{}`, "runc"},                     // dockerd's own default
		{`{"default-runtime":""}`, "runc"}, // as dockerd reads an empty one
	} {
		daemonDefault(t, c.config)
		if got, err := DefaultRuntime(); err != nil || got != c.want {
			t.Errorf("%s: got %q, %v; want %q", c.config, got, err, c.want)
		}
	}
	for _, bad := range []string{"", "{"} {
		daemonDefault(t, bad)
		if got, err := DefaultRuntime(); err == nil {
			t.Errorf("config %q: got %q and no error", bad, got)
		}
	}
}

// With vesper the daemon's default, a create that names no runtime is checked as a vesper create (D-099); with
// vesper-runc the default it is not; an unreadable configuration, or a default outside the known runtimes, refuses
// every create that names no runtime.
func TestCreateWithoutRuntimeGetsTheDefault(t *testing.T) {
	const create = "/v1.47/containers/create"
	// Each refusal is matched by its own words, without --platform, so no other check can stand in for it.
	vesperOnly := map[string]string{
		`{"Image":"alpine","HostConfig":{"Privileged":true}}`:     "--privileged is refused for the vesper runtime",
		`{"Image":"alpine","HostConfig":{"NetworkMode":"host"}}`:  "--network=host",
		`{"Image":"alpine","HostConfig":{"ReadonlyRootfs":true}}`: "--read-only is refused for the vesper runtime",
		`{"Image":"alpine","HostConfig":{"Devices":[{}]}}`:        "--device and --gpus are refused for the vesper runtime",
	}
	daemonDefault(t, `{"default-runtime":"vesper"}`)
	for b, want := range vesperOnly {
		if d := decide("POST", create, b); d.Allow || !strings.Contains(d.Reason, want) {
			t.Errorf("vesper default: %s: want a refusal with %q, got %+v", b, want, d)
		}
	}
	for _, b := range []string{`{"Image":"alpine"}`, `{"Image":"alpine","HostConfig":null}`, `{"Image":"alpine","HostConfig":{}}`} {
		if d := decide("POST", create, b); !d.Allow {
			t.Errorf("vesper default: a plain create %s was refused: %q", b, d.Reason)
		}
		if d := decide("POST", create+"?platform=linux/amd64", b); d.Allow || !strings.Contains(d.Reason, "--runtime vesper-runc") {
			t.Errorf("vesper default: amd64 %s: want the refusal naming the fallback, got %+v", b, d)
		}
	}
	// An explicit runtime is not replaced by the default: a read-only rootfs is refused only for vesper.
	if d := decide("POST", create, `{"Image":"alpine","HostConfig":{"Runtime":"vesper-runc","ReadonlyRootfs":true}}`); !d.Allow {
		t.Errorf("an explicit vesper-runc was checked as vesper: %q", d.Reason)
	}
	daemonDefault(t, `{"default-runtime":"vesper-runc"}`)
	for b := range vesperOnly {
		if d := decide("POST", create, b); strings.Contains(d.Reason, "(ADR 0011)") {
			t.Errorf("vesper-runc default: %s was checked as vesper: %q", b, d.Reason)
		}
	}
	if d := decide("POST", create+"?platform=linux/amd64", `{"Image":"alpine"}`); !d.Allow {
		t.Errorf("vesper-runc default: amd64 refused: %q", d.Reason)
	}
	// Refused when the default cannot be known or is not a runtime the plugin knows; an explicit runtime still passes.
	for config, want := range map[string]string{
		"":                               "default runtime could not be read",
		"{":                              "default runtime could not be read",
		`{"default-runtime":"crun"}`:     "--runtime=crun is refused",
		`{"default-runtime":["vesper"]}`: "default runtime could not be read",
	} {
		daemonDefault(t, config)
		if d := decide("POST", create, `{"Image":"alpine"}`); d.Allow || !strings.Contains(d.Reason, want) {
			t.Errorf("daemon.json %q: want a refusal with %q, got %+v", config, want, d)
		}
		if d := decide("POST", create, `{"Image":"alpine","HostConfig":{"Runtime":"vesper-runc"}}`); !d.Allow {
			t.Errorf("daemon.json %q, explicit runtime: refused: %q", config, d.Reason)
		}
	}
}
