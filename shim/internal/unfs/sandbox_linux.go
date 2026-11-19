//go:build linux

package unfs

import (
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"os/signal"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"sync"
	"syscall"
	"time"

	"golang.org/x/sys/unix"
)

// Config is what `vesper-unfs run` is given by vesper-microvm.
type Config struct {
	Mounts  []string // the trusted virtio-fs share mounts the sources lie in (Source.Mount indexes it)
	Sources []Source // exported as /s/0, /s/1, ...
	Squash  bool     // run unfsd as SquashID with no capabilities (all_squash); else root with RootCaps
	Root    string   // an empty directory to mount the server's tmpfs root on (in its own mount namespace)
	Unfsd   string   // the unfsd binary, under /usr so the read-only /usr bind carries it
	Cgroup  string   // a cgroup directory to join before the server starts; empty for none
	AnyFS   bool     // accept a mount that is not virtio-fs (unit tests on a plain directory only)
	// Selfcheck runs SelfcheckBin (under /usr) with argument "selfcheck" in place of unfsd: the same sandbox and
	// privileges, for the escape tests (GH #160 M3).
	Selfcheck    bool
	SelfcheckBin string
	// TestPauseNests holds the sandbox for this long before it places the nests (tests only: the suite renames a
	// nested source in the meantime and expects a refusal).
	TestPauseNests time.Duration
	// TestNoAnchors leaves the anchors out (tests only: the suites' steps that begin with a task renaming a
	// directory on the way to a read-only source, to reach the layers beneath; TD-080).
	TestNoAnchors bool
	// NotifyGuest tells the guest to look again at the binds of file export N, after the supervisor attached the
	// file that replaced it on the host (D-052), and returns the guest's answer: "same" when its binds are the
	// export's file, "again" when it bound one again, "failed", "none" when it has no such bind, or "" when it
	// said nothing. Nil when there is no guest to tell.
	NotifyGuest func(export int) (string, error)
	// ProtectFile is where the supervisor records, across its restarts, what each read-only source was (TD-073):
	// the server keeps those objects read-only by number. Empty keeps no record.
	ProtectFile string
	// list is set by Run: the server's list, open for AppendSeen. The sandbox has that file attached.
	list *os.File
	// anchors is set by Run: the directories on the way to the read-only sources, which the watcher keeps current.
	anchors *anchorState
	// unrecorded is set by Run: the read-only sources that are neither recorded nor held. They are the read-write
	// sources that this start serves read-only only because their path could not be compared with the record, and
	// the sources whose path was also given read-write.
	unrecorded map[int]bool
}

// childConfig crosses into the sandbox in the environment; the sources cross as descriptors 3, 4, ...
type childConfig struct {
	Sources []Source `json:"sources"`
	Nests   []Nest   `json:"nests"`
	Squash  bool     `json:"squash"`
	Root    string   `json:"root"`
	Unfsd   string   `json:"unfsd"`
	// Check, when set, is run with argument "selfcheck" in place of unfsd.
	Check string `json:"check,omitempty"`
	// PauseNestsMs: Config.TestPauseNests.
	PauseNestsMs int64 `json:"pause_nests_ms,omitempty"`
}

const (
	configEnv = "VESPER_UNFS_CONFIG"
	firstFd   = 3
	// RootCaps is the bounding set of a root server: acting as each caller and nothing else (ADR 0009, Sandbox):
	// CAP_CHOWN, CAP_DAC_OVERRIDE, CAP_FOWNER, CAP_FSETID, CAP_SETGID, CAP_SETUID.
	RootCaps = 1<<unix.CAP_CHOWN | 1<<unix.CAP_DAC_OVERRIDE | 1<<unix.CAP_FOWNER | 1<<unix.CAP_FSETID |
		1<<unix.CAP_SETGID | 1<<unix.CAP_SETUID
	stopGrace = 5 * time.Second
)

