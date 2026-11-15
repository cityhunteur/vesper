//go:build linux

package runcwrap

import (
	"fmt"
	"runtime"
	"time"

	"golang.org/x/sys/unix"
)

func onShare(path string) bool {
	var fs unix.Statfs_t
	return unix.Statfs(path, &fs) == nil && fs.Type == unix.FUSE_SUPER_MAGIC
}

// Refresh asks the share for the attributes of each path on a macOS share as t.UID and t.GID, so the System VM caches
// the owner those ids see. Paths on other filesystems are skipped. It gives up after timeout: a create must not wait
// on a share that does not answer.
func Refresh(t Target, timeout time.Duration) Result {
	done := make(chan Result, 1)
	go func() {
		// The thread keeps its changed fsuid and is never unlocked, so the runtime ends it with this goroutine.
		runtime.LockOSThread()
		var r Result
		var shared []string
		for _, p := range t.Paths {
			if onShare(p) {
				shared = append(shared, p)
			} else {
				r.Skipped++
			}
		}
		if len(shared) == 0 {
			done <- r
			return
		}
		// setfsuid and setfsgid change only this thread, and only the ids the kernel puts on filesystem requests.
		unix.Setfsgid(int(t.GID))
		unix.Setfsuid(int(t.UID))
		fsgid, _ := unix.SetfsgidRetGid(-1)
		fsuid, _ := unix.SetfsuidRetUid(-1)
		if fsuid != int(t.UID) || fsgid != int(t.GID) {
			r.Errors = append(r.Errors, fmt.Errorf("cannot ask as %d:%d (fs ids are %d:%d)", t.UID, t.GID, fsuid, fsgid))
			done <- r
			return
		}
		for _, p := range shared {
			var st unix.Statx_t
			err := unix.Statx(unix.AT_FDCWD, p, unix.AT_STATX_FORCE_SYNC, unix.STATX_UID|unix.STATX_GID, &st)
			switch {
			case err != nil:
				r.Errors = append(r.Errors, fmt.Errorf("%s: %w", p, err))
			case st.Uid != t.UID:
				r.Errors = append(r.Errors, fmt.Errorf("%s: the share answered owner %d, not %d", p, st.Uid, t.UID))
			default:
				r.Refreshed++
			}
		}
		done <- r
	}()
	select {
	case r := <-done:
		return r
	case <-time.After(timeout):
		return Result{Errors: []error{fmt.Errorf("no answer from the share within %s", timeout)}}
	}
}
