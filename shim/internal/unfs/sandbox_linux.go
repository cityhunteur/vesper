//go:build linux

package unfs

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"os/signal"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"syscall"
	"time"

	"golang.org/x/sys/unix"
)

// Config is what `vesper-unfs run` is given by vesper-microvm.
type Config struct {
	Mount   string   // the trusted virtio-fs share mount the sources lie in
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

// OpenSources opens each source relative to the share mount, with no symbolic link on the way, beneath the mount
// and on it alone, and clones what it opened into detached mount trees with their final attributes: one per source,
// then one per nest (Nests order) of the read-only source it carries. Nothing opens a source path again after this
// (ADR 0009, Source binding). The trees are made here because open_tree clones only a mount of the caller's own
// mount namespace; the sandbox, in a new one, attaches them with move_mount.
func OpenSources(mount string, srcs []Source, anyFS bool) ([]*os.File, error) {
	if err := Check(srcs); err != nil {
		return nil, err
	}
	mfd, err := unix.Open(mount, unix.O_PATH|unix.O_DIRECTORY|unix.O_NOFOLLOW|unix.O_CLOEXEC, 0)
	if err != nil {
		return nil, fmt.Errorf("share mount %s: %w", mount, err)
	}
	defer unix.Close(mfd)
	var sfs unix.Statfs_t
	if err := unix.Fstatfs(mfd, &sfs); err != nil {
		return nil, fmt.Errorf("share mount %s: %w", mount, err)
	}
	if !anyFS && sfs.Type != unix.FUSE_SUPER_MAGIC { // virtio-fs reports the FUSE magic
		return nil, fmt.Errorf("share mount %s is not virtio-fs (type %#x)", mount, sfs.Type)
	}
	var dirs []int
	defer func() {
		for _, fd := range dirs {
			unix.Close(fd)
		}
	}()
	for _, s := range srcs {
		fd, err := unix.Openat2(mfd, s.Rel, &unix.OpenHow{
			Flags:   unix.O_PATH | unix.O_DIRECTORY | unix.O_CLOEXEC,
			Resolve: unix.RESOLVE_BENEATH | unix.RESOLVE_NO_SYMLINKS | unix.RESOLVE_NO_MAGICLINKS | unix.RESOLVE_NO_XDEV,
		})
		if err != nil {
			return nil, fmt.Errorf("source %q: %w", s.Rel, err)
		}
		dirs = append(dirs, fd)
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
	files, err := OpenSources(cfg.Mount, cfg.Sources, cfg.AnyFS)
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
	cc, _ := json.Marshal(childConfig{Sources: cfg.Sources, Nests: Nests(cfg.Sources), Squash: cfg.Squash, Root: cfg.Root, Unfsd: cfg.Unfsd, Check: check, PauseNestsMs: cfg.TestPauseNests.Milliseconds()})
	cmd := exec.Command("/proc/self/exe", "init")
	cmd.Env = []string{configEnv + "=" + string(cc)}
	cmd.Stdin, cmd.Stdout, cmd.Stderr = nil, os.Stdout, os.Stderr
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
	done := make(chan error, 1)
	go func() { done <- cmd.Wait() }()
	var kill <-chan time.Time
	for {
		select {
		case s := <-sigs:
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
// only the exports, a read-only /usr and /lib for unfsd, /dev/null and the exports file; moves into it with
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
	for i := range cc.Sources {
		dst := filepath.Join(root, ExportPath(i))
		if err := os.Mkdir(dst, 0o755); err != nil {
			return err
		}
		if err := step(fmt.Sprintf("attach source %d", i), attachFd(firstFd+i, dst)); err != nil {
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
		at, err := unix.Openat2(outer, n.Sub, &unix.OpenHow{
			Flags:   unix.O_PATH | unix.O_DIRECTORY | unix.O_CLOEXEC,
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
	for i := 0; i < len(cc.Sources)+len(cc.Nests); i++ { // the server gets no descriptor of a share
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

func attachFd(tree int, dst string) error {
	to, err := unix.Open(dst, unix.O_PATH|unix.O_DIRECTORY|unix.O_NOFOLLOW|unix.O_CLOEXEC, 0)
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