// OpenSources opens each source relative to its share mount, with no symbolic link on the way, beneath the mount
// and on it alone, and clones what it opened into detached mount trees with their final attributes: one per source,
// then one per nest (Nests order) of the read-only source it carries. Nothing binds from a directory source's
// path again after this (ADR 0009, Source binding); a file source's path is looked at again, to attach a file that
// replaced it (reattach_linux.go, D-052), and the directories of a chain are opened again only to learn their
// numbers (anchor_linux.go, D-058). The trees are made here because open_tree clones only a mount of the caller's own
// mount namespace; the sandbox, in a new one, attaches them with move_mount.
//
// each, when not nil, is called with every source's own descriptor (an O_PATH descriptor in this mount namespace)
// before the trees are made, for a caller that needs more than a detached tree of it.
func OpenSources(mounts []string, srcs []Source, anyFS bool, each func(i int, fd int)) ([]*os.File, error) {
	if err := (Args{Mounts: mounts, Sources: srcs}).Check(); err != nil {
		return nil, err
	}
	var mfds, dirs []int
	defer func() {
		for _, fd := range append(mfds, dirs...) {
			unix.Close(fd)
		}
	}()
	for _, mount := range mounts {
		mfd, err := unix.Open(mount, unix.O_PATH|unix.O_DIRECTORY|unix.O_NOFOLLOW|unix.O_CLOEXEC, 0)
		if err != nil {
			return nil, fmt.Errorf("share mount %s: %w", mount, err)
		}
		mfds = append(mfds, mfd)
		var sfs unix.Statfs_t
		if err := unix.Fstatfs(mfd, &sfs); err != nil {
			return nil, fmt.Errorf("share mount %s: %w", mount, err)
		}
		if !anyFS && sfs.Type != unix.FUSE_SUPER_MAGIC { // virtio-fs reports the FUSE magic
			return nil, fmt.Errorf("share mount %s is not virtio-fs (type %#x)", mount, sfs.Type)
		}
	}
	for _, s := range srcs {
		fd, err := openLive(mfds[s.Mount], s.Rel, s.File, time.Now().Add(StaleWait))
		if err != nil {
			return nil, fmt.Errorf("source %q: %w", s.Rel, err)
		}
		dirs = append(dirs, fd)
	}
	if each != nil {
		for i, fd := range dirs {
			each(i, fd)
		}
	}
	var files []*os.File
	fail := func(err error) ([]*os.File, error) {
		for _, f := range files {
			f.Close()
		}
		return nil, err
	}
	noexec := uint64(unix.MOUNT_ATTR_NOSUID | unix.MOUNT_ATTR_NODEV | unix.MOUNT_ATTR_NOEXEC)
	for i, s := range srcs {
		set := noexec
		if s.ReadOnly {
			set |= unix.MOUNT_ATTR_RDONLY
		}
		t, err := detach(dirs[i], set)
		if err != nil {
			return fail(fmt.Errorf("source %q: %w", s.Rel, err))
		}
		files = append(files, os.NewFile(uintptr(t), s.Rel))
	}
	for _, n := range Nests(srcs) {
		t, err := detach(dirs[n.Inner], noexec|unix.MOUNT_ATTR_RDONLY)
		if err != nil {
			return fail(fmt.Errorf("nest of %q: %w", srcs[n.Inner].Rel, err))
		}
		files = append(files, os.NewFile(uintptr(t), srcs[n.Inner].Rel))
	}
	return files, nil
}

// StaleWait bounds how long OpenSources waits for the System VM to see a source directory that macOS has just made
// again under the same name (TD-071).
var StaleWait = 15 * time.Second

// openLive opens rel beneath mfd (no symbolic link, no other mount) and checks that the directory still exists. A
// directory deleted on macOS and made again under the same name can still be the deleted one in the System VM for a
// few seconds: virtio-fs keeps the name's old entry until its timeout. Reading the directory always asks macOS, and
// the kernel refuses to read a dead one (ENOENT), so a dead directory is opened again until the entry expires or the
// deadline passes; the server never serves a directory that is gone (TD-071).
//
// A file source (GH #208) is opened the same way without O_DIRECTORY, and must be a regular file: a FIFO or a socket
// would hang the single-threaded server on a READ (council SEC-7).
func openLive(mfd int, rel string, file bool, deadline time.Time) (int, error) {
	flags := uint64(unix.O_PATH | unix.O_DIRECTORY | unix.O_CLOEXEC)
	if file {
		flags = unix.O_PATH | unix.O_CLOEXEC
	}
	for {
		fd, err := unix.Openat2(mfd, rel, &unix.OpenHow{
			Flags:   flags,
			Resolve: unix.RESOLVE_BENEATH | unix.RESOLVE_NO_SYMLINKS | unix.RESOLVE_NO_MAGICLINKS | unix.RESOLVE_NO_XDEV,
		})
		if err != nil {
			return -1, err
		}
		if file {
			var st unix.Stat_t
			if err := unix.Fstat(fd, &st); err != nil || st.Mode&unix.S_IFMT != unix.S_IFREG {
				unix.Close(fd)
				if err == nil {
					err = fmt.Errorf("not a regular file (mode %#o)", st.Mode&unix.S_IFMT)
				}
				return -1, err
			}
			err = nil
		} else {
			err = dirAlive(fd)
		}
		if err == nil {
			err = refreshAttrs(fd)
		}
		if err == nil {
			return fd, nil
		}
		unix.Close(fd)
		if !(errors.Is(err, unix.ENOENT) || errors.Is(err, unix.ESTALE)) {
			return -1, err
		}
		if time.Now().After(deadline) {
			return -1, fmt.Errorf("the directory was deleted and the System VM still sees the deleted one: %w", err)
		}
		time.Sleep(250 * time.Millisecond)
	}
}

// refreshAttrs asks macOS for the directory's attributes again. Apple's virtio-fs answers for a name: a node the
// System VM still holds for a deleted directory reads as the new one, but its cached inode number is the old one
// until it expires, and unfs3, which checks a handle's inode against lstat, would then refuse the handles it gave
// out before the expiry (ESTALE in the guest). A changed inode number means the open raced the remade directory:
// open it again (TD-071).
func refreshAttrs(fd int) error {
	var before unix.Stat_t
	if err := unix.Fstat(fd, &before); err != nil {
		return err
	}
	var now unix.Statx_t
	if err := unix.Statx(fd, "", unix.AT_EMPTY_PATH|unix.AT_STATX_FORCE_SYNC, unix.STATX_TYPE|unix.STATX_INO, &now); err != nil {
		return err
	}
	if now.Ino != before.Ino {
		return fmt.Errorf("inode %d became %d: %w", before.Ino, now.Ino, unix.ESTALE)
	}
	return nil
}

