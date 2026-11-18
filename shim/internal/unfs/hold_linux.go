package unfs

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"runtime"

	"golang.org/x/sys/unix"
)

// Holds keep the recorded read-only objects known by one inode number (TD-076, D-055).
//
// The server and the supervisor know an object that was bound read-only by its device and inode number. The System
// VM gives a file another number when it has forgotten the file and looks it up again (measured:
// run/TD-073/notes.txt 8), and it forgets a file that nothing holds. What a running server serves is held by the
// sandbox's mounts. An object that a task moved away from its source's path is not, and after a restart of the
// server it is known only by its number in the record. So each recorded object is also held by a read-only mount of
// its own, which stays when the supervisor and its sandbox end.
//
// The holds are mounts of the user's files, so they are not put where the System VM's other tools would find them
// (a search of /run, a list of the virtio-fs mounts). They live in a mount namespace of their own, the hold
// namespace, which no process is in. Its root is a small tmpfs and it has no other mount but the holds, each at
// /DEV-INO, so it keeps nothing else of the System VM alive. It is kept by a bind of its namespace file at
// HoldDir/ns, as a network namespace is; that file is all there is of it in the System VM's own namespace.
// Dropping the bind drops every hold at once: vesper-microvm does that when the task stops or is launched again
// (`vesper-unfs release`).

const nsfsMagic = 0x6e736673

var holdNameRE = regexp.MustCompile(`^[0-9]+-[0-9]+$`)

// HoldDir is where the hold namespace of the record at protectFile is kept; empty when there is no record.
func HoldDir(protectFile string) string {
	if protectFile == "" {
		return ""
	}
	return protectFile + ".d"
}

func holdName(id fileID) string { return fmt.Sprintf("%d-%d", id.dev, id.ino) }

// holdNS opens the hold namespace kept in dir, and makes it when create is set and it is not there. It returns -1
// and no error when there is none and create is not set.
func holdNS(dir string, create bool) (int, error) {
	nsfile := filepath.Join(dir, "ns")
	open := func() (int, error) {
		fd, err := unix.Open(nsfile, unix.O_RDONLY|unix.O_NOFOLLOW|unix.O_CLOEXEC, 0)
		if err != nil {
			return -1, err
		}
		var sfs unix.Statfs_t
		if err := unix.Fstatfs(fd, &sfs); err != nil || sfs.Type != nsfsMagic {
			unix.Close(fd)
			return -1, unix.ENOENT // the plain file a namespace is bound on, with none bound
		}
		return fd, nil
	}
	fd, err := open()
	if err == nil || !create {
		if errors.Is(err, unix.ENOENT) {
			err = nil
		}
		return fd, err
	}
	if !errors.Is(err, unix.ENOENT) {
		return -1, err
	}
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return -1, err
	}
	f, err := os.OpenFile(nsfile, os.O_CREATE|os.O_WRONLY|unix.O_NOFOLLOW, 0o600)
	if err != nil {
		return -1, err
	}
	f.Close()
	// A thread leaves this process's mount namespace for a new one, which the caller's thread binds at nsfile. The
	// thread is not used again: a goroutine that ends while locked to its thread takes the thread with it. The
	// namespace lives on through the bind.
	tid, made, bound := make(chan int, 1), make(chan error, 1), make(chan struct{})
	go func() {
		runtime.LockOSThread()
		made <- func() error {
			if err := unix.Unshare(unix.CLONE_NEWNS); err != nil {
				return fmt.Errorf("unshare a mount namespace: %w", err)
			}
			// Nothing in it reaches the other namespaces.
			if err := unix.Mount("", "/", "", unix.MS_REC|unix.MS_PRIVATE, ""); err != nil {
				return fmt.Errorf("make the hold namespace private: %w", err)
			}
			// A root of its own, and the copy of the System VM's mounts goes: the namespace keeps nothing but
			// what is mounted in it from now on. The tmpfs is mounted on the hold directory in this namespace
			// only.
			if err := unix.Mount("vesper-holds", dir, "tmpfs", unix.MS_NOSUID|unix.MS_NODEV|unix.MS_NOEXEC, "size=1m,mode=0700"); err != nil {
				return fmt.Errorf("the hold namespace's root: %w", err)
			}
			old := filepath.Join(dir, "old")
			if err := os.Mkdir(old, 0o700); err != nil {
				return err
			}
			if err := unix.PivotRoot(dir, old); err != nil {
				return fmt.Errorf("pivot_root: %w", err)
			}
			if err := unix.Chdir("/"); err != nil {
				return err
			}
			if err := unix.Unmount("/old", unix.MNT_DETACH); err != nil {
				return fmt.Errorf("drop the copied mounts: %w", err)
			}
			if err := os.Remove("/old"); err != nil {
				return err
			}
			tid <- unix.Gettid()
			return nil
		}()
		<-bound
	}()
	if err := <-made; err != nil {
		close(bound)
		os.Remove(nsfile)
		return -1, err
	}
	err = unix.Mount(fmt.Sprintf("/proc/%d/task/%d/ns/mnt", os.Getpid(), <-tid), nsfile, "", unix.MS_BIND, "")
	close(bound)
	if err != nil {
		return -1, fmt.Errorf("keep the hold namespace at %s: %w", nsfile, err)
	}
	return open()
}

