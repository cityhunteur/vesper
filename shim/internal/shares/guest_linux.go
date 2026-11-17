//go:build linux

package shares

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"os"
	"os/exec"
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

// MountTimeout bounds each mount: a server that accepts the connection but does not answer (stopped, overloaded)
// would hold mount(2) for minutes. Each mount runs in a child (MountOne), killed when this runs out.
var MountTimeout = 5 * time.Second

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
		var err error
		if s.File {
			var hidden string
			hidden, err = mountFile(rfd, sessionDir, s)
			if hidden != "" {
				done = append(done, hiddenMark+hidden)
			}
		} else {
			err = mountOne(rfd, s)
		}
		if err != nil {
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

// hiddenMark starts a line of the mount record that names a guest path outside the task's root: the hidden mount of
// a file share's export.
const hiddenMark = "!"

// mountFile mounts a file share's export at a hidden directory of the session, outside the task's root, and binds its
// one file onto the destination, which it makes as an empty file when missing (GH #208). It returns the hidden
// directory once that is mounted, so a failure after it can be undone.
func mountFile(rfd int, sessionDir string, s Share) (string, error) {
	hidden := filepath.Join(sessionDir, "share-file"+strings.TrimPrefix(s.Export, "/s/"))
	if err := os.MkdirAll(hidden, 0o700); err != nil {
		return "", err
	}
	hfd, err := unix.Open(hidden, unix.O_PATH|unix.O_DIRECTORY|unix.O_NOFOLLOW|unix.O_CLOEXEC, 0)
	if err != nil {
		return "", err
	}
	hf := os.NewFile(uintptr(hfd), hidden)
	err = mountAt(hf, s)
	hf.Close()
	if err != nil {
		return "", err
	}
	src, err := unix.Open(filepath.Join(hidden, unfs.FileName), unix.O_PATH|unix.O_NOFOLLOW|unix.O_CLOEXEC, 0)
	if err != nil {
		return hidden, fmt.Errorf("the export holds no file: %w", err)
	}
	defer unix.Close(src)
	dst, err := openFileInRoot(rfd, s.Dest)
	if err != nil {
		return hidden, err
	}
	defer unix.Close(dst)
	t, err := unix.OpenTree(src, "", unix.OPEN_TREE_CLONE|unix.OPEN_TREE_CLOEXEC|unix.AT_EMPTY_PATH)
	if err != nil {
		return hidden, fmt.Errorf("open_tree: %w", err)
	}
	defer unix.Close(t)
	set := uint64(unix.MOUNT_ATTR_NOSUID | unix.MOUNT_ATTR_NODEV)
	if s.ReadOnly {
		set |= unix.MOUNT_ATTR_RDONLY
	}
	if err := unix.MountSetattr(t, "", unix.AT_EMPTY_PATH, &unix.MountAttr{Attr_set: set}); err != nil {
		return hidden, fmt.Errorf("mount_setattr: %w", err)
	}
	if err := unix.MoveMount(t, "", dst, "", unix.MOVE_MOUNT_F_EMPTY_PATH|unix.MOVE_MOUNT_T_EMPTY_PATH); err != nil {
		return hidden, fmt.Errorf("move_mount: %w", err)
	}
	return hidden, nil
}

// openFileInRoot opens dest beneath root as a file (symbolic links in the image resolve inside it), making its
// directories and an empty file when missing. A destination that is a directory is refused, as runc refuses a file
// bound onto one.
func openFileInRoot(root int, dest string) (int, error) {
	rel := strings.TrimPrefix(dest, "/")
	how := &unix.OpenHow{Flags: unix.O_PATH | unix.O_CLOEXEC, Resolve: unix.RESOLVE_IN_ROOT | unix.RESOLVE_NO_MAGICLINKS}
	fd, err := unix.Openat2(root, rel, how)
	if errors.Is(err, unix.ENOENT) {
		pfd := root // a destination directly under the task's root
		if dir := filepath.Dir(dest); dir != "/" {
			var perr error
			if pfd, perr = openInRoot(root, dir); perr != nil {
				return -1, perr
			}
		}
		c, cerr := unix.Openat(pfd, filepath.Base(dest), unix.O_CREAT|unix.O_EXCL|unix.O_WRONLY|unix.O_NOFOLLOW|unix.O_CLOEXEC, 0o644)
		if pfd != root {
			unix.Close(pfd)
		}
		if cerr != nil && !errors.Is(cerr, unix.EEXIST) {
			return -1, fmt.Errorf("make %s: %w", dest, cerr)
		}
		if cerr == nil {
			unix.Close(c)
		}
		fd, err = unix.Openat2(root, rel, how)
	}
	if err != nil {
		return -1, err
	}
	var st unix.Stat_t
	if err := unix.Fstat(fd, &st); err != nil {
		unix.Close(fd)
		return -1, err
	}
	if st.Mode&unix.S_IFMT == unix.S_IFDIR {
		unix.Close(fd)
		return -1, fmt.Errorf("the destination %s is a directory; a file can only be bound onto a file", dest)
	}
	return fd, nil
}

func mountOne(rfd int, s Share) error {
	fd, err := openInRoot(rfd, s.Dest)
	if err != nil {
		return err
	}
	f := os.NewFile(uintptr(fd), s.Dest)
	defer f.Close()
	return mountAt(f, s)
}

// mountAt mounts s's export on the directory f refers to, in a child that a timeout can kill.
func mountAt(f *os.File, s Share) error {
	ctx, cancel := context.WithTimeout(context.Background(), MountTimeout)
	defer cancel()
	ro := "rw"
	if s.ReadOnly {
		ro = "ro"
	}
	// The child mounts on descriptor 3, the directory resolved here; SIGKILL ends it if the server stalls (an NFS
	// mount's RPC waits are killable).
	cmd := exec.CommandContext(ctx, "/proc/self/exe", "shares", "mount1", s.Export, ro)
	cmd.ExtraFiles = []*os.File{f}
	out, err := cmd.CombinedOutput()
	if ctx.Err() != nil {
		return fmt.Errorf("the share server did not answer the mount of %s within %s", s.Export, MountTimeout)
	}
	if err != nil {
		return fmt.Errorf("mount %s: %s", s.Export, strings.TrimSpace(string(out)))
	}
	return nil
}

// MountOne is the child of mountOne: it mounts export on descriptor 3.
func MountOne(export, mode string) error {
	if err := checkExport(export, unfs.MaxSources); err != nil {
		return err
	}
	var flags uintptr
	if mode == "ro" {
		flags |= unix.MS_RDONLY
	}
	// The kernel resolves the descriptor's magic link to the directory itself, not to a path that could change.
	return unix.Mount(unfs.ServerAddr+":"+export, "/proc/self/fd/3", "nfs", flags, MountOptions())
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

// unmountList unmounts the record's entries, last first: a file share's bind before its hidden mount.
func unmountList(rfd int, dests []string) error {
	var first error
	for i := len(dests) - 1; i >= 0; i-- {
		var fd int
		var err error
		if h, ok := strings.CutPrefix(dests[i], hiddenMark); ok {
			fd, err = unix.Open(h, unix.O_PATH|unix.O_DIRECTORY|unix.O_NOFOLLOW|unix.O_CLOEXEC, 0)
		} else {
			fd, err = unix.Openat2(rfd, strings.TrimPrefix(dests[i], "/"), &unix.OpenHow{
				Flags: unix.O_PATH | unix.O_CLOEXEC, Resolve: unix.RESOLVE_IN_ROOT | unix.RESOLVE_NO_MAGICLINKS})
		}
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
// reaches macOS leaves the file's size and mtime as they were, except that unfs3 sets times to the whole second
// (TD-066): a fractional mtime loses its fraction and comes back once, and the next replay changes nothing, so the
// launcher's (mtime, size) filter ends the loop there. It returns the counts it prints at the end.
func Replay(r io.Reader, sessionDir, root string) (touched, missing, rejected int) {
	sc := bufio.NewScanner(r)
	sc.Buffer(make([]byte, 64*1024), 64*1024)
	var dests map[string][]string
	files := map[string]bool{}
	load := func() {
		list, err := readList(sessionDir)
		if err != nil {
			return
		}
		dests = map[string][]string{}
		for _, s := range list {
			dests[s.Export] = append(dests[s.Export], s.Dest)
			files[s.Export] = s.File
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
			p := strings.TrimPrefix(d, "/") + "/" + e.Rest
			if files[unfs.ExportPath(e.Export)] {
				if e.Rest != unfs.FileName {
					rejected++
					continue
				}
				p = strings.TrimPrefix(d, "/") // the bound file itself (GH #208)
			}
			if touchInRoot(rfd, p, e.Mtime) {
				touched++
			} else {
				missing++
			}
		}
	}
	return touched, missing, rejected
}

// touchInRoot sets rel's mtime, found beneath root with no link leaving it, to its own value. A server that reports
// whole seconds (an unpatched unfs3, TD-066) shows no fraction, and setting that would floor the macOS mtime; then
// the event's mtime, the macOS mtime rounded up, is set instead when it is not older (D-043).
func touchInRoot(rfd int, rel string, mtime int64) bool {
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
	p := fmt.Sprintf("/proc/self/fd/%d", fd)
	set := st.Mtim
	if st.Mtim.Nsec == 0 && mtime >= st.Mtim.Sec && mtime > 0 {
		set = unix.Timespec{Sec: mtime, Nsec: 0}
	}
	ts := []unix.Timespec{{Sec: 0, Nsec: unix.UTIME_OMIT}, set}
	if unix.UtimesNanoAt(unix.AT_FDCWD, p, ts, 0) != nil {
		return false
	}
	// A host write that landed between the read and the set has had its mtime set back to the older value. The
	// reply to the set carries the file's attributes, so a size change shows it; then set the mtime to now, which
	// is never older than that write (the System VM's sink does the same, GH #150).
	var after unix.Stat_t
	if unix.Fstat(fd, &after) == nil && after.Size != st.Size {
		now := []unix.Timespec{{Sec: 0, Nsec: unix.UTIME_OMIT}, {Sec: 0, Nsec: unix.UTIME_NOW}}
		_ = unix.UtimesNanoAt(unix.AT_FDCWD, p, now, 0)
	}
	return true
}