// dirAlive reads the directory fd refers to (an O_PATH descriptor) once.
func dirAlive(fd int) error {
	d, err := unix.Openat(fd, ".", unix.O_RDONLY|unix.O_DIRECTORY|unix.O_CLOEXEC, 0)
	if err != nil {
		return err
	}
	defer unix.Close(d)
	buf := make([]byte, 4096)
	_, err = unix.Getdents(d, buf)
	return err
}

// detach clones the directory fd refers to into a detached mount tree and sets its attributes before anything can
// see it.
func detach(fd int, set uint64) (int, error) {
	t, err := unix.OpenTree(fd, "", unix.OPEN_TREE_CLONE|unix.OPEN_TREE_CLOEXEC|unix.AT_EMPTY_PATH)
	if err != nil {
		return -1, fmt.Errorf("open_tree: %w", err)
	}
	if err := unix.MountSetattr(t, "", unix.AT_EMPTY_PATH, &unix.MountAttr{Attr_set: set}); err != nil {
		unix.Close(t)
		return -1, fmt.Errorf("mount_setattr: %w", err)
	}
	return t, nil
}

// Run opens the sources, starts the sandbox (this binary's `init`) in new mount, pid, ipc and uts namespaces, passes
// TERM, INT and HUP on to it, and returns its exit code. It runs in the microVM's network namespace already
// (vesper-microvm starts it under nsenter), which the sandbox keeps.
func Run(cfg Config) (int, error) {
	// The sandbox is killed when the thread that started it ends (Pdeathsig is the thread's, not the process's), and
	// attaching a replaced file ends threads on purpose (inMountNamespace). So this goroutine keeps its thread, and
	// no other goroutine can run on it or end it (council REL-1).
	runtime.LockOSThread()
	// A start after the first: a task may have renamed directories since, so that a read-write source's path now
	// lies under an object an earlier start knew as read-only. Such a source is served read-only (council SEC-3).
	prior, err := ReadProtectIDs(cfg.ProtectFile)
	if err != nil {
		return 1, fmt.Errorf("record of read-only objects %s: %w", cfg.ProtectFile, err)
	}
	given := cfg.Sources // as configured: what is bound both ways is judged by this, not by what a start changes
	cfg.Sources = append([]Source(nil), cfg.Sources...)
	hit, failed := GuardSources(cfg.Mounts, cfg.Sources, prior)
	for _, i := range hit {
		fmt.Fprintf(os.Stderr, "vesper-unfs run: source %q now lies under an object that was bound read-only; it is served read-only\n", cfg.Sources[i].Rel)
		cfg.Sources[i].ReadOnly = true
	}
	// A source that could not be compared is served read-only too, for this start only: its own object is neither
	// recorded nor held, so the next start compares it again (council J-4).
	for i, err := range failed {
		fmt.Fprintf(os.Stderr, "vesper-unfs run: source %q could not be compared with the record of read-only objects (%v); it is served read-only until this server starts again\n", cfg.Sources[i].Rel, err)
		cfg.Sources[i].ReadOnly = true
	}
	// One rule for the start and for the watcher (council K-4): a read-only source is recorded and held unless
	// it could not be compared, or the same path was also given read-write.
	cfg.unrecorded = map[int]bool{}
	for i, s := range cfg.Sources {
		if _, unsure := failed[i]; s.ReadOnly && (unsure || alsoReadWrite(given, s)) {
			cfg.unrecorded[i] = true
		}
	}
	recorded := func(i int) bool { return cfg.Sources[i].ReadOnly && !cfg.unrecorded[i] }
	// Each read-only source's object gets a mount that outlives this process, so that it keeps its inode number
	// when a task moves it away and this server starts again (TD-076). A hold that fails is reported and the
	// start goes on: the object is then known only while the System VM does not forget it.
	holds := HoldDir(cfg.ProtectFile)
	files, err := OpenSources(cfg.Mounts, cfg.Sources, cfg.AnyFS, func(i int, fd int) {
		if recorded(i) && !cfg.Selfcheck {
			if err := hold(holds, fd, cfg.Sources[i].File); err != nil {
				fmt.Fprintf(os.Stderr, "vesper-unfs run: source %q: its object is not held across restarts: %v\n", cfg.Sources[i].Rel, err)
			}
		}
	})
	if err != nil {
		return 1, err
	}
	defer func() {
		for _, f := range files {
			f.Close()
		}
	}()
	if err := checkRootDir(cfg.Root); err != nil {
		return 1, err
	}
	if cfg.Cgroup != "" {
		if err := os.WriteFile(filepath.Join(cfg.Cgroup, "cgroup.procs"), []byte(strconv.Itoa(os.Getpid())), 0); err != nil {
			return 1, fmt.Errorf("join cgroup %s: %w", cfg.Cgroup, err)
		}
	}
	check := ""
	if cfg.Selfcheck {
		check = cfg.SelfcheckBin
	}
	// What each read-only source is now joins the record of what it was at earlier starts (TD-073).
	var now []ProtectID
	for i, s := range cfg.Sources {
		if recorded(i) {
			id, err := idOf(int(files[i].Fd()))
			if err != nil {
				return 1, fmt.Errorf("source %q: %w", s.Rel, err)
			}
			now = append(now, ProtectID{Dev: id.dev, Ino: id.ino})
		}
	}
	protect, err := RecordProtectIDs(cfg.ProtectFile, now...)
	if err != nil {
		return 1, fmt.Errorf("record of read-only objects %s: %w", cfg.ProtectFile, err)
	}
	if _, left, err := ReleaseHolds(holds, protect); left > 0 { // what the record no longer names
		fmt.Fprintf(os.Stderr, "vesper-unfs run: %d holds that the record no longer names are still there: %v\n", left, err)
	}
	// The server's list is a file of the supervisor's own, attached read-only in the sandbox: the supervisor adds
	// an object to it before it puts the object at a protected place (TD-078). It starts with what is at each
	// protected place now. Every sandbox has one, so there is one shape to test: with no record file (the escape
	// checks, a direct call) the list is in a directory of its own that goes when this returns.
	var seen []ProtectID
	for i, s := range cfg.Sources {
		if hasPlace(cfg.Sources, s) {
			id, err := idOf(int(files[i].Fd()))
			if err != nil {
				return 1, fmt.Errorf("source %q: %w", s.Rel, err)
			}
			seen = append(seen, ProtectID{Dev: id.dev, Ino: id.ino})
		}
	}
	list := ListFile(cfg.ProtectFile)
	if list == "" || cfg.Selfcheck {
		dir, err := os.MkdirTemp("", "vesper-unfs-list-")
		if err != nil {
			return 1, fmt.Errorf("a directory for the server's list: %w", err)
		}
		defer os.RemoveAll(dir) // this directory holds the list and its temporary file, and no mount
		list = filepath.Join(dir, "list")
	}
	// The directories on the way to each read-only source inside a read-write one: the server refuses a rename or
	// a removal of them, so that a task cannot change what a read-only source's path names (TD-080).
	var anchorIDs []ProtectID
	switch {
	case cfg.Selfcheck:
	case cfg.TestNoAnchors:
		fmt.Fprintln(os.Stderr, "vesper-unfs test: started with --test-no-anchors: a task can rename the directories on the way to its read-only sources")
	default:
		roots := map[fileID]bool{}
		for i, s := range cfg.Sources {
			if !s.ReadOnly && !s.File { // a task can rename directories through a read-write directory source
				id, err := idOf(int(files[i].Fd()))
				if err != nil {
					return 1, fmt.Errorf("source %q: %w", s.Rel, err)
				}
				roots[id] = true
			}
		}
		a, ids, err := startAnchors(cfg.Mounts, cfg.Sources, roots)
		if err != nil {
			return 1, err
		}
		cfg.anchors, anchorIDs = a, ids
		// The directory now at a read-only directory source's path is read-only with what lies in it (TD-081). It
		// is the source that was just opened, which is in seen already, unless macOS replaced it in between.
		for _, leaf := range leafList(cfg.Sources, a.chains) {
			known := false
			for _, id := range seen {
				known = known || id.same(leaf)
			}
			if !known {
				seen = append(seen, leaf)
			}
		}
	}
	if err := WriteList(list, ProtectList(cfg.Sources, protect, seen, anchorIDs)); err != nil {
		return 1, fmt.Errorf("the server's list %s: %w", list, err)
	}
	lw, err := OpenList(list)
	if err != nil {
		return 1, fmt.Errorf("the server's list %s: %w", list, err)
	}
	defer lw.Close()
	// The sandbox gets a read-only clone of the same file that lw writes to.
	lt, err := detach(int(lw.Fd()), unix.MOUNT_ATTR_RDONLY|unix.MOUNT_ATTR_NOSUID|unix.MOUNT_ATTR_NODEV|unix.MOUNT_ATTR_NOEXEC)
	if err != nil {
		return 1, fmt.Errorf("the server's list %s: %w", list, err)
	}
	files = append(files, os.NewFile(uintptr(lt), list)) // after the sources and the nests; closed with them
	cfg.list = lw
	cc, _ := json.Marshal(childConfig{Sources: cfg.Sources, Nests: Nests(cfg.Sources), Squash: cfg.Squash, Root: cfg.Root, Unfsd: cfg.Unfsd, Check: check, PauseNestsMs: cfg.TestPauseNests.Milliseconds()})
	cmd := exec.Command("/proc/self/exe", "init")
	cmd.Env = []string{configEnv + "=" + string(cc)}
	// The sandbox's and the server's last words are kept, for whoever asks why the server ended (LastWords). unfsd
	// in the foreground writes its own messages, a segmentation fault among them, to its standard output.
	lastWords.reset()
	cmd.Stdin, cmd.Stdout, cmd.Stderr = nil, io.MultiWriter(os.Stdout, &lastWords), io.MultiWriter(os.Stderr, &lastWords)
	cmd.ExtraFiles = files
	cmd.SysProcAttr = &syscall.SysProcAttr{
		Cloneflags: syscall.CLONE_NEWNS | syscall.CLONE_NEWPID | syscall.CLONE_NEWIPC | syscall.CLONE_NEWUTS,
		Pdeathsig:  syscall.SIGKILL,
	}
	sigs := make(chan os.Signal, 4)
	signal.Notify(sigs, syscall.SIGTERM, syscall.SIGINT, syscall.SIGHUP)
	defer signal.Stop(sigs)
	if err := cmd.Start(); err != nil {
		return 1, fmt.Errorf("start sandbox: %w", err)
	}
	// File sources are watched for a replacement on the host (D-052). The escape checks have no server to watch.
	if !cfg.Selfcheck {
		w, err := newFileWatch(cfg, files, cmd.Process.Pid)
		if err != nil {
			fmt.Fprintf(os.Stderr, "vesper-unfs run: replaced files will not be attached again, and the directories on the way to the read-only sources are not looked at again: %v\n", err)
		} else if w != nil {
			// The watcher ends the server when it cannot tell it of a file: the next start writes the list anew.
			w.end = func() {
				lastWords.forList()
				_ = cmd.Process.Kill()
			}
			stop := make(chan struct{})
			defer close(stop)
			go w.run(stop)
		}
	}
	done := make(chan error, 1)
	go func() { done <- cmd.Wait() }()
	var kill <-chan time.Time
	for {
		select {
		case s := <-sigs:
			if s != syscall.SIGHUP { // a reload is not a stop
				lastWords.told()
			}
			_ = cmd.Process.Signal(s)
			if s != syscall.SIGHUP && kill == nil {
				kill = time.After(stopGrace)
			}
		case <-kill:
			_ = cmd.Process.Kill()
		case err := <-done:
			var ee *exec.ExitError
			if errors.As(err, &ee) {
				if ws, ok := ee.Sys().(syscall.WaitStatus); ok && ws.Signaled() {
					return 128 + int(ws.Signal()), nil
				}
				return ee.ExitCode(), nil
			}
			if err != nil {
				return 1, err
			}
			return 0, nil
		}
	}
}