// hold attaches a read-only clone of the object fd refers to (a descriptor in this mount namespace, not a detached
// tree) at its place in the hold namespace of dir, unless that object is held there already.
func hold(dir string, fd int, file bool) error {
	if dir == "" {
		return nil
	}
	id, err := idOf(fd)
	if err != nil {
		return err
	}
	ns, err := holdNS(dir, true)
	if err != nil {
		return err
	}
	defer unix.Close(ns)
	t, err := detach(fd, unix.MOUNT_ATTR_RDONLY|unix.MOUNT_ATTR_NOSUID|unix.MOUNT_ATTR_NODEV|unix.MOUNT_ATTR_NOEXEC)
	if err != nil {
		return err
	}
	defer unix.Close(t)
	at := "/" + holdName(id)
	return inMountNamespace(ns, func() error {
		if h, err := unix.Open(at, unix.O_PATH|unix.O_NOFOLLOW|unix.O_CLOEXEC, 0); err == nil {
			have, herr := idOf(h)
			unix.Close(h)
			if herr == nil && have == id {
				return nil // held already
			}
		} else if file { // the mount point, on the namespace's own root: an empty file or directory
			f, err := os.OpenFile(at, os.O_CREATE|os.O_WRONLY|unix.O_NOFOLLOW, 0o600)
			if err != nil {
				return err
			}
			f.Close()
		} else if err := os.Mkdir(at, 0o700); err != nil {
			return err
		}
		if err := unix.MoveMount(t, "", unix.AT_FDCWD, at, unix.MOVE_MOUNT_F_EMPTY_PATH); err != nil {
			return fmt.Errorf("move_mount to the hold namespace's %s: %w", at, err)
		}
		return nil
	})
}

// heldNames lists, on a thread that is in a hold namespace, the mount points in its root that carry a mount: the
// entries named DEV-INO that are on another file system than the root.
func heldNames() ([]string, error) {
	var root unix.Stat_t
	if err := unix.Lstat("/", &root); err != nil {
		return nil, err
	}
	entries, err := os.ReadDir("/")
	if err != nil {
		return nil, err
	}
	var out []string
	for _, e := range entries {
		var st unix.Stat_t
		if holdNameRE.MatchString(e.Name()) && unix.Lstat("/"+e.Name(), &st) == nil && st.Dev != root.Dev {
			out = append(out, e.Name())
		}
	}
	return out, nil
}

