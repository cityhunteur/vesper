package bundle

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/containerd/containerd/v2/core/mount"
)

func write(t *testing.T, dir, name, body string) {
	t.Helper()
	if err := os.WriteFile(filepath.Join(dir, name), []byte(body), 0o644); err != nil {
		t.Fatal(err)
	}
}

const good = `{"ociVersion":"1.2.0","process":{"args":["sh","-c","true"],"cwd":"/"},"root":{"path":"rootfs"}}`

func overlay(lower ...string) []mount.Mount {
	return []mount.Mount{{Type: "overlay", Source: "overlay", Options: []string{"index=off", "workdir=/w", "upperdir=/u", "lowerdir=" + strings.Join(lower, ":")}}}
}

func TestValidate(t *testing.T) {
	dir := t.TempDir()
	cases := []struct {
		name, body, want string
		mounts           []mount.Mount
	}{
		{"missing", "", "config.json unreadable", overlay("/a")},
		{"badjson", "{", "not valid JSON", overlay("/a")},
		{"noversion", `{"process":{"args":["sh"]}}`, "lacks ociVersion", overlay("/a")},
		{"noargs", `{"ociVersion":"1.2.0","process":{"args":[]}}`, "process.args is empty", overlay("/a")},
		{"relcwd", `{"ociVersion":"1.2.0","process":{"args":["sh"],"cwd":"work"}}`, "cwd is not absolute", overlay("/a")},
		{"nomounts", good, "rootfs is empty", nil},
		{"badmount", good, "lacks type or source", []mount.Mount{{Type: "bind"}}},
	}
	for _, c := range cases {
		b := filepath.Join(dir, c.name)
		os.MkdirAll(filepath.Join(b, "rootfs"), 0o755)
		if c.body != "" {
			write(t, b, "config.json", c.body)
		}
		_, err := Validate(b, c.mounts)
		if err == nil || !strings.Contains(err.Error(), c.want) {
			t.Errorf("%s: want %q, got %v", c.name, c.want, err)
		}
		if _, ok := err.(*ValidationError); !ok {
			t.Errorf("%s: not a ValidationError", c.name)
		}
	}
	ok := filepath.Join(dir, "ok")
	os.MkdirAll(ok, 0o755)
	write(t, ok, "config.json", good)
	spec, err := Validate(ok, overlay("/a", "/b"))
	if err != nil || spec.Process.Args[0] != "sh" {
		t.Fatalf("valid bundle rejected: %v", err)
	}
	if _, err := Validate("relative/bundle", overlay("/a")); err == nil || !strings.Contains(err.Error(), "not absolute") {
		t.Fatalf("relative bundle must fail: %v", err)
	}
}

func TestInspectAndKey(t *testing.T) {
	a := Inspect(overlay("/snap/3/fs", "/snap/1/fs"))
	b := Inspect(overlay("/snap/3/fs", "/snap/1/fs"))
	c := Inspect(overlay("/snap/1/fs"))
	if a.Kind != "overlay" || len(a.Lower) != 2 || a.Upper != "/u" {
		t.Fatalf("overlay not parsed: %+v", a)
	}
	if a.Key == "" || a.Key != b.Key {
		t.Fatal("same lower chain must give the same key")
	}
	if a.Key == c.Key {
		t.Fatal("different chains must differ")
	}
	d := Inspect([]mount.Mount{{Type: "bind", Source: "/snap/9/fs", Options: []string{"rw", "rbind"}}})
	if d.Kind != "mounts" || d.Key == "" {
		t.Fatalf("mounts fallback: %+v", d)
	}
	if e := Inspect(nil); e.Kind != "directory" || e.Key != "" {
		t.Fatalf("directory fallback: %+v", e)
	}
}

func TestMetaRoundTrip(t *testing.T) {
	dir := t.TempDir()
	if err := WriteMeta(dir, Meta{Task: "t", Image: "/x.ext4", Cached: true, Sha256: "ab", Layers: Layers{Kind: "overlay", Key: "k"}}); err != nil {
		t.Fatal(err)
	}
	m, err := ReadMeta(dir)
	if err != nil || !m.Cached || m.Layers.Key != "k" || m.Bundle != dir || m.BuiltAt == "" {
		t.Fatalf("round trip: %+v %v", m, err)
	}
	if _, err := ReadMeta(t.TempDir()); !os.IsNotExist(err) {
		t.Fatalf("missing meta must be not-exist: %v", err)
	}
}