// checkRootDir accepts an existing directory owned by root and not a symbolic link, or makes one (mode 0700).
func checkRootDir(dir string) error {
	if !filepath.IsAbs(dir) {
		return fmt.Errorf("root %q is not absolute", dir)
	}
	st, err := os.Lstat(dir)
	if errors.Is(err, os.ErrNotExist) {
		if err := os.Mkdir(dir, 0o700); err != nil {
			return err
		}
		st, err = os.Lstat(dir)
	}
	if err != nil {
		return err
	}
	if !st.IsDir() || st.Mode()&os.ModeSymlink != 0 {
		return fmt.Errorf("root %s is not a directory", dir)
	}
	if s, ok := st.Sys().(*syscall.Stat_t); !ok || s.Uid != 0 {
		return fmt.Errorf("root %s is not owned by root", dir)
	}
	return nil
}

// Init is the sandbox: process 1 of a new pid namespace, in a new mount namespace. It builds a tmpfs root holding
// only the exports, a read-only /usr and /lib for unfsd, /dev/null, the exports file and the supervisor's list of
// protected objects, attached read-only at /etc/exports.protect (TD-078); moves into it with
// pivot_root and drops the old root; drops privileges; and becomes unfsd. Any failure ends it before unfsd starts.
func Init() error {
	runtime.LockOSThread() // capability and no_new_privs settings are per thread; exec happens on this one
	readCapLast()
	// The root's directories must be searchable by a squashing server whatever umask the caller had (the System
	// VM's tools use 077, ADR 0010); the exports file is readable, nothing is writable but the exports.
	unix.Umask(0o022)
	var cc childConfig
	if err := json.Unmarshal([]byte(os.Getenv(configEnv)), &cc); err != nil {
		return fmt.Errorf("config: %w", err)
	}
	if err := Check(cc.Sources); err != nil {
		return err
	}
	step := func(what string, err error) error {
		if err != nil {
			return fmt.Errorf("%s: %w", what, err)
		}
		return nil
	}
	// Nothing mounted here propagates back to the System VM.
	if err := step("make / private", unix.Mount("", "/", "", unix.MS_REC|unix.MS_PRIVATE, "")); err != nil {
		return err
	}
	root := cc.Root
	if err := step("tmpfs root", unix.Mount("vesper-unfs", root, "tmpfs", unix.MS_NOSUID|unix.MS_NODEV|unix.MS_NOEXEC, "size=1m,mode=0755")); err != nil {
		return err
	}
	for _, d := range []string{"old", "s", "usr", "lib", "etc", "dev"} {
		if err := os.Mkdir(filepath.Join(root, d), 0o755); err != nil {
			return err
		}
	}
	ro := uint64(unix.MOUNT_ATTR_RDONLY | unix.MOUNT_ATTR_NOSUID | unix.MOUNT_ATTR_NODEV)
	for _, d := range []string{"usr", "lib"} {
		if err := step("bind /"+d, attachPath("/"+d, filepath.Join(root, d), ro)); err != nil {
			return err
		}
	}
	if err := os.WriteFile(filepath.Join(root, "dev/null"), nil, 0o644); err != nil {
		return err
	}
	// Read-only: the device still opens for writing, but its inode is the System VM's own, so a SETATTR through a
	// symbolic link in an export (unfs3 follows them mid-path) must fail with EROFS rather than change the System
	// VM's /dev/null (council SEC-1).
	if err := step("bind /dev/null", attachPath("/dev/null", filepath.Join(root, "dev/null"), uint64(unix.MOUNT_ATTR_RDONLY|unix.MOUNT_ATTR_NOSUID|unix.MOUNT_ATTR_NOEXEC))); err != nil {
		return err
	}
	// Each source: the detached tree the parent cloned from the descriptor it opened, attributes already set.
	// A file source goes on an empty file FileName in /s/N, which its own read-only tmpfs (below) keeps from gaining
	// siblings, being renamed or having its mode changed (GH #208, council SEC-7).
	for i, s := range cc.Sources {
		dst := filepath.Join(root, ExportPath(i))
		if err := os.Mkdir(dst, 0o755); err != nil {
			return err
		}
		if s.File {
			// The export of a file is a small file system of its own. The supervisor attaches a file that replaced
			// this one beneath its mount (D-052), and the kernel refuses that directly under a namespace's root.
			if err := step(fmt.Sprintf("file system for source %d", i), unix.Mount("vesper-unfs-file", dst, "tmpfs", unix.MS_NOSUID|unix.MS_NODEV|unix.MS_NOEXEC, "size=4k,nr_inodes=4,mode=0755")); err != nil {
				return err
			}
			dir := dst
			dst = filepath.Join(dst, FileName)
			if err := os.WriteFile(dst, nil, 0o644); err != nil {
				return err
			}
			// Read-only like the root, so the export cannot gain siblings, or the file be renamed or change mode.
			if err := step(fmt.Sprintf("read-only file system for source %d", i), unix.MountSetattr(unix.AT_FDCWD, dir, 0, &unix.MountAttr{Attr_set: unix.MOUNT_ATTR_RDONLY})); err != nil {
				return err
			}
		}
		if err := step(fmt.Sprintf("attach source %d", i), attachFd(firstFd+i, dst, s.File)); err != nil {
			return err
		}
	}
	// Each read-only source again, read-only, inside each read-write source that holds it. The place is found
	// beneath the outer export with no symbolic link on the way, so a swapped directory cannot move the bind.
	if cc.PauseNestsMs > 0 {
		time.Sleep(time.Duration(cc.PauseNestsMs) * time.Millisecond)
	}
	for k, n := range cc.Nests {
		outer, err := unix.Open(filepath.Join(root, ExportPath(n.Outer)), unix.O_PATH|unix.O_DIRECTORY|unix.O_NOFOLLOW|unix.O_CLOEXEC, 0)
		if err != nil {
			return fmt.Errorf("nest %d in %d: %w", n.Inner, n.Outer, err)
		}
		nf := uint64(unix.O_PATH | unix.O_DIRECTORY | unix.O_CLOEXEC)
		if cc.Sources[n.Inner].File {
			nf = unix.O_PATH | unix.O_CLOEXEC // a read-only file inside a read-write directory (council ARC-15)
		}
		at, err := unix.Openat2(outer, n.Sub, &unix.OpenHow{
			Flags:   nf,
			Resolve: unix.RESOLVE_BENEATH | unix.RESOLVE_NO_SYMLINKS | unix.RESOLVE_NO_MAGICLINKS,
		})
		unix.Close(outer)
		if err != nil {
			return fmt.Errorf("nest %d in %d at %q: %w", n.Inner, n.Outer, n.Sub, err)
		}
		// The place must be the very directory the parent opened as the inner source. A host process could rename it
		// and put another directory in its place in between; binding the read-only tree there would leave the
		// original writable under its new name through the outer export. Once attached, the bind follows the
		// directory wherever it is renamed.
		tree := firstFd + len(cc.Sources) + k
		var want, got unix.Stat_t
		if err := unix.Fstat(tree, &want); err != nil {
			unix.Close(at)
			return fmt.Errorf("nest %d in %d: %w", n.Inner, n.Outer, err)
		}
		if err := unix.Fstat(at, &got); err != nil {
			unix.Close(at)
			return fmt.Errorf("nest %d in %d: %w", n.Inner, n.Outer, err)
		}
		if want.Dev != got.Dev || want.Ino != got.Ino {
			unix.Close(at)
			return fmt.Errorf("nest %d in %d at %q: the directory there is not the source that was opened (renamed or replaced)", n.Inner, n.Outer, n.Sub)
		}
		err = moveTree(tree, at)
		unix.Close(at)
		if err != nil {
			return fmt.Errorf("nest %d in %d: %w", n.Inner, n.Outer, err)
		}
	}
	if err := os.WriteFile(filepath.Join(root, "etc/exports"), []byte(Exports(cc.Sources, cc.Squash)), 0o644); err != nil {
		return err
	}
	// The objects the server keeps read-only whatever name reaches them (TD-073, unfs3 patch 0006).
	// The server's list is the supervisor's own file, which it adds to while the server runs (TD-078): the
	// descriptor after the sources and the nests, attached on an empty file.
	listAt := filepath.Join(root, "etc/exports.protect")
	if err := os.WriteFile(listAt, nil, 0o644); err != nil {
		return err
	}
	if err := step("attach the list", attachFd(firstFd+len(cc.Sources)+len(cc.Nests), listAt, true)); err != nil {
		return err
	}
	for _, f := range []string{"netconfig", "protocols", "services"} { // libtirpc's tables, if present
		if b, err := os.ReadFile("/etc/" + f); err == nil {
			if err := os.WriteFile(filepath.Join(root, "etc", f), b, 0o644); err != nil {
				return err
			}
		}
	}
	if err := step("pivot_root", unix.PivotRoot(root, filepath.Join(root, "old"))); err != nil {
		return err
	}
	if err := unix.Chdir("/"); err != nil {
		return err
	}
	if err := step("drop old root", unix.Unmount("/old", unix.MNT_DETACH)); err != nil {
		return err
	}
	if err := os.Remove("/old"); err != nil {
		return err
	}
	if err := step("read-only root", unix.MountSetattr(unix.AT_FDCWD, "/", 0, &unix.MountAttr{Attr_set: unix.MOUNT_ATTR_RDONLY})); err != nil {
		return err
	}
	_ = unix.Sethostname([]byte("vesper-unfs"))
	for i := 0; i < len(cc.Sources)+len(cc.Nests)+1; i++ { // the server gets no descriptor of a share, nor of its list's tree
		unix.CloseOnExec(firstFd + i)
	}
	if cc.Squash {
		err := dropToSquash()
		if err != nil {
			return step("drop to squash user", err)
		}
	} else if err := dropToRootCaps(); err != nil {
		return step("drop capabilities", err)
	}
	if err := installFilter(); err != nil { // after no_new_privs, which both drops set
		return err
	}
	if cc.Check != "" {
		mode := "root"
		if cc.Squash {
			mode = "squash"
		}
		return unix.Exec(cc.Check, []string{"vesper-unfs", "selfcheck", mode, cc.Check}, []string{"PATH=/usr/sbin:/usr/bin"})
	}
	unfsd := cc.Unfsd
	if unfsd == "" {
		unfsd = "/usr/sbin/unfsd"
	}
	return unix.Exec(unfsd, append([]string{"unfsd"}, UnfsdArgs("/etc/exports")...), []string{"PATH=/usr/sbin:/usr/bin"})
}