// CountHolds reports how many objects the hold namespace of dir holds.
func CountHolds(dir string) (int, error) {
	if dir == "" {
		return 0, nil
	}
	ns, err := holdNS(dir, false)
	if err != nil || ns < 0 {
		return 0, err
	}
	defer unix.Close(ns)
	n := 0
	err = inMountNamespace(ns, func() error {
		names, err := heldNames()
		n = len(names)
		return err
	})
	return n, err
}

// ReleaseHolds drops the holds of dir that keep does not name and reports how many. With keep nil it drops the hold
// namespace itself, and with it every hold, and removes dir; left then counts what it could not remove.
//
// In this mount namespace the only mount it takes off is the bind of the namespace file, and the only things it
// removes are that file, when it is a regular file of no length, and the directory, when it is empty. A single
// hold is taken off inside the hold namespace, where its mount point is an empty entry of that namespace's own
// root. No mount of a user's file is ever at a path that this function removes.
func ReleaseHolds(dir string, keep []ProtectID) (released, left int, err error) {
	if dir == "" {
		return 0, 0, nil
	}
	var st unix.Stat_t
	if lerr := unix.Lstat(dir, &st); errors.Is(lerr, unix.ENOENT) {
		return 0, 0, nil
	} else if lerr != nil {
		return 0, 1, lerr
	} else if st.Mode&unix.S_IFMT != unix.S_IFDIR {
		return 0, 1, fmt.Errorf("%s is not a directory", dir)
	}
	offAll := func(at string) error { // every mount at a place; nil when none is left there
		var uerr error
		for n := 0; n < 8 && uerr == nil; n++ {
			uerr = unix.Unmount(at, unix.MNT_DETACH|unix.UMOUNT_NOFOLLOW)
		}
		if errors.Is(uerr, unix.EINVAL) || errors.Is(uerr, unix.ENOENT) { // EINVAL: nothing is mounted there
			return nil
		}
		if uerr == nil {
			return errors.New("more mounts than expected")
		}
		return uerr
	}
	if keep != nil {
		ns, nerr := holdNS(dir, false)
		if nerr != nil || ns < 0 {
			return 0, 0, nerr
		}
		defer unix.Close(ns)
		kept := map[string]bool{}
		for _, id := range keep {
			kept[holdName(fileID{id.Dev, id.Ino})] = true
		}
		nerr = inMountNamespace(ns, func() error {
			names, err := heldNames()
			if err != nil {
				return err
			}
			var last error
			for _, name := range names {
				if kept[name] {
					continue
				}
				at := "/" + name
				if err := offAll(at); err != nil {
					left, last = left+1, fmt.Errorf("hold %s: unmount: %w", name, err)
					continue
				}
				// The mount point on the namespace's own root: an empty directory or an empty file.
				if rerr := unix.Rmdir(at); errors.Is(rerr, unix.ENOTDIR) {
					_ = unix.Unlink(at)
				}
				released++
			}
			return last
		})
		return released, left, nerr
	}
	nsfile := filepath.Join(dir, "ns")
	if n, cerr := CountHolds(dir); cerr == nil {
		released = n
	}
	if uerr := offAll(nsfile); uerr != nil {
		return 0, 1, fmt.Errorf("%s: unmount: %w", nsfile, uerr)
	}
	if lerr := unix.Lstat(nsfile, &st); lerr == nil {
		if st.Mode&unix.S_IFMT != unix.S_IFREG || st.Size != 0 {
			return released, 1, fmt.Errorf("%s is not the empty file a namespace is bound on", nsfile)
		}
		if rerr := unix.Unlink(nsfile); rerr != nil {
			return released, 1, fmt.Errorf("%s: %w", nsfile, rerr)
		}
	}
	if rerr := unix.Rmdir(dir); rerr != nil && !errors.Is(rerr, unix.ENOENT) {
		return released, 1, fmt.Errorf("%s: %w", dir, rerr)
	}
	return released, 0, nil
}
