//go:build linux

package unfs

import (
	"encoding/binary"
	"testing"

	"golang.org/x/sys/unix"
)

// run interprets the subset of classic BPF the filter uses against one seccomp_data.
func run(t *testing.T, prog []unix.SockFilter, arch, nr uint32, arg0 uint64) uint32 {
	t.Helper()
	data := make([]byte, 64)
	binary.LittleEndian.PutUint32(data[offNr:], nr)
	binary.LittleEndian.PutUint32(data[offArch:], arch)
	binary.LittleEndian.PutUint64(data[offArg0:], arg0)
	var acc uint32
	for pc := 0; pc < len(prog); pc++ {
		in := prog[pc]
		switch in.Code {
		case unix.BPF_LD | unix.BPF_W | unix.BPF_ABS:
			acc = binary.LittleEndian.Uint32(data[in.K:])
		case unix.BPF_JMP | unix.BPF_JEQ | unix.BPF_K:
			if acc == in.K {
				pc += int(in.Jt)
			} else {
				pc += int(in.Jf)
			}
		case unix.BPF_JMP | unix.BPF_JSET | unix.BPF_K:
			if acc&in.K != 0 {
				pc += int(in.Jt)
			} else {
				pc += int(in.Jf)
			}
		case unix.BPF_RET | unix.BPF_K:
			return in.K
		default:
			t.Fatalf("unexpected instruction %#x at %d", in.Code, pc)
		}
	}
	t.Fatal("fell off the end of the program")
	return 0
}

type filterCase struct {
	name string
	arch uint32
	nr   uint32
	arg0 uint64
	want uint32
}

func TestFilter(t *testing.T) {
	arch, err := auditArch()
	if err != nil {
		t.Skip(err)
	}
	prog := filter(arch)
	eperm := uint32(seccompRetErrno | uint32(unix.EPERM))
	enosys := uint32(seccompRetErrno | uint32(unix.ENOSYS))
	cases := []filterCase{
		{"unshare user", arch, unix.SYS_UNSHARE, unix.CLONE_NEWUSER, eperm},
		{"unshare user and mount", arch, unix.SYS_UNSHARE, unix.CLONE_NEWUSER | unix.CLONE_NEWNS, eperm},
		{"unshare mount", arch, unix.SYS_UNSHARE, unix.CLONE_NEWNS, seccompRetAllow},
		{"clone user", arch, unix.SYS_CLONE, unix.CLONE_NEWUSER | uint64(unix.SIGCHLD), eperm},
		{"clone thread", arch, unix.SYS_CLONE, unix.CLONE_VM | unix.CLONE_THREAD, seccompRetAllow},
		{"clone3", arch, unix.SYS_CLONE3, 0, enosys},
		{"setns", arch, unix.SYS_SETNS, 0, eperm},
		{"read", arch, unix.SYS_READ, unix.CLONE_NEWUSER, seccompRetAllow},
		{"other arch", 0x40000003, unix.SYS_READ, 0, eperm},
	}
	if arch == auditArchX86_64 {
		cases = append(cases,
			filterCase{"x32 unshare", arch, x32SyscallBit | uint32(unix.SYS_UNSHARE), unix.CLONE_NEWUSER, eperm},
			filterCase{"x32 read", arch, x32SyscallBit | uint32(unix.SYS_READ), 0, eperm})
	}
	for _, c := range cases {
		if got := run(t, prog, c.arch, uint32(c.nr), c.arg0); got != c.want {
			t.Errorf("%s: %#x, want %#x", c.name, got, c.want)
		}
	}
}
