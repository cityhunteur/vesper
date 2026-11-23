//go:build linux

package bundle

import (
	"os"
	"path/filepath"
	"strings"
	"syscall"
	"testing"

	specs "github.com/opencontainers/runtime-spec/specs-go"
)

// TestCheckArchRootfs reads programs from a real directory: symbolic links resolve inside the rootfs, and only an
// executable regular file is read.
func TestCheckArchRootfs(t *testing.T) {
	if elfMachine == 0 {
		t.Skip("no microVM architecture for this GOARCH")
	}
	other := uint16(62)
	if elfMachine == 62 {
		other = 183
	}
	root := t.TempDir()
	outside := t.TempDir()
	write := func(dir, name string, b []byte, mode os.FileMode) {
		p := filepath.Join(dir, name)
		if err := os.MkdirAll(filepath.Dir(p), 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(p, b, mode); err != nil {
			t.Fatal(err)
		}
	}
	write(root, "bin/busybox", elfHeader(elfMachine, false), 0o755)
	write(root, "bin/foreign", elfHeader(other, false), 0o755)
	write(root, "bin/noexec", elfHeader(other, false), 0o644)
	write(outside, "foreign", elfHeader(other, false), 0o755)
	// An absolute link resolves inside the rootfs, as in the guest: /bin/true -> /bin/busybox in the image.
	if err := os.Symlink("/bin/busybox", filepath.Join(root, "bin/true")); err != nil {
		t.Fatal(err)
	}
	// A link that climbs out stays at the rootfs's "/": it cannot reach the System VM's own files.
	if err := os.Symlink("../../../../../../"+outside+"/foreign", filepath.Join(root, "bin/escape")); err != nil {
		t.Fatal(err)
	}
	// ".." after a link climbs from the link's target, as exec resolves it: /alias/../foreign is /bin/foreign, not
	// /foreign (a native file planted there to catch a textual clean).
	if err := os.Symlink("/bin/sub", filepath.Join(root, "alias")); err != nil {
		t.Fatal(err)
	}
	write(root, "bin/sub/.keep", nil, 0o644)
	write(root, "foreign", elfHeader(elfMachine, false), 0o755)
	write(root, "bin/empty", nil, 0o755)
	if err := syscall.Mkfifo(filepath.Join(root, "bin/fifo"), 0o755); err != nil {
		t.Fatal(err)
	}
	proc := func(arg string) *specs.Process {
		return &specs.Process{Args: []string{arg}, Env: []string{"PATH=/bin"}}
	}
	for _, c := range []struct {
		arg    string
		refuse bool
	}{
		{"true", false},
		{"busybox", false},
		{"foreign", true},
		{"noexec", false}, // not executable: exec would not run it either
		{"escape", false}, // resolves to /<outside>/foreign inside the rootfs, which is not there
		{"fifo", false},   // never opened for reading, so the check does not block
		{"missing", false},
		{"/alias/../foreign", true},
		{"empty", false},
	} {
		err := CheckArch(root, proc(c.arg))
		if c.refuse != (err != nil) {
			t.Errorf("%s: refuse=%v, got %v", c.arg, c.refuse, err)
		}
		if err != nil && !strings.Contains(err.Error(), "--runtime "+FallbackRuntime) {
			t.Errorf("%s: %v does not name the fallback", c.arg, err)
		}
	}
	if err := CheckArch(filepath.Join(root, "absent"), proc("true")); err == nil {
		t.Error("a missing rootfs passed")
	}
}
