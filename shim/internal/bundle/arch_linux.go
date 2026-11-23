//go:build linux

package bundle

import (
	"fmt"
	"io"
	"os"
	"strings"

	specs "github.com/opencontainers/runtime-spec/specs-go"
	"golang.org/x/sys/unix"
)

// CheckArch refuses a process whose program in rootfs is built for another architecture than the microVM's
// (D-099 G3). It reads only the program's first bytes; a missing program, or one that is neither ELF nor a "#!"
// script, passes, and the guest reports it as runc does.
func CheckArch(rootfs string, p *specs.Process) error {
	root, err := unix.Open(rootfs, unix.O_PATH|unix.O_DIRECTORY|unix.O_CLOEXEC, 0)
	if err != nil {
		return fmt.Errorf("open the rootfs %s: %w", rootfs, err)
	}
	defer unix.Close(root)
	return checkArch(p, func(name string) ([]byte, bool) { return readHeaderIn(root, name) })
}

// readHeaderIn reads the first bytes of name, resolved inside root as if root were "/" (the image's own symbolic
// links, absolute ones included, stay inside it). Only an executable regular file is read, as exec would run one:
// a path is opened without reading first, so a device or a FIFO in the image is never opened for reading.
func readHeaderIn(root int, name string) ([]byte, bool) {
	fd, err := unix.Openat2(root, strings.TrimPrefix(name, "/"), &unix.OpenHow{
		Flags: unix.O_PATH | unix.O_CLOEXEC, Resolve: unix.RESOLVE_IN_ROOT | unix.RESOLVE_NO_MAGICLINKS,
	})
	if err != nil {
		return nil, false
	}
	defer unix.Close(fd)
	var st unix.Stat_t
	if err := unix.Fstat(fd, &st); err != nil || st.Mode&unix.S_IFMT != unix.S_IFREG || st.Mode&0o111 == 0 {
		return nil, false
	}
	f, err := os.OpenFile(fmt.Sprintf("/proc/self/fd/%d", fd), os.O_RDONLY|unix.O_NONBLOCK|unix.O_NOCTTY, 0)
	if err != nil {
		return nil, false
	}
	defer f.Close()
	b := make([]byte, headerLen)
	n, err := io.ReadFull(f, b)
	// A short or empty file is still the one exec finds: it passes, and the guest reports it (council review-02-f07).
	if err != nil && err != io.ErrUnexpectedEOF && err != io.EOF {
		return nil, false
	}
	return b[:n], true
}
