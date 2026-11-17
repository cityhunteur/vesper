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
	got, err := openLive(mfd, "w", false, time.Now().Add(time.Second))
	if err != nil {
		t.Fatalf("openLive of a remade directory: %v", err)
	}
	unix.Close(got)
}

// A file source opens only as a regular file, beneath the mount and with no symbolic link (GH #208, council SEC-7).
func TestOpenLiveFile(t *testing.T) {
	base := t.TempDir()
	if err := os.WriteFile(filepath.Join(base, "a.txt"), []byte("a"), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := unix.Mkfifo(filepath.Join(base, "fifo"), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink("a.txt", filepath.Join(base, "link")); err != nil {
		t.Fatal(err)
	}
	if err := os.Mkdir(filepath.Join(base, "dir"), 0o755); err != nil {
		t.Fatal(err)
	}
	mfd, err := unix.Open(base, unix.O_PATH|unix.O_DIRECTORY|unix.O_CLOEXEC, 0)
	if err != nil {
		t.Fatal(err)
	}
	defer unix.Close(mfd)
	fd, err := openLive(mfd, "a.txt", true, time.Now())
	if errors.Is(err, unix.ENOSYS) {
		t.Skip("openat2 is not available")
	}
	if err != nil {
		t.Fatalf("a regular file: %v", err)
	}
	unix.Close(fd)
	for _, bad := range []string{"fifo", "link", "dir"} {
		if fd, err := openLive(mfd, bad, true, time.Now()); err == nil {
			unix.Close(fd)
			t.Errorf("%s opened as a file source", bad)
		}
	}
	if fd, err := openLive(mfd, "a.txt", false, time.Now()); err == nil {
		unix.Close(fd)
		t.Error("a file opened as a directory source")
	}
}
