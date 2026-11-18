//go:build linux

package unfs

import (
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"runtime"
	"sort"
	"strings"
	"syscall"

	"golang.org/x/sys/unix"
)

// Selfcheck runs inside the sandbox with the server's own privileges (vesper-unfs run --selfcheck) and tries what
// an exploited server would: leave the root, mount, remount an export writable, write the read-only parts, and use
// a new user namespace to gain capabilities and escape a nested chroot. Every line is "SELF <name> ok|FAIL <detail>",
// then "SELF-SUMMARY passed=N failed=M". mode is "root" or "squash".
func Selfcheck(mode, self string, out io.Writer) bool {
	runtime.LockOSThread()
	pass, fail := 0, 0
	check := func(name string, ok bool, format string, a ...any) {
		res := "ok"
		if ok {
			pass++
		} else {
			res = "FAIL"
			fail++
		}
		fmt.Fprintf(out, "SELF %s %s %s\n", name, res, fmt.Sprintf(format, a...))
	}
	denied := func(err error) bool {
		return errors.Is(err, unix.EPERM) || errors.Is(err, unix.EACCES) || errors.Is(err, unix.EROFS)
	}

	// Who it is.
	hdr := unix.CapUserHeader{Version: unix.LINUX_CAPABILITY_VERSION_3}
	var data [2]unix.CapUserData
	_ = unix.Capget(&hdr, &data[0])
	eff := uint64(data[0].Effective) | uint64(data[1].Effective)<<32
	nnp, _ := unix.PrctlRetInt(unix.PR_GET_NO_NEW_PRIVS, 0, 0, 0, 0)
	wantEff, wantUID := uint64(RootCaps), 0
	if mode == "squash" {
		wantEff, wantUID = 0, SquashID
	}
	check("identity", os.Getuid() == wantUID && eff == wantEff && nnp == 1, "uid=%d caps=%#x no_new_privs=%d", os.Getuid(), eff, nnp)
	check("pid-1", os.Getpid() == 1, "pid=%d", os.Getpid())
	mode2, _ := unix.PrctlRetInt(unix.PR_GET_SECCOMP, 0, 0, 0, 0)
	check("seccomp filter", mode2 == unix.SECCOMP_MODE_FILTER, "mode=%d", mode2)
	err := unix.Unshare(unix.CLONE_NEWUSER)
	check("unshare user namespace", errors.Is(err, unix.EPERM), "%v (EPERM is the filter; EINVAL would be the kernel's thread check)", err)
	err = unix.Unshare(unix.CLONE_NEWNS)
	check("unshare mount namespace", denied(err), "%v", err)

	// What the root holds: the exports, /usr, /lib, /etc (exports and libtirpc tables), /dev/null; nothing else.
	ents, _ := os.ReadDir("/")
	var names []string
	for _, e := range ents {
		names = append(names, e.Name())
	}
	sort.Strings(names)
	check("root-contents", strings.Join(names, ",") == "dev,etc,lib,s,usr", "%v", names)
	for _, p := range []string{"/proc", "/sys", "/old", "/run", "/var", "/root", "/etc/shadow", "/etc/passwd", "/dev/vda", "/dev/kvm"} {
		_, err := os.Lstat(p)
		check("absent "+p, errors.Is(err, os.ErrNotExist), "%v", err)
	}
	etc, _ := os.ReadDir("/etc")
	var etcNames []string
	for _, e := range etc {
		etcNames = append(etcNames, e.Name())
	}
	// The exports, the list of objects kept read-only by number (TD-073) and libtirpc's three tables; nothing else.
	allowed := map[string]bool{"exports": true, "exports.protect": true, "netconfig": true, "protocols": true, "services": true}
	extra := 0
	for _, n := range etcNames {
		if !allowed[n] {
			extra++
		}
	}
	check("etc-contents", extra == 0 && len(etcNames) <= len(allowed), "%v", etcNames)

	// Writes to the read-only parts.
	for _, p := range []string{"/x", "/etc/exports", "/etc/exports.protect", "/usr/x", "/lib/x", "/s/1/x", "/s/2/x", "/s/0/conf/x"} {
		f, err := os.OpenFile(p, os.O_WRONLY|os.O_CREATE, 0o644)
		if f != nil {
			f.Close()
			os.Remove(p)
		}
		check("write "+p, denied(err), "%v", err)
	}
	// /dev/null is the System VM's own inode: it opens for writing, but its mode and owner cannot change. The
	// attempts set what is already there, so a failing check changes nothing.
	if st, err := os.Stat("/dev/null"); err != nil {
		check("stat /dev/null", false, "%v", err)
	} else {
		sys := st.Sys().(*syscall.Stat_t)
		err = unix.Fchmodat(unix.AT_FDCWD, "/dev/null", uint32(st.Mode().Perm()), 0)
		check("chmod /dev/null", denied(err), "%v", err)
		err = unix.Fchownat(unix.AT_FDCWD, "/dev/null", int(sys.Uid), int(sys.Gid), 0)
		check("chown /dev/null", denied(err), "%v", err)
		f, err := os.OpenFile("/dev/null", os.O_WRONLY, 0)
		if f != nil {
			f.Close()
		}
		check("write /dev/null (control)", err == nil, "%v", err)
	}
	// The writable export is writable (control for the checks above).
	if f, err := os.OpenFile("/s/0/.vesper-selfcheck", os.O_WRONLY|os.O_CREATE, 0o644); err == nil {
		f.Close()
		os.Remove("/s/0/.vesper-selfcheck")
		check("write /s/0 (control)", true, "")
	} else {
		check("write /s/0 (control)", false, "%v", err)
	}

	// Mounts and the root.
	err = unix.Mount("none", "/s/0", "tmpfs", 0, "")
	check("mount tmpfs", denied(err), "%v", err)
	err = unix.Mount("", "/s/1", "", unix.MS_REMOUNT|unix.MS_BIND, "")
	check("remount /s/1 rw", denied(err), "%v", err)
	err = unix.MountSetattr(unix.AT_FDCWD, "/s/1", 0, &unix.MountAttr{Attr_clr: unix.MOUNT_ATTR_RDONLY})
	check("mount_setattr /s/1 rw", denied(err), "%v", err)
	err = unix.Unmount("/s/0/conf", unix.MNT_DETACH)
	check("umount nested ro", denied(err), "%v", err)
	_, err = unix.OpenTree(unix.AT_FDCWD, "/s/1", unix.OPEN_TREE_CLONE)
	check("open_tree clone", denied(err), "%v", err)
	err = unix.Chroot("/s/0")
	check("chroot", denied(err), "%v", err)
	err = unix.PivotRoot("/s/0", "/s/0")
	check("pivot_root", denied(err) || errors.Is(err, unix.EINVAL), "%v", err)
	err = unix.Mknod("/s/0/.dev", unix.S_IFBLK|0o600, int(unix.Mkdev(253, 0)))
	if err == nil {
		os.Remove("/s/0/.dev")
	}
	check("mknod", denied(err), "%v", err)
	if mode == "squash" {
		err = syscall.Setuid(0)
		check("setuid 0", denied(err), "%v", err)
	}
	_, err = unix.Open("/s/0/../..", unix.O_PATH|unix.O_DIRECTORY, 0)
	check("dotdot from an export is the root", err == nil, "%v", err)

	// A new user namespace gives every capability over namespaces it owns. unshare cannot make one in a process
	// with threads (every Go process), so a child is started in a new user and mount namespace: its mount namespace
	// is a copy of this one, so a nested chroot can climb no higher than this root.
	if self != "" {
		cmd := exec.Command(self, "selfcheck-userns")
		cmd.Stdout, cmd.Stderr = out, out
		cmd.SysProcAttr = &syscall.SysProcAttr{Cloneflags: syscall.CLONE_NEWUSER | syscall.CLONE_NEWNS}
		err := cmd.Run()
		var ee *exec.ExitError
		switch {
		case err == nil:
			check("userns child refused", false, "a child started in a new user namespace (its checks all held)")
		case errors.As(err, &ee):
			check("userns child", false, "exit %d", ee.ExitCode())
		default:
			check("userns child refused", errors.Is(err, unix.EPERM) || errors.Is(err, unix.ENOSYS), "%v", err)
		}
	}
	fmt.Fprintf(out, "SELF-SUMMARY passed=%d failed=%d\n", pass, fail)
	return fail == 0
}