func attachPath(src, dst string, set uint64) error {
	to, err := unix.Open(dst, unix.O_PATH|unix.O_NOFOLLOW|unix.O_CLOEXEC, 0)
	if err != nil {
		return err
	}
	defer unix.Close(to)
	return attachTree(unix.AT_FDCWD, src, 0, to, set)
}

func attachFd(tree int, dst string, file bool) error {
	flags := unix.O_PATH | unix.O_DIRECTORY | unix.O_NOFOLLOW | unix.O_CLOEXEC
	if file {
		flags = unix.O_PATH | unix.O_NOFOLLOW | unix.O_CLOEXEC
	}
	to, err := unix.Open(dst, flags, 0)
	if err != nil {
		return err
	}
	defer unix.Close(to)
	return moveTree(tree, to)
}

// moveTree attaches a detached mount tree on the object the descriptor to refers to.
func moveTree(tree, to int) error {
	if err := unix.MoveMount(tree, "", to, "", unix.MOVE_MOUNT_F_EMPTY_PATH|unix.MOVE_MOUNT_T_EMPTY_PATH); err != nil {
		return fmt.Errorf("move_mount: %w", err)
	}
	return nil
}

// attachTree clones the mount at (dirfd, path), sets its attributes before anyone can see it, and attaches it on
// the object the descriptor to refers to.
func attachTree(dirfd int, path string, flags uint, to int, set uint64) error {
	t, err := unix.OpenTree(dirfd, path, unix.OPEN_TREE_CLONE|unix.OPEN_TREE_CLOEXEC|flags)
	if err != nil {
		return fmt.Errorf("open_tree: %w", err)
	}
	defer unix.Close(t)
	if err := unix.MountSetattr(t, "", unix.AT_EMPTY_PATH, &unix.MountAttr{Attr_set: set}); err != nil {
		return fmt.Errorf("mount_setattr: %w", err)
	}
	if err := unix.MoveMount(t, "", to, "", unix.MOVE_MOUNT_F_EMPTY_PATH|unix.MOVE_MOUNT_T_EMPTY_PATH); err != nil {
		return fmt.Errorf("move_mount: %w", err)
	}
	return nil
}

