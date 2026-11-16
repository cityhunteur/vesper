//go:build linux

package shares

import (
	"errors"
	"os"
	"path/filepath"
	"testing"

	"golang.org/x/sys/unix"
)

// A destination that passes through a symbolic link in the image resolves inside the root, and the directories it
// makes are made there, never at the link's target outside. A link whose target is missing inside the root fails
// the mount rather than being followed anywhere.
func TestOpenInRootStaysInside(t *testing.T) {
	base := t.TempDir()
	root := filepath.Join(base, "root")
	outside := filepath.Join(base, "outside")
	for _, d := range []string{root, outside, filepath.Join(root, "etc"), filepath.Join(root, "inner")} {
		if err := os.MkdirAll(d, 0o755); err != nil {
			t.Fatal(err)
		}
	}
	links := map[string]string{
		"to-inner": "/inner",   // absolute: means root/inner
		"etc/up":   "../../..", // climbs, but stops at the root
		"dangling": outside,    // absolute host path: root/<base>/outside does not exist
	}
	for name, target := range links {
		if err := os.Symlink(target, filepath.Join(root, name)); err != nil {
			t.Fatal(err)
		}
	}
	rfd, err := unix.Open(root, unix.O_PATH|unix.O_DIRECTORY, 0)
	if err != nil {
		t.Fatal(err)
	}
	defer unix.Close(rfd)
	for _, dest := range []string{"/to-inner/data", "/etc/up/made"} {
		fd, err := openInRoot(rfd, dest)
		if errors.Is(err, unix.ENOSYS) {
			t.Skip("openat2 is not available")
		}
		if err != nil {
			t.Fatalf("%s: %v", dest, err)
		}
		unix.Close(fd)
	}
	if _, err := openInRoot(rfd, "/dangling/data"); err == nil {
		t.Fatal("a dangling link was followed")
	}
	if ents, _ := os.ReadDir(outside); len(ents) != 0 {
		t.Fatalf("a directory was made outside the root: %v", ents)
	}
	if _, err := os.Stat(filepath.Join(base, "made")); err == nil {
		t.Fatal("a directory was made above the root")
	}
	for _, p := range []string{"inner/data", "made"} {
		if _, err := os.Stat(filepath.Join(root, p)); err != nil {
			t.Fatalf("expected %s inside the root: %v", p, err)
		}
	}
}
