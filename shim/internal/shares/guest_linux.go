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
	hiddenUp := map[string]bool{} // one hidden mount for each export and mode, however many destinations bind it
	for _, s := range Sorted(list) {
		var err error
		if s.File {
			var hidden string
			hidden, err = mountFile(rfd, sessionDir, s, hiddenUp)
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

// Mounted returns the task-side paths of the shares MountAll recorded for a session (root joined with each
// destination), in mount order; hidden mounts outside the task's root are left out. No record means no shares.
func Mounted(sessionDir, root string) ([]string, error) {
	raw, err := os.ReadFile(filepath.Join(sessionDir, mountedFile))
	if errors.Is(err, os.ErrNotExist) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	var out []string
	for _, l := range strings.Split(string(raw), "\n") {
		if l == "" || strings.HasPrefix(l, hiddenMark) {
			continue
		}
		out = append(out, filepath.Join(root, l))
	}
	return out, nil
}

// mountFile mounts a file share's export at a hidden directory of the session, outside the task's root, and binds its
// one file onto the destination, which it makes as an empty file when missing (GH #208). up lists the hidden
// directories mounted so far: a file bound at two destinations with one mode has one hidden mount, and a file bound
// both read-write and read-only has one for each mode, so a bind and the mount it is cloned from always agree. It
// returns the hidden directory when it mounted it, so a failure after it can be undone.
func mountFile(rfd int, sessionDir string, s Share, up map[string]bool) (string, error) {
	hidden, made := hiddenDir(sessionDir, s.Export, s.ReadOnly), ""
	if !up[hidden] {
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
		up[hidden], made = true, hidden
	}
	dst, err := openFileInRoot(rfd, s.Dest)
	if err != nil {
		return made, err
	}
	defer unix.Close(dst)
	t, err := cloneExportFile(hidden, s.ReadOnly)
	if err != nil {
		return made, err
	}
	defer unix.Close(t)
	if err := unix.MoveMount(t, "", dst, "", unix.MOVE_MOUNT_F_EMPTY_PATH|unix.MOVE_MOUNT_T_EMPTY_PATH); err != nil {
		return made, fmt.Errorf("move_mount: %w", err)
	}
	return made, nil
}

// hiddenDir is where a file share's export is mounted in the guest, outside the task's root: one place for each
// export and mode. A read-only mount and a read-write mount of one export are two file systems to the guest, and a
// bind is compared with, and cloned from, the one of its own mode (council COR-1).
func hiddenDir(sessionDir, export string, readOnly bool) string {
	mode := "rw"
	if readOnly {
		mode = "ro"
	}
	return filepath.Join(sessionDir, "share-file"+strings.TrimPrefix(export, "/s/")+"-"+mode)
}

// cloneExportFile clones the one file of an export mounted at hidden into a detached mount with the bind's
// attributes.
func cloneExportFile(hidden string, readOnly bool) (int, error) {
	src, err := unix.Open(filepath.Join(hidden, unfs.FileName), unix.O_PATH|unix.O_NOFOLLOW|unix.O_CLOEXEC, 0)
	if err != nil {
		return -1, fmt.Errorf("the export holds no file: %w", err)
	}
	defer unix.Close(src)
	t, err := unix.OpenTree(src, "", unix.OPEN_TREE_CLONE|unix.OPEN_TREE_CLOEXEC|unix.AT_EMPTY_PATH)
	if err != nil {
		return -1, fmt.Errorf("open_tree: %w", err)
	}
	set := uint64(unix.MOUNT_ATTR_NOSUID | unix.MOUNT_ATTR_NODEV)
	if readOnly {
		set |= unix.MOUNT_ATTR_RDONLY
	}
	if err := unix.MountSetattr(t, "", unix.AT_EMPTY_PATH, &unix.MountAttr{Attr_set: set}); err != nil {
		unix.Close(t)
		return -1, fmt.Errorf("mount_setattr: %w", err)
	}
	return t, nil
}

// ReplayLog is where Replay reports a bind it made again, or failed to: the guest's console in the microVM, because
// the sender of an event may be gone before there is anything to answer.
var ReplayLog io.Writer = os.Stderr

// ReplayAnswer is where Replay answers a notice for a file share (an event with an empty rest), one line each:
// "BIND N same" when every bind of export N is the export's file, "again" when one was bound again, "failed" when
// one could not be, "none" when the session has no bind of it. The share server's supervisor reads the line
// (NotifyBind) and stops telling once the binds are the same twice in a row.
var ReplayAnswer io.Writer = io.Discard

// ReplayStall ends the process when one event has waited this long on the share server; zero waits for ever.
var ReplayStall time.Duration

// resolveDest finds where the bind of dest sits: the directory, opened inside the task's root, and the name in it.
// The first bind followed a symbolic link at the end of dest (openFileInRoot), so the bind sits on the link's
// target; a stale bind cannot be opened to ask, so the links are followed here by name, inside the root, at most
// eight of them (council COR-3). The caller closes the directory.
func resolveDest(rfd int, root, dest string) (int, string, error) {
	cur := filepath.Clean("/" + dest)
	for hop := 0; ; hop++ {
		dir, base := filepath.Dir(cur), filepath.Base(cur)
		pfd, err := unix.Openat2(rfd, strings.TrimPrefix(dir, "/")+"/.", &unix.OpenHow{
			Flags: unix.O_PATH | unix.O_DIRECTORY | unix.O_CLOEXEC, Resolve: unix.RESOLVE_IN_ROOT | unix.RESOLVE_NO_MAGICLINKS})
		if err != nil {
			return -1, "", fmt.Errorf("the directory of %s: %w", dest, err)
		}
		var st unix.Stat_t
		// A stale bind answers ESTALE here: then the name is the bind itself, not a link.
		if err := unix.Fstatat(pfd, base, &st, unix.AT_SYMLINK_NOFOLLOW); err != nil || st.Mode&unix.S_IFMT != unix.S_IFLNK {
			return pfd, base, nil
		}
		buf := make([]byte, unix.PathMax)
		n, err := unix.Readlinkat(pfd, base, buf)
		realDir := dir
		if real, rerr := os.Readlink(fmt.Sprintf("/proc/self/fd/%d", pfd)); rerr == nil {
			if rel, ok := strings.CutPrefix(real, strings.TrimSuffix(root, "/")); ok && (rel == "" || strings.HasPrefix(rel, "/")) {
				realDir = "/" + strings.TrimPrefix(rel, "/")
			}
		}
		unix.Close(pfd)
		if err != nil || hop == 8 {
			return -1, "", fmt.Errorf("the destination %s: too many symbolic links, or one that does not read", dest)
		}
		if target := string(buf[:n]); strings.HasPrefix(target, "/") {
			cur = filepath.Clean(target)
		} else {
			// Relative to the directory the link is really in: dir may itself have been reached through a link, and
			// ".." in the target then means another place than the text of dir says (judge JDG-9).
			cur = filepath.Clean(filepath.Join(realDir, target))
		}
	}
}

// rebindFile binds a file share again at dest when the bind there is not the file the export holds now (D-052).
// macOS replaced the file (an editor's save renames a new file over the old one); the share server attached the new
// one, and the bind still holds the old one, which answers every use with a stale handle.
//
// It asks the server about both. The export's file is looked up again by the client when its handle has gone stale.
// The bind is another file when it is stale or has another inode number. A stale bind cannot be opened, not even
// to mount on it, so nothing can be attached beneath it: the kernel only lets it be unmounted by name. So the old
// bind is removed and the new file attached at once after it. Between those two calls a lookup of the path finds
// the image's own file there, or the empty file the first bind made. A process that holds the old file open keeps
// a stale descriptor and must open the path again.
//
// The destination's directory is resolved inside the task's root (resolveDest), and the bind is named beneath that
// descriptor, with no link followed. The task is root in its guest and may have mounted something else there; then that is
// what goes, and the task did it to itself.
func rebindFile(rfd int, root, sessionDir string, s Share) (bool, error) {
	hidden := hiddenDir(sessionDir, s.Export, s.ReadOnly)
	var now, bound unix.Statx_t
	// The NFS client asks the server only when the request names an attribute that can change: for the inode number
	// alone it answers from memory, whatever AT_STATX_FORCE_SYNC says. So the basic set is asked for.
	const fresh = unix.AT_STATX_FORCE_SYNC | unix.AT_SYMLINK_NOFOLLOW
	if err := unix.Statx(unix.AT_FDCWD, filepath.Join(hidden, unfs.FileName), fresh, unix.STATX_BASIC_STATS, &now); err != nil {
		return false, fmt.Errorf("the export's file: %w", err)
	}
	pfd, base, err := resolveDest(rfd, root, s.Dest)
	if err != nil {
		return false, err
	}
	defer unix.Close(pfd)
	err = unix.Statx(pfd, base, fresh, unix.STATX_BASIC_STATS, &bound)
	switch {
	case err == nil && bound.Ino == now.Ino && bound.Dev_major == now.Dev_major && bound.Dev_minor == now.Dev_minor:
		return false, nil
	case err != nil && !errors.Is(err, unix.ESTALE):
		return false, fmt.Errorf("the bind at %s: %w", s.Dest, err)
	}
	t, err := cloneExportFile(hidden, s.ReadOnly)
	if err != nil {
		return false, err
	}
	defer unix.Close(t)
	// EINVAL: nothing is mounted there. A destination inside another share is a name on the server; when the host
	// replaced the file, the client dropped the name and the kernel dropped the bind with it. What shows there now is
	// the outer share's own view of the file, writable when that share is, until the new bind covers it.
	if err := unix.Unmount(fmt.Sprintf("/proc/self/fd/%d/%s", pfd, base), unix.MNT_DETACH|unix.UMOUNT_NOFOLLOW); err != nil && !errors.Is(err, unix.EINVAL) {
		return false, fmt.Errorf("remove the old bind at %s: %w", s.Dest, err)
	}
	if err := unix.MoveMount(t, "", pfd, base, unix.MOVE_MOUNT_F_EMPTY_PATH); err != nil {
		return false, fmt.Errorf("the old bind at %s is removed and the new file did not attach: %w", s.Dest, err)
	}
	return true, nil
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
		return fmt.Errorf("mount %s: %s (%s)", s.Export, strings.TrimSpace(string(out)), serverPorts())
	}
	return nil
}

// serverPorts says, for a mount that failed, whether the server's two ports take a connection now: the NFS port,
// which the probe before the mounts checked, and the mount port, which it did not.
func serverPorts() string {
	var parts []string
	for _, p := range []int{unfs.NFSPort, unfs.MountPort} {
		state := "open"
		c, err := net.DialTimeout("tcp", net.JoinHostPort(unfs.ServerAddr, fmt.Sprint(p)), time.Second)
		if err != nil {
			state = err.Error()
			if i := strings.LastIndex(state, ": "); i >= 0 {
				state = state[i+2:]
			}
		} else {
			c.Close()
		}
		parts = append(parts, fmt.Sprintf("port %d: %s", p, state))
	}
	return strings.Join(parts, ", ")
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
//
// An event for a file share first checks the bind (rebindFile, D-052): a file that macOS replaced is bound again
// before it is touched. An event with an empty rest for a file share, which the share server's supervisor sends
// after it attached a replacing file, checks the bind and touches nothing. Each new bind is reported on ReplayLog.
func Replay(r io.Reader, sessionDir, root string) (touched, missing, rejected int) {
	sc := bufio.NewScanner(r)
	sc.Buffer(make([]byte, 64*1024), 64*1024)
	var dests map[string][]string
	files := map[string]bool{}
	byDest := map[string]Share{}
	load := func() {
		list, err := readList(sessionDir)
		if err != nil {
			return
		}
		dests = map[string][]string{}
		for _, s := range list {
			dests[s.Export] = append(dests[s.Export], s.Dest)
			files[s.Export] = s.File
			byDest[s.Dest] = s
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
		// A notice (an empty rest) may wait on the share server, and a server that is gone never answers a hard
		// mount. Past ReplayStall the process ends, which the kernel allows a waiting NFS request; the supervisor
		// sends its notice again. Other events come down one long connection from the fanout, which counts a line
		// as sent once written: ending that replay would lose the lines behind it, so those wait as they always did.
		var stall *time.Timer
		if ReplayStall > 0 && e.Rest == "" {
			stall = time.AfterFunc(ReplayStall, func() {
				fmt.Fprintf(ReplayLog, "VESPER-SHARE-REPLAY-STALLED export %d after %s\n", e.Export, ReplayStall)
				os.Exit(3)
			})
		}
		t, m, r := replayOne(e, rfd, root, sessionDir, load, &dests, files, byDest)
		if stall != nil {
			stall.Stop()
		}
		touched, missing, rejected = touched+t, missing+m, rejected+r
	}
	return touched, missing, rejected
}

// replayOne applies one event and returns what it adds to Replay's counts.
func replayOne(e Event, rfd int, root, sessionDir string, load func(), all *map[string][]string, files map[string]bool, byDest map[string]Share) (touched, missing, rejected int) {
	{
		if *all == nil {
			load()
		}
		dests := *all
		ds := dests[unfs.ExportPath(e.Export)]
		if len(ds) == 0 {
			load() // the session may have started since
			dests = *all
			ds = dests[unfs.ExportPath(e.Export)]
		}
		if len(ds) == 0 {
			if e.Rest == "" {
				fmt.Fprintf(ReplayAnswer, "BIND %d none\n", e.Export)
			}
			missing++
			return
		}
		outcome := "same" // of a notice for a file share: what its binds are now
		defer func() {
			if e.Rest == "" && files[unfs.ExportPath(e.Export)] {
				fmt.Fprintf(ReplayAnswer, "BIND %d %s\n", e.Export, outcome)
			}
		}()
		for _, d := range ds {
			p := strings.TrimPrefix(d, "/") + "/" + e.Rest
			if files[unfs.ExportPath(e.Export)] {
				if e.Rest != unfs.FileName && e.Rest != "" {
					rejected++
					continue
				}
				again, err := rebindFile(rfd, root, sessionDir, byDest[d])
				if again {
					fmt.Fprintf(ReplayLog, "VESPER-SHARE-REBOUND %s\n", d)
					if outcome == "same" {
						outcome = "again"
					}
				}
				if err != nil {
					fmt.Fprintf(ReplayLog, "VESPER-SHARE-REBIND-FAILED %s: %v\n", d, err)
					outcome = "failed"
				}
				if e.Rest == "" { // the supervisor's notice: nothing changed in the file to tell watchers of
					if again {
						touched++
					}
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