// capLast is the kernel's highest capability, read in Init before pivot_root takes /proc away.
var capLast = 63

func readCapLast() {
	if b, err := os.ReadFile("/proc/sys/kernel/cap_last_cap"); err == nil {
		if n, err := strconv.Atoi(strings.TrimSpace(string(b))); err == nil {
			capLast = n
		}
	}
}

// dropBounding drops every capability not in keep from the bounding set.
func dropBounding(keep uint64) error {
	for c := 0; c <= capLast; c++ {
		if keep&(1<<uint(c)) != 0 {
			continue
		}
		if err := unix.Prctl(unix.PR_CAPBSET_DROP, uintptr(c), 0, 0, 0); err != nil && !errors.Is(err, unix.EINVAL) {
			return fmt.Errorf("drop cap %d: %w", c, err)
		}
	}
	return nil
}

// dropToRootCaps keeps uid 0 with only RootCaps, no ambient or inheritable capabilities, and no_new_privs.
func dropToRootCaps() error {
	if err := dropBounding(RootCaps); err != nil {
		return err
	}
	if err := unix.Prctl(unix.PR_CAP_AMBIENT, unix.PR_CAP_AMBIENT_CLEAR_ALL, 0, 0, 0); err != nil {
		return err
	}
	hdr := unix.CapUserHeader{Version: unix.LINUX_CAPABILITY_VERSION_3}
	var data [2]unix.CapUserData
	data[0].Effective, data[0].Permitted = uint32(RootCaps), uint32(RootCaps)
	if err := unix.Capset(&hdr, &data[0]); err != nil {
		return fmt.Errorf("capset: %w", err)
	}
	return unix.Prctl(unix.PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0)
}

