//go:build linux

package shares

import (
	"bufio"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"os"
	"path/filepath"
	"strings"
	"time"

	specs "github.com/opencontainers/runtime-spec/specs-go"
	"golang.org/x/sys/unix"

	"github.com/cityhunteur/vesper/shim/internal/unfs"
)

// mountedFile records, in the session directory, the destinations mounted, in order.
const mountedFile = "shares.mounted"

// ProbeTimeout bounds the wait for the server before the first hard mount, so a stalled server cannot hang session
// start until its deadline (ADR 0009, Guest).
var ProbeTimeout = 3 * time.Second

// readList reads the share list from the session's OCI config.
func readList(sessionDir string) ([]Share, error) {
	raw, err := os.ReadFile(filepath.Join(sessionDir, "config.json"))
	if err != nil {
		return nil, err
	}
	var spec specs.Spec
	if err := json.Unmarshal(raw, &spec); err != nil {
		return nil, fmt.Errorf("config: %w", err)
	}
	return FromAnnotations(spec.Annotations)
}

// openInRoot opens dest beneath root as if root were "/" (symbolic links in the image resolve inside it), making
// missing directories on the way. It returns an O_PATH descriptor of the directory.
func openInRoot(root int, dest string) (int, error) {
	rel := strings.TrimPrefix(dest, "/")
	how := &unix.OpenHow{Flags: unix.O_PATH | unix.O_DIRECTORY | unix.O_CLOEXEC, Resolve: unix.RESOLVE_IN_ROOT | unix.RESOLVE_NO_MAGICLINKS}
	if fd, err := unix.Openat2(root, rel, how); err == nil {
		return fd, nil
	} else if !errors.Is(err, unix.ENOENT) {
		return -1, err
	}
	parts := strings.Split(rel, "/")
	for i := range parts {
		prefix := strings.Join(parts[:i+1], "/")
		if fd, err := unix.Openat2(root, prefix, how); err == nil {
			unix.Close(fd)
			continue
		} else if !errors.Is(err, unix.ENOENT) {
			return -1, fmt.Errorf("%s: %w", "/"+prefix, err)
		}
		parent := root
		if i > 0 {
			p, err := unix.Openat2(root, strings.Join(parts[:i], "/"), how)
			if err != nil {
				return -1, err
			}
			parent = p
		}
		// mkdirat with an O_PATH parent: the name is one plain component, so no link is followed.
		err := unix.Mkdirat(parent, parts[i], 0o755)
		if parent != root {
			unix.Close(parent)
		}
		if err != nil && !errors.Is(err, unix.EEXIST) {
			return -1, fmt.Errorf("make %s: %w", "/"+prefix, err)
		}
	}
	return unix.Openat2(root, rel, how)
}

// probe waits up to ProbeTimeout for the server's NFS port.
func probe() error {
	deadline := time.Now().Add(ProbeTimeout)
	var last error
	for time.Now().Before(deadline) {
		c, err := net.DialTimeout("tcp", net.JoinHostPort(unfs.ServerAddr, fmt.Sprint(unfs.NFSPort)), time.Until(deadline))
		if err == nil {
			c.Close()
			return nil
		}
		last = err
		time.Sleep(100 * time.Millisecond)
	}
	return last
}

// MountAll mounts the session's shares under root (the task's root, /newroot), shallowest destination first, and
// records them. A session without shares does nothing. On a failure it unmounts what it mounted.
func MountAll(sessionDir, root string) error {
	list, err := readList(sessionDir)
	if err != nil || len(list) == 0 {
		return err
	}
	if err := probe(); err != nil {
		return fmt.Errorf("share server %s does not answer (share %s): %v", unfs.ServerAddr, list[0].Dest, err)
	}
	rfd, err := unix.Open(root, unix.O_PATH|unix.O_DIRECTORY|unix.O_CLOEXEC, 0)
	if err != nil {
		return err
	}
	defer unix.Close(rfd)
	var done []string
	for _, s := range Sorted(list) {
		if err := mountOne(rfd, s); err != nil {
			unmountList(rfd, done)
			return fmt.Errorf("share %s: %w", s.Dest, err)
		}
		done = append(done, s.Dest)
		if err := os.WriteFile(filepath.Join(sessionDir, mountedFile), []byte(strings.Join(done, "\n")+"\n"), 0o600); err != nil {
			unmountList(rfd, done)
			return err
		}
	}
	return nil
}

func mountOne(rfd int, s Share) error {
	fd, err := openInRoot(rfd, s.Dest)
	if err != nil {
		return err
	}
	defer unix.Close(fd)
	var flags uintptr
	if s.ReadOnly {
		flags |= unix.MS_RDONLY
	}
	// The kernel resolves the descriptor's magic link to the directory itself, not to a path that could change.
	target := fmt.Sprintf("/proc/self/fd/%d", fd)
	if err := unix.Mount(unfs.ServerAddr+":"+s.Export, target, "nfs", flags, MountOptions()); err != nil {
		return fmt.Errorf("mount %s: %w", s.Export, err)
	}
	return nil
}

