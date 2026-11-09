// Package backend maps task lifecycle operations onto Vesper microVM commands inside the System VM.
package backend

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"
	"time"

	"github.com/cityhunteur/vesper/shim/internal/bundle"
	"github.com/containerd/containerd/v2/core/mount"
)

// RootfsImageName is the ext4 image built from the bundle rootfs, next to config.json.
const RootfsImageName = "rootfs.ext4"

const (
	microvmBin = "/usr/local/bin/vesper-microvm"
	logDir     = "/var/log"
	minSizeKiB = 16 * 1024
	// CacheDir holds one ext4 per image layer chain (block-first strategy, GH #24).
	CacheDir = "/var/lib/vesper/rootfs-cache"
)

// Backend is what the task service needs from the runtime; MicroVM is the real one and tests use a fake.
type Backend interface {
	// Prepare produces the task's private root drive image: from the layer-chain cache when the same
	// chain was built before, else by building it from the mounted bundle rootfs. The record describes
	// what happened.
	Prepare(ctx context.Context, task, bundleDir string, mounts []mount.Mount) (image string, meta bundle.Meta, err error)
	// Launch boots the microVM and its private guest control listener; returns the VMM pid.
	Launch(ctx context.Context, id, image string) (pid int, err error)
	OpenSession(ctx context.Context, id string, port uint32, config []byte, isExec bool, paths IO) (*Session, error)
	Abort(ctx context.Context, id string, port uint32) error
	Resize(ctx context.Context, id string, port, rows, cols uint32) error
	Signal(ctx context.Context, id string, port, signal uint32, all bool) error
	// Stop terminates the microVM (SIGTERM, then SIGKILL after a grace period).
	Stop(ctx context.Context, id string) error
	// Cleanup removes the jail, the cgroup and any process; it is safe to call repeatedly.
	Cleanup(ctx context.Context, id string) error
	// LogPath is the microVM boot and runtime diagnostic log.
	LogPath(id string) string
	// Alive reports whether the VMM process still runs.
	Alive(id string) bool
}

// MicroVM shells out to vesper-microvm and mke2fs.
type MicroVM struct {
	cacheDir string
}

// NewMicroVM returns the production backend.
func NewMicroVM() *MicroVM { return &MicroVM{cacheDir: CacheDir} }

var pidRe = regexp.MustCompile(`pid=(\d+)`)

func (m *MicroVM) Prepare(ctx context.Context, task, bundleDir string, mounts []mount.Mount) (string, bundle.Meta, error) {
	started := time.Now()
	rootfs := filepath.Join(bundleDir, "rootfs")
	image := filepath.Join(bundleDir, RootfsImageName)
	layers := bundle.Inspect(mounts)
	meta := bundle.Meta{Task: task, Image: image, Layers: layers, Strategy: "block-first"}
	_ = os.Remove(image)
	if layers.Key == "" {
		// Documented fallback: a pre-populated rootfs directory has no stable identity, so it is built
		// per task and never cached.
		meta.Fallback = true
		meta.Strategy = "direct"
		if err := mkfs(ctx, rootfs, image); err != nil {
			return "", meta, diagnose("build root drive from the bundle directory", err, "check free space in the bundle filesystem and that mke2fs (e2fsprogs) is installed")
		}
	} else {
		cached := filepath.Join(m.cacheDir, layers.Key+".ext4")
		meta.Cache = cached
		if err := os.MkdirAll(m.cacheDir, 0o700); err != nil {
			return "", meta, diagnose("create the rootfs cache directory", err, "the cache lives in "+m.cacheDir+"; check permissions and free space")
		}
		if ok, _ := cacheValid(cached); ok {
			meta.Cached = true
		} else {
			tmp := cached + ".tmp"
			_ = os.Remove(tmp)
			if err := mkfs(ctx, rootfs, tmp); err != nil {
				_ = os.Remove(tmp)
				return "", meta, diagnose("build the layer-chain root drive", err, "the image layers are mounted at "+rootfs+"; check free space in "+m.cacheDir)
			}
			sum, err := fileSha256(tmp)
			if err != nil {
				return "", meta, diagnose("digest the root drive", err, "")
			}
			if err := os.WriteFile(tmp+".sha256", []byte(sum), 0o644); err != nil {
				return "", meta, diagnose("record the root drive digest", err, "")
			}
			if err := os.Rename(tmp, cached); err != nil {
				return "", meta, diagnose("publish the cached root drive", err, "")
			}
			if err := os.Rename(tmp+".sha256", cached+".sha256"); err != nil {
				return "", meta, diagnose("publish the cached digest", err, "")
			}
		}
		if err := copyFile(cached, image); err != nil {
			return "", meta, diagnose("copy the cached root drive into the bundle", err, "check free space in the bundle filesystem")
		}
	}
	sum, err := fileSha256(image)
	if err != nil {
		return "", meta, diagnose("digest the task root drive", err, "")
	}
	st, _ := os.Stat(image)
	meta.Sha256 = sum
	if st != nil {
		meta.Bytes = st.Size()
	}
	meta.BuildMS = time.Since(started).Milliseconds()
	if err := bundle.WriteMeta(bundleDir, meta); err != nil {
		return "", meta, diagnose("write "+bundle.MetaName, err, "")
	}
	return image, meta, nil
}

