//go:build linux

package unfs

import (
	"errors"
	"os"
	"path/filepath"
	"testing"
	"time"

	"golang.org/x/sys/unix"
)

// A directory that is gone reads as ENOENT, which is what openLive waits out (TD-071); a live one reads.
func TestDirAliveRefusesADeletedDirectory(t *testing.T) {
	base := t.TempDir()
	d := filepath.Join(base, "w")
	if err := os.Mkdir(d, 0o755); err != nil {
		t.Fatal(err)
	}
	fd, err := unix.Open(d, unix.O_PATH|unix.O_DIRECTORY|unix.O_CLOEXEC, 0)
	if err != nil {
		t.Fatal(err)
	}
	defer unix.Close(fd)
	if err := dirAlive(fd); err != nil {
		t.Fatalf("live directory: %v", err)
	}
	if err := refreshAttrs(fd); err != nil {
		t.Fatalf("refresh of a live directory: %v", err)
	}
	if err := os.Remove(d); err != nil {
		t.Fatal(err)
	}
	if err := dirAlive(fd); !errors.Is(err, unix.ENOENT) {
		t.Fatalf("deleted directory: %v, want ENOENT", err)
	}
	mfd, err := unix.Open(base, unix.O_PATH|unix.O_DIRECTORY|unix.O_CLOEXEC, 0)
	if err != nil {
		t.Fatal(err)
	}
	defer unix.Close(mfd)
	if err := os.Mkdir(d, 0o755); err != nil {
		t.Fatal(err)
	}
	got, err := openLive(mfd, "w", time.Now().Add(time.Second))
	if err != nil {
		t.Fatalf("openLive of a remade directory: %v", err)
	}
	unix.Close(got)
}