// UnmountAll unmounts what MountAll recorded, deepest first, so the server receives the writes still buffered
// before the session reports EXIT (ADR 0009, Lifecycle). It returns the first error but tries every share.
func UnmountAll(sessionDir, root string) error {
	raw, err := os.ReadFile(filepath.Join(sessionDir, mountedFile))
	if errors.Is(err, os.ErrNotExist) {
		return nil
	}
	if err != nil {
		return err
	}
	rfd, err := unix.Open(root, unix.O_PATH|unix.O_DIRECTORY|unix.O_CLOEXEC, 0)
	if err != nil {
		return err
	}
	defer unix.Close(rfd)
	var dests []string
	for _, d := range strings.Split(string(raw), "\n") { // one per line; a destination may hold spaces
		if d != "" {
			dests = append(dests, d)
		}
	}
	err = unmountList(rfd, dests)
	if err == nil {
		os.Remove(filepath.Join(sessionDir, mountedFile))
	}
	return err
}

func unmountList(rfd int, dests []string) error {
	var first error
	for i := len(dests) - 1; i >= 0; i-- {
		fd, err := unix.Openat2(rfd, strings.TrimPrefix(dests[i], "/"), &unix.OpenHow{
			Flags: unix.O_PATH | unix.O_DIRECTORY | unix.O_CLOEXEC, Resolve: unix.RESOLVE_IN_ROOT | unix.RESOLVE_NO_MAGICLINKS})
		if err != nil {
			if first == nil {
				first = fmt.Errorf("share %s: %w", dests[i], err)
			}
			continue
		}
		target := fmt.Sprintf("/proc/self/fd/%d", fd)
		// Write back the client's dirty pages first, so a busy mount's lazy detach below loses nothing written
		// before it; the workload has exited, so nothing writes after.
		serr := unix.Syncfs(fd)
		err = unix.Unmount(target, 0)
		if errors.Is(err, unix.EBUSY) {
			err = unix.Unmount(target, unix.MNT_DETACH)
		}
		if err == nil && serr != nil {
			err = fmt.Errorf("sync before unmount: %w", serr)
		}
		unix.Close(fd)
		if err != nil && first == nil {
			first = fmt.Errorf("unmount share %s: %w", dests[i], err)
		}
	}
	return first
}

// Replay applies share events from the System VM (Fanout) on the session's NFS mounts, one "N<TAB>rest" line at a
// time: each path is opened (the NFS client fetches its attributes again on open) and its mtime is set to the value
// it already has, which gives IN_MODIFY to watchers in the guest. The value is the server's, so the change that
// reaches macOS leaves the file's mtime and size as they were, and the launcher's (mtime, size) filter drops it as
// an echo. It returns the counts it prints at the end.
func Replay(r io.Reader, sessionDir, root string) (touched, missing, rejected int) {
	sc := bufio.NewScanner(r)
	sc.Buffer(make([]byte, 64*1024), 64*1024)
	var dests map[string][]string
	load := func() {
		list, err := readList(sessionDir)
		if err != nil {
			return
		}
		dests = map[string][]string{}
		for _, s := range list {
			dests[s.Export] = append(dests[s.Export], s.Dest)
		}
	}
	rfd, err := unix.Open(root, unix.O_PATH|unix.O_DIRECTORY|unix.O_CLOEXEC, 0)
	if err != nil {
		return 0, 0, 0
	}
	defer unix.Close(rfd)
	for sc.Scan() {
		e, err := ParseEvent(sc.Text())
		if err != nil {
			rejected++
			continue
		}
		if dests == nil {
			load()
		}
		ds := dests[unfs.ExportPath(e.Export)]
		if len(ds) == 0 {
			load() // the session may have started since
			ds = dests[unfs.ExportPath(e.Export)]
		}
		if len(ds) == 0 {
			missing++
			continue
		}
		for _, d := range ds {
			if touchInRoot(rfd, strings.TrimPrefix(d, "/")+"/"+e.Rest) {
				touched++
			} else {
				missing++
			}
		}
	}
	return touched, missing, rejected
}

// touchInRoot sets rel's mtime to its own value, found beneath root with no link leaving it.
func touchInRoot(rfd int, rel string) bool {
	fd, err := unix.Openat2(rfd, rel, &unix.OpenHow{
		Flags:   unix.O_RDONLY | unix.O_NOFOLLOW | unix.O_NONBLOCK | unix.O_CLOEXEC,
		Resolve: unix.RESOLVE_IN_ROOT | unix.RESOLVE_NO_MAGICLINKS,
	})
	if err != nil {
		return false
	}
	defer unix.Close(fd)
	var st unix.Stat_t
	if unix.Fstat(fd, &st) != nil {
		return false
	}
	ts := []unix.Timespec{{Sec: 0, Nsec: unix.UTIME_OMIT}, st.Mtim}
	return unix.UtimesNanoAt(unix.AT_FDCWD, fmt.Sprintf("/proc/self/fd/%d", fd), ts, 0) == nil
}