// dropToSquash becomes SquashID with no supplementary groups, no capabilities and no_new_privs.
func dropToSquash() error {
	if err := dropBounding(0); err != nil {
		return err
	}
	if err := unix.Prctl(unix.PR_CAP_AMBIENT, unix.PR_CAP_AMBIENT_CLEAR_ALL, 0, 0, 0); err != nil {
		return err
	}
	if err := syscall.Setgroups(nil); err != nil {
		return err
	}
	if err := syscall.Setresgid(SquashID, SquashID, SquashID); err != nil {
		return err
	}
	if err := syscall.Setresuid(SquashID, SquashID, SquashID); err != nil {
		return err
	}
	return unix.Prctl(unix.PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0)
}

// GuardSources returns the read-write sources whose path, or a directory above it up to the share mount, is an
// object in prior, the record of what earlier starts of this server knew as read-only. Sources the configured paths
// put inside a read-only directory source are left out (MovedUnder). failed holds the sources whose path could not
// be walked now, with the error: OpenSources waits for a path that is not there yet, and what it then opens was not
// compared.
func GuardSources(mounts []string, srcs []Source, prior []ProtectID) (hit []int, failed map[int]error) {
	failed = map[int]error{}
	if len(prior) == 0 {
		return nil, failed
	}
	known := knownIDs(prior)
	for _, i := range MovedUnder(srcs) {
		s := srcs[i]
		if s.Mount < 0 || s.Mount >= len(mounts) || s.Rel == "." {
			continue
		}
		mfd, err := unix.Open(mounts[s.Mount], unix.O_PATH|unix.O_DIRECTORY|unix.O_NOFOLLOW|unix.O_CLOEXEC, 0)
		if err != nil {
			continue // OpenSources refuses the start
		}
		under, err := underKnown(mfd, s.Rel, known)
		unix.Close(mfd)
		switch {
		case err != nil:
			failed[i] = err
		case under:
			hit = append(hit, i)
		}
	}
	return hit, failed
}