// PrepareError is an actionable rootfs preparation failure.
type PrepareError struct {
	Step string
	Err  error
	Hint string
}

func (e *PrepareError) Error() string {
	if e.Hint == "" {
		return fmt.Sprintf("rootfs: %s: %v", e.Step, e.Err)
	}
	return fmt.Sprintf("rootfs: %s: %v (%s)", e.Step, e.Err, e.Hint)
}

func (e *PrepareError) Unwrap() error { return e.Err }

func diagnose(step string, err error, hint string) error {
	return &PrepareError{Step: step, Err: err, Hint: hint}
}

func mkfs(ctx context.Context, rootfs, image string) error {
	used, err := diskUsageKiB(rootfs)
	if err != nil {
		return fmt.Errorf("size rootfs: %w", err)
	}
	size := used + used/5 + 4096
	if size < minSizeKiB {
		size = minSizeKiB
	}
	cmd := exec.CommandContext(ctx, "mke2fs", "-q", "-F", "-t", "ext4", "-d", rootfs, "-L", "vesper-task", "-E", "root_owner=0:0", image, strconv.FormatInt(size, 10)+"K")
	if out, err := cmd.CombinedOutput(); err != nil {
		return fmt.Errorf("mke2fs: %w: %s", err, strings.TrimSpace(string(out)))
	}
	return nil
}

// cacheValid is true when the cached image exists and matches its recorded digest.
func cacheValid(path string) (bool, error) {
	want, err := os.ReadFile(path + ".sha256")
	if err != nil {
		return false, err
	}
	got, err := fileSha256(path)
	if err != nil {
		return false, err
	}
	return strings.TrimSpace(string(want)) == got, nil
}

func fileSha256(path string) (string, error) {
	f, err := os.Open(path)
	if err != nil {
		return "", err
	}
	defer f.Close()
	h := sha256.New()
	if _, err := io.Copy(h, f); err != nil {
		return "", err
	}
	return hex.EncodeToString(h.Sum(nil)), nil
}

func copyFile(src, dst string) error {
	in, err := os.Open(src)
	if err != nil {
		return err
	}
	defer in.Close()
	tmp := dst + ".tmp"
	out, err := os.OpenFile(tmp, os.O_CREATE|os.O_TRUNC|os.O_WRONLY, 0o600)
	if err != nil {
		return err
	}
	if _, err := io.Copy(out, in); err != nil {
		out.Close()
		_ = os.Remove(tmp)
		return err
	}
	if err := out.Close(); err != nil {
		return err
	}
	return os.Rename(tmp, dst)
}

func (m *MicroVM) Launch(ctx context.Context, id, image string) (int, error) {
	out, err := exec.CommandContext(ctx, microvmBin, "launch", id, "--rootfs", image, "--guest-exec").CombinedOutput()
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
