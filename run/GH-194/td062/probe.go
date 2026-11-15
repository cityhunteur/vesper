package main

import (
	"fmt"
	"os"
	"time"

	"golang.org/x/sys/unix"
)

func st(p string, flags int) string {
	var s unix.Statx_t
	if err := unix.Statx(unix.AT_FDCWD, p, flags, unix.STATX_BASIC_STATS, &s); err != nil {
		return "err:" + err.Error()
	}
	return fmt.Sprintf("%d:%d", s.Uid, s.Gid)
}

func main() {
	p := os.Args[1]
	t0 := time.Now()
	fmt.Printf("plain=%s ", st(p, 0))
	fmt.Printf("chmod=%v ", unix.Chmod(p, 0o755))
	fmt.Printf("force=%s ", st(p, unix.AT_STATX_FORCE_SYNC))
	fmt.Printf("plain2=%s ", st(p, 0))
	fmt.Printf("chmod2=%v elapsed=%s\n", unix.Chmod(p, 0o755), time.Since(t0))
}
