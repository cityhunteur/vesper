//go:build linux

package unfs

import (
	"fmt"
	"runtime"
	"unsafe"

	"golang.org/x/sys/unix"
)

// The server must not make a user namespace: in one it would hold every capability over a copy of its mount
// namespace (GH #160 M3). /proc is absent, so it could not map a uid and exec, but native code needs neither. This
// filter refuses unshare and clone with CLONE_NEWUSER and setns with EPERM, and clone3 with ENOSYS, whose flags sit
// behind a pointer a filter cannot read (libc then falls back to clone). Everything else is allowed.
const (
	auditArchAarch64 = 0xc00000b7
	auditArchX86_64  = 0xc000003e
	seccompRetAllow  = 0x7fff0000
	seccompRetErrno  = 0x00050000
	// struct seccomp_data offsets
	offNr   = 0
	offArch = 4
	offArg0 = 16
	// x32SyscallBit marks an x32 system call on x86-64.
	x32SyscallBit = 0x40000000
)

func auditArch() (uint32, error) {
	switch runtime.GOARCH {
	case "arm64":
		return auditArchAarch64, nil
	case "amd64":
		return auditArchX86_64, nil
	}
	return 0, fmt.Errorf("no seccomp filter for %s", runtime.GOARCH)
}

func stmt(code uint16, k uint32) unix.SockFilter { return unix.SockFilter{Code: code, K: k} }
func jump(code uint16, k uint32, jt, jf uint8) unix.SockFilter {
	return unix.SockFilter{Code: code, Jt: jt, Jf: jf, K: k}
}

// denied are calls an exploited unfsd, still root with six capabilities, could use against the System VM and that
// unfsd never makes: the kernel keyrings (root's are shared across the System VM; request_key starts a helper),
// io_uring, userfaultfd, perf events, BPF, tracing and other processes' memory, module and kexec loading, and the
// mount API (council SEC-3, TD-069). Most also need a capability the server lacks; the filter refuses them anyway.
var denied = []uintptr{
	unix.SYS_KEYCTL, unix.SYS_ADD_KEY, unix.SYS_REQUEST_KEY,
	unix.SYS_IO_URING_SETUP, unix.SYS_IO_URING_ENTER, unix.SYS_IO_URING_REGISTER,
	unix.SYS_USERFAULTFD, unix.SYS_PERF_EVENT_OPEN, unix.SYS_BPF,
	unix.SYS_PTRACE, unix.SYS_PROCESS_VM_READV, unix.SYS_PROCESS_VM_WRITEV,
	unix.SYS_INIT_MODULE, unix.SYS_FINIT_MODULE, unix.SYS_DELETE_MODULE, unix.SYS_KEXEC_LOAD,
	unix.SYS_MOUNT, unix.SYS_UMOUNT2, unix.SYS_PIVOT_ROOT, unix.SYS_OPEN_TREE, unix.SYS_MOVE_MOUNT,
	unix.SYS_FSOPEN, unix.SYS_FSCONFIG, unix.SYS_FSMOUNT, unix.SYS_FSPICK, unix.SYS_MOUNT_SETATTR,
}

// filter builds the BPF program.
func filter(arch uint32) []unix.SockFilter {
	const (
		ld   = unix.BPF_LD | unix.BPF_W | unix.BPF_ABS
		jeq  = unix.BPF_JMP | unix.BPF_JEQ | unix.BPF_K
		jset = unix.BPF_JMP | unix.BPF_JSET | unix.BPF_K
		ret  = unix.BPF_RET | unix.BPF_K
	)
	eperm := uint32(seccompRetErrno | uint32(unix.EPERM))
	enosys := uint32(seccompRetErrno | uint32(unix.ENOSYS))
	prog := []unix.SockFilter{
		stmt(ld, offArch),
		jump(jeq, arch, 1, 0),
		stmt(ret, eperm), // another architecture's calling convention: refuse everything
		stmt(ld, offNr),
	}
	if arch == auditArchX86_64 {
		// x32 calls share the x86-64 audit architecture and carry __X32_SYSCALL_BIT in the number; refuse them all.
		prog = append(prog, jump(jset, x32SyscallBit, 0, 1), stmt(ret, eperm))
	}
	for _, nr := range denied {
		prog = append(prog, jump(jeq, uint32(nr), 0, 1), stmt(ret, eperm))
	}
	return append(prog,
		jump(jeq, uint32(unix.SYS_CLONE3), 0, 1),
		stmt(ret, enosys),
		jump(jeq, uint32(unix.SYS_SETNS), 0, 1),
		stmt(ret, eperm),
		jump(jeq, uint32(unix.SYS_UNSHARE), 1, 0),
		jump(jeq, uint32(unix.SYS_CLONE), 0, 3),
		stmt(ld, offArg0), // the flags: low 32 bits of the first argument on both architectures
		jump(jset, unix.CLONE_NEWUSER, 0, 1),
		stmt(ret, eperm),
		stmt(ret, seccompRetAllow),
	)
}

// installFilter loads the filter for this thread (no_new_privs must already be set); exec keeps it.
func installFilter() error {
	arch, err := auditArch()
	if err != nil {
		return err
	}
	prog := filter(arch)
	fprog := unix.SockFprog{Len: uint16(len(prog)), Filter: &prog[0]}
	if err := unix.Prctl(unix.PR_SET_SECCOMP, unix.SECCOMP_MODE_FILTER, uintptr(unsafe.Pointer(&fprog)), 0, 0); err != nil {
		return fmt.Errorf("seccomp: %w", err)
	}
	return nil
}
