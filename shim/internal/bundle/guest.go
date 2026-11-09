package bundle

import (
	"encoding/json"
	"fmt"
	"strings"

	specs "github.com/opencontainers/runtime-spec/specs-go"
)

// GuestProcess removes host security labels: the microVM is the isolation boundary (ADR 0006).
func GuestProcess(p *specs.Process) (*specs.Process, error) {
	if p == nil || len(p.Args) == 0 {
		return nil, fmt.Errorf("process.args is empty")
	}
	raw, err := json.Marshal(p)
	if err != nil {
		return nil, fmt.Errorf("encode process: %w", err)
	}
	var out specs.Process
	if err := json.Unmarshal(raw, &out); err != nil {
		return nil, fmt.Errorf("copy process: %w", err)
	}
	if out.Cwd == "" {
		out.Cwd = "/"
	}
	if !strings.HasPrefix(out.Cwd, "/") || strings.ContainsRune(out.Cwd, 0) {
		return nil, fmt.Errorf("process.cwd must be an absolute path without NUL")
	}
	for _, arg := range out.Args {
		if strings.ContainsRune(arg, 0) {
			return nil, fmt.Errorf("process.args contains NUL")
		}
	}
	for _, env := range out.Env {
		if !strings.Contains(env, "=") || strings.ContainsRune(env, 0) {
			return nil, fmt.Errorf("invalid process environment entry")
		}
	}
	if out.Terminal && out.ConsoleSize == nil {
		out.ConsoleSize = &specs.Box{Height: 24, Width: 80}
	}
	out.ApparmorProfile, out.SelinuxLabel = "", ""
	out.NoNewPrivileges = false
	// The pinned 6.12 kernel supports capabilities through CAP_CHECKPOINT_RESTORE.
	caps := strings.Fields("CAP_CHOWN CAP_DAC_OVERRIDE CAP_DAC_READ_SEARCH CAP_FOWNER CAP_FSETID CAP_KILL CAP_SETGID CAP_SETUID CAP_SETPCAP CAP_LINUX_IMMUTABLE CAP_NET_BIND_SERVICE CAP_NET_BROADCAST CAP_NET_ADMIN CAP_NET_RAW CAP_IPC_LOCK CAP_IPC_OWNER CAP_SYS_MODULE CAP_SYS_RAWIO CAP_SYS_CHROOT CAP_SYS_PTRACE CAP_SYS_PACCT CAP_SYS_ADMIN CAP_SYS_BOOT CAP_SYS_NICE CAP_SYS_RESOURCE CAP_SYS_TIME CAP_SYS_TTY_CONFIG CAP_MKNOD CAP_LEASE CAP_AUDIT_WRITE CAP_AUDIT_CONTROL CAP_SETFCAP CAP_MAC_OVERRIDE CAP_MAC_ADMIN CAP_SYSLOG CAP_WAKE_ALARM CAP_BLOCK_SUSPEND CAP_AUDIT_READ CAP_PERFMON CAP_BPF CAP_CHECKPOINT_RESTORE")
	out.Capabilities = &specs.LinuxCapabilities{Bounding: caps, Effective: caps, Permitted: caps, Inheritable: caps, Ambient: caps}
	return &out, nil
}

// GuestConfig is transported separately from the cached rootfs; the guest writes these bytes verbatim.
func GuestConfig(p *specs.Process) ([]byte, error) {
	process, err := GuestProcess(p)
	if err != nil {
		return nil, err
	}
	return json.Marshal(&specs.Spec{Version: specs.Version, Process: process, Root: &specs.Root{Path: "/newroot"}, Linux: &specs.Linux{Namespaces: []specs.LinuxNamespace{{Type: specs.NetworkNamespace, Path: "/proc/1/ns/net"}}}})
}
