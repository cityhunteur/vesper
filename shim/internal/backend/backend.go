// Package backend maps task lifecycle operations onto Vesper microVM commands inside the System VM.
package backend

import (
	"context"
	"encoding/base64"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"
)

// RootfsImageName is the ext4 image built from the bundle rootfs, next to config.json.
const RootfsImageName = "rootfs.ext4"

const (
	microvmBin = "/usr/local/bin/vesper-microvm"
	logDir     = "/var/log"
	minSizeKiB = 16 * 1024
)

// Backend is what the task service needs from the runtime; MicroVM is the real one and tests use a fake.
type Backend interface {
	// Prepare builds the root drive image for the task from the mounted bundle rootfs.
	Prepare(ctx context.Context, bundle string) (image string, err error)
	// Launch boots the microVM with the image and the encoded command; returns the VMM pid.
	Launch(ctx context.Context, id, image, cmdB64 string) (pid int, err error)
	// Stop terminates the microVM (SIGTERM, then SIGKILL after a grace period).
	Stop(ctx context.Context, id string) error
	// Cleanup removes the jail, the cgroup and any process; it is safe to call repeatedly.
	Cleanup(ctx context.Context, id string) error
	// LogPath is the microVM console log that carries the workload markers.
	LogPath(id string) string
	// Alive reports whether the VMM process still runs.
	Alive(id string) bool
}

// MicroVM shells out to vesper-microvm and mke2fs.
type MicroVM struct{}

// NewMicroVM returns the production backend.
func NewMicroVM() *MicroVM { return &MicroVM{} }

var pidRe = regexp.MustCompile(`pid=(\d+)`)

func (m *MicroVM) Prepare(ctx context.Context, bundle string) (string, error) {
	rootfs := filepath.Join(bundle, "rootfs")
	image := filepath.Join(bundle, RootfsImageName)
	used, err := diskUsageKiB(rootfs)
	if err != nil {
		return "", fmt.Errorf("size rootfs: %w", err)
	}
	size := used + used/5 + 4096
	if size < minSizeKiB {
		size = minSizeKiB
	}
	_ = os.Remove(image)
	cmd := exec.CommandContext(ctx, "mke2fs", "-q", "-F", "-t", "ext4", "-d", rootfs, "-L", "vesper-task", "-E", "root_owner=0:0", image, strconv.FormatInt(size, 10)+"K")
	if out, err := cmd.CombinedOutput(); err != nil {
		return "", fmt.Errorf("mke2fs: %w: %s", err, strings.TrimSpace(string(out)))
	}
	return image, nil
}

func (m *MicroVM) Launch(ctx context.Context, id, image, cmdB64 string) (int, error) {
	out, err := exec.CommandContext(ctx, microvmBin, "launch", id, "--rootfs", image, "--cmd", cmdB64).CombinedOutput()
	first := strings.SplitN(strings.TrimSpace(string(out)), "\n", 2)[0]
	if err != nil || !strings.HasPrefix(first, "LAUNCHED ") {
		return 0, fmt.Errorf("vesper-microvm launch %s: %v: %s", id, err, first)
	}
	// A workload that exits before the launch returns leaves `pid=` empty: not an error, the watcher
	// reads its exit marker.
	match := pidRe.FindStringSubmatch(first)
	if match == nil {
		return 0, nil
	}
	pid, _ := strconv.Atoi(match[1])
	return pid, nil
}

func (m *MicroVM) Stop(ctx context.Context, id string) error {
	out, err := exec.CommandContext(ctx, microvmBin, "stop", id).CombinedOutput()
	if err != nil {
		return fmt.Errorf("vesper-microvm stop %s: %w: %s", id, err, strings.TrimSpace(string(out)))
	}
	return nil
}

func (m *MicroVM) Cleanup(ctx context.Context, id string) error {
	out, err := exec.CommandContext(ctx, microvmBin, "cleanup", id).CombinedOutput()
	first := strings.SplitN(strings.TrimSpace(string(out)), "\n", 2)[0]
	if err != nil || !strings.HasPrefix(first, "CLEAN") {
		return fmt.Errorf("vesper-microvm cleanup %s: %v: %s", id, err, first)
	}
	return nil
}

func (m *MicroVM) LogPath(id string) string {
	return filepath.Join(logDir, "vesper-microvm-"+id+".log")
}

func (m *MicroVM) Alive(id string) bool {
	out, err := exec.Command(microvmBin, "status", id).Output()
	return err == nil && strings.HasPrefix(string(out), "RUNNING ")
}

// EncodeCommand turns an OCI process (args, env, cwd) into the base64 shell command the microVM init
// runs inside the rootfs: `cd CWD; export ENV…; exec ARGS` with every word shell-quoted.
func EncodeCommand(args, env []string, cwd string) (string, error) {
	if len(args) == 0 {
		return "", fmt.Errorf("process has no args")
	}
	var b strings.Builder
	if cwd != "" && cwd != "/" {
		b.WriteString("cd " + quote(cwd) + " || exit 126; ")
	}
	for _, kv := range env {
		k, v, ok := strings.Cut(kv, "=")
		if !ok || k == "" {
			continue
		}
		b.WriteString("export " + k + "=" + quote(v) + "; ")
	}
	b.WriteString("exec")
	for _, a := range args {
		b.WriteString(" " + quote(a))
	}
	return base64.StdEncoding.EncodeToString([]byte(b.String())), nil
}

func quote(s string) string {
	return "'" + strings.ReplaceAll(s, "'", `'\''`) + "'"
}

func diskUsageKiB(root string) (int64, error) {
	var total int64
	err := filepath.Walk(root, func(_ string, info os.FileInfo, err error) error {
		if err != nil {
			return err
		}
		total += (info.Size() + 4095) / 4096 * 4
		return nil
	})
	return total, err
}