// SelfcheckUserns runs in a new user and mount namespace started by Selfcheck, with every capability over them, and
// tries to leave the root with a nested chroot and to make the read-only exports writable. "USERNS ..." lines; it
// returns false if any attempt succeeded.
func SelfcheckUserns(out io.Writer) bool {
	ok := true
	report := func(name string, good bool, format string, a ...any) {
		res := "ok"
		if !good {
			res, ok = "FAIL", false
		}
		fmt.Fprintf(out, "USERNS %s %s %s\n", name, res, fmt.Sprintf(format, a...))
	}
	hdr := unix.CapUserHeader{Version: unix.LINUX_CAPABILITY_VERSION_3}
	var data [2]unix.CapUserData
	_ = unix.Capget(&hdr, &data[0])
	fmt.Fprintf(out, "INFO userns child uid=%d caps=%#x\n", os.Getuid(), uint64(data[0].Effective)|uint64(data[1].Effective)<<32)
	_ = unix.Mount("", "/", "", unix.MS_REC|unix.MS_PRIVATE, "")
	escaped := "none"
	if err := unix.Chroot("/s/0"); err == nil {
		for range 32 {
			_ = unix.Chdir("..")
		}
		if err := unix.Chroot("."); err == nil {
			ents, _ := os.ReadDir("/")
			var got []string
			for _, e := range ents {
				got = append(got, e.Name())
			}
			sort.Strings(got)
			escaped = strings.Join(got, ",")
		}
	} else {
		escaped = "chroot refused: " + err.Error()
	}
	report("nested-chroot", escaped == "dev,etc,lib,s,usr" || strings.HasPrefix(escaped, "chroot refused"), "root after climbing: %s", escaped)
	_, err := os.Lstat("/proc")
	report("absent /proc", errors.Is(err, os.ErrNotExist), "%v", err)
	err = unix.Mount("", "/s/1", "", unix.MS_REMOUNT|unix.MS_BIND, "")
	report("remount /s/1 rw", err != nil, "%v", err)
	err = unix.MountSetattr(unix.AT_FDCWD, "/s/1", 0, &unix.MountAttr{Attr_clr: unix.MOUNT_ATTR_RDONLY})
	report("mount_setattr /s/1 rw", err != nil, "%v", err)
	err = unix.Unmount("/s/0/conf", unix.MNT_DETACH)
	if err == nil {
		// Taking the nested bind away in this copy of the namespace leaves the server's own namespace as it was;
		// what matters is that the read-only source is still read-only underneath.
		f, werr := os.OpenFile("/s/0/conf/x", os.O_WRONLY|os.O_CREATE, 0o644)
		if f != nil {
			f.Close()
			os.Remove("/s/0/conf/x")
		}
		report("write conf after umount in userns", werr != nil, "umount allowed; write: %v", werr)
	} else {
		report("umount nested ro", true, "%v", err)
	}
	f, err := os.OpenFile("/s/1/x", os.O_WRONLY|os.O_CREATE, 0o644)
	if f != nil {
		f.Close()
		os.Remove("/s/1/x")
	}
	report("write /s/1", err != nil, "%v", err)
	err = unix.Mount("proc", "/s/0", "proc", 0, "")
	if err == nil {
		_ = unix.Unmount("/s/0", unix.MNT_DETACH)
	}
	report("mount proc", err != nil, "%v", err)
	return ok
}