func knownIDs(ids []ProtectID) map[fileID]bool {
	known := map[fileID]bool{}
	for _, p := range ids {
		known[fileID{p.Dev, p.Ino}] = true
	}
	return known
}

// underKnown reports whether rel beneath the share mount mfd, or a directory on the way to it, is an object in
// known. Every step is opened beneath the mount with no symbolic link. A step that cannot be opened or looked at is
// an error, and the caller serves read-only what it could not compare.
func underKnown(mfd int, rel string, known map[fileID]bool) (bool, error) {
	parts := strings.Split(rel, "/")
	for n := 1; n <= len(parts); n++ {
		fd, err := unix.Openat2(mfd, strings.Join(parts[:n], "/"), &unix.OpenHow{
			Flags:   unix.O_PATH | unix.O_CLOEXEC,
			Resolve: unix.RESOLVE_BENEATH | unix.RESOLVE_NO_SYMLINKS | unix.RESOLVE_NO_MAGICLINKS | unix.RESOLVE_NO_XDEV,
		})
		if err != nil {
			return false, err
		}
		id, err := idOf(fd)
		unix.Close(fd)
		if err != nil {
			return false, err
		}
		if known[id] {
			return true, nil
		}
	}
	return false, nil
}

// tailBuffer keeps the last bytes written to it.
type tailBuffer struct {
	mu   sync.Mutex
	buf  []byte
	stop bool
	list bool
}

const tailKeep = 1024

func (t *tailBuffer) Write(p []byte) (int, error) {
	t.mu.Lock()
	defer t.mu.Unlock()
	t.buf = append(t.buf, p...)
	if len(t.buf) > tailKeep {
		t.buf = append([]byte(nil), t.buf[len(t.buf)-tailKeep:]...)
	}
	return len(p), nil
}

// told records that the supervisor was sent a signal to stop the server and passed it on.
func (t *tailBuffer) told() {
	t.mu.Lock()
	t.stop = true
	t.mu.Unlock()
}

// forList records that the supervisor ended the server itself, to have its list written anew (TD-078).
func (t *tailBuffer) forList() {
	t.mu.Lock()
	t.list = true
	t.mu.Unlock()
}

func (t *tailBuffer) reset() {
	t.mu.Lock()
	t.buf, t.stop, t.list = nil, false, false
	t.mu.Unlock()
}

var lastWords tailBuffer

// EndedForList reports whether the supervisor ended the server of the last Run itself, to have its list written
// anew: it could not tell the server of a file, or the list had grown too long (TD-078).
func EndedForList() bool {
	lastWords.mu.Lock()
	defer lastWords.mu.Unlock()
	return lastWords.list
}

// LastWords is the end of what the sandbox and the server wrote in the last Run, on one line and at most max bytes
// (the end of it, where a fault is reported), and whether the supervisor was told to stop the server. An end it was
// not told to make, and did not make for the list (EndedForList), is the server's own.
func LastWords(max int) (words string, told bool) {
	lastWords.mu.Lock()
	defer lastWords.mu.Unlock()
	words = strings.Join(strings.Fields(string(lastWords.buf)), " ")
	if len(words) > max {
		words = words[len(words)-max:]
	}
	return words, lastWords.stop
}
