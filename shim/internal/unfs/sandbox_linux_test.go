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

// The supervisor finds a nest that is gone in the sandbox's mountinfo (D-052): the fifth field is the mount point,
// with a space, a tab, a newline and a backslash written as octal escapes.
func TestParseMountPoints(t *testing.T) {
	got := parseMountPoints("" +
		"812 780 0:61 / / ro,nosuid,nodev,noexec - tmpfs vesper-unfs rw,size=1024k\n" +
		"815 812 0:42 /proj /s/0 rw,nosuid,nodev,noexec - virtiofs share0 rw\n" +
		"816 815 0:42 /proj/my\\040conf.txt /s/0/my\\040conf.txt ro,nosuid,nodev,noexec - virtiofs share0 rw\n" +
		"817 815 0:42 /proj/a\\134b /s/0/a\\134b ro - virtiofs share0 rw\n" +
		"short line\n")
	for _, p := range []string{"/", "/s/0", "/s/0/my conf.txt", `/s/0/a\b`} {
		if !got[p] {
			t.Errorf("mount point %q not found in %v", p, got)
		}
	}
	if len(got) != 4 {
		t.Errorf("%d mount points, want 4: %v", len(got), got)
	}
	if u := unescapeMount(`a\04`); u != `a\04` {
		t.Errorf("a short escape changed: %q", u)
	}
}
