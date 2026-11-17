//go:build linux

package shares

import "golang.org/x/sys/unix"

// ShareMtime is the macOS mtime of rel beneath the share mount, rounded up to a whole second, or 0. It opens rel
// beneath the mount with no symbolic link on the way and no other mount, so a path the sink passes cannot make the
// fanout read the time of a file outside the share.
func ShareMtime(mount, rel string) int64 {
	mfd, err := unix.Open(mount, unix.O_PATH|unix.O_DIRECTORY|unix.O_NOFOLLOW|unix.O_CLOEXEC, 0)
	if err != nil {
		return 0
	}
	defer unix.Close(mfd)
	fd, err := unix.Openat2(mfd, rel, &unix.OpenHow{
		Flags:   unix.O_PATH | unix.O_NOFOLLOW | unix.O_CLOEXEC,
		Resolve: unix.RESOLVE_BENEATH | unix.RESOLVE_NO_SYMLINKS | unix.RESOLVE_NO_MAGICLINKS | unix.RESOLVE_NO_XDEV,
	})
	if err != nil {
		return 0
	}
	defer unix.Close(fd)
	var st unix.Stat_t
	if unix.Fstat(fd, &st) != nil {
		return 0
	}
	return RoundUp(st.Mtim.Sec, st.Mtim.Nsec)
}
