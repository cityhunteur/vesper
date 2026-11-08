// Package manager implements the shim v2 process manager: containerd invokes the binary with `start`
// to spawn the long-lived shim daemon and with `delete` to clean up after a dead shim.
package manager

import (
	"context"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"syscall"
	"time"

	"github.com/cityhunteur/vesper/shim/internal/backend"
	"github.com/cityhunteur/vesper/shim/internal/task"
	"github.com/containerd/containerd/api/types"
	"github.com/containerd/containerd/v2/core/mount"
	"github.com/containerd/containerd/v2/pkg/namespaces"
	"github.com/containerd/containerd/v2/pkg/shim"
	"github.com/containerd/errdefs"
	"github.com/containerd/log"
)

// RuntimeName is the containerd runtime name; containerd derives the binary name from it.
const RuntimeName = "io.containerd.vesper.v2"

// Version is stamped at build time.
var Version = "dev"

type manager struct {
	name string
}

// New returns the shim manager.
func New(name string) shim.Manager {
	return manager{name: name}
}

func (m manager) Name() string { return m.name }

func newCommand(ctx context.Context, id, containerdAddress string, debug bool) (*exec.Cmd, error) {
	ns, err := namespaces.NamespaceRequired(ctx)
	if err != nil {
		return nil, err
	}
	self, err := os.Executable()
	if err != nil {
		return nil, err
	}
	cwd, err := os.Getwd()
	if err != nil {
		return nil, err
	}
	args := []string{"-namespace", ns, "-id", id, "-address", containerdAddress}
	if debug {
		args = append(args, "-debug")
	}
	cmd := exec.Command(self, args...)
	cmd.Dir = cwd
	cmd.Env = append(os.Environ(), "GOMAXPROCS=2")
	cmd.SysProcAttr = &syscall.SysProcAttr{Setpgid: true}
	return cmd, nil
}

// Start spawns the shim daemon with the listening socket inherited as fd 3 (the containerd convention).
func (m manager) Start(ctx context.Context, id string, opts shim.StartOpts) (_ shim.BootstrapParams, retErr error) {
	params := shim.BootstrapParams{Version: 3, Protocol: "ttrpc"}
	cmd, err := newCommand(ctx, id, opts.Address, opts.Debug)
	if err != nil {
		return params, err
	}
	address, err := shim.SocketAddress(ctx, opts.Address, id, false)
	if err != nil {
		return params, err
	}
	socket, err := shim.NewSocket(address)
	if err != nil {
		if !shim.SocketEaddrinuse(err) {
			return params, fmt.Errorf("create shim socket: %w", err)
		}
		if shim.CanConnect(address) {
			params.Address = address
			return params, nil
		}
		if err := shim.RemoveSocket(address); err != nil {
			return params, fmt.Errorf("remove stale shim socket: %w", err)
		}
		if socket, err = shim.NewSocket(address); err != nil {
			return params, fmt.Errorf("create shim socket (retry): %w", err)
		}
	}
	defer func() {
		if retErr != nil {
			socket.Close()
			_ = shim.RemoveSocket(address)
		}
	}()
	f, err := socket.File()
	if err != nil {
		return params, err
	}
	defer f.Close()
	cmd.ExtraFiles = append(cmd.ExtraFiles, f)
	if err := cmd.Start(); err != nil {
		return params, err
	}
	defer func() {
		if retErr != nil {
			_ = cmd.Process.Kill()
		}
	}()
	go cmd.Wait()
	if err := shim.AdjustOOMScore(cmd.Process.Pid); err != nil {
		return params, fmt.Errorf("adjust OOM score: %w", err)
	}
	params.Address = address
	return params, nil
}

// Stop is containerd's `delete` after the shim died: tear down the microVM and the bundle mounts.
func (m manager) Stop(ctx context.Context, id string) (shim.StopStatus, error) {
	cwd, err := os.Getwd()
	if err != nil {
		return shim.StopStatus{}, err
	}
	bundle := filepath.Join(filepath.Dir(cwd), id)
	if _, err := namespaces.NamespaceRequired(ctx); err != nil {
		return shim.StopStatus{}, err
	}
	// Reconcile from the durable record: a task that had already exited keeps its real exit status; a
	// task that was still running when the shim died is reported as killed. Either way the microVM, the
	// jail and the drive are removed so nothing is orphaned.
	status := shim.StopStatus{ExitedAt: time.Now(), ExitStatus: 128 + int(syscall.SIGKILL)}
	if st, err := task.ReadState(bundle); err == nil {
		status.Pid = st.Pid
		if st.Phase == "stopped" || st.Phase == "deleted" {
			status.ExitStatus = int(st.ExitStatus)
			if !st.ExitedAt.IsZero() {
				status.ExitedAt = st.ExitedAt
			}
		}
		log.G(ctx).WithFields(log.Fields{"id": id, "phase": st.Phase, "pid": st.Pid}).Info("reconciling task after shim exit")
	} else if !os.IsNotExist(err) {
		log.G(ctx).WithError(err).Warn("read task state during shim delete")
	}
	b := backend.NewMicroVM()
	if err := b.Cleanup(ctx, id); err != nil {
		log.G(ctx).WithError(err).Warn("microVM cleanup during shim delete")
	}
	if err := mount.UnmountRecursive(filepath.Join(bundle, "rootfs"), 0); err != nil {
		log.G(ctx).WithError(err).Warn("rootfs unmount during shim delete")
	}
	_ = os.Remove(filepath.Join(bundle, backend.RootfsImageName))
	return status, nil
}

func (m manager) Info(ctx context.Context, optionsR io.Reader) (*types.RuntimeInfo, error) {
	if optionsR != nil {
		_, _ = io.ReadAll(optionsR)
	}
	return &types.RuntimeInfo{
		Name:    m.name,
		Version: &types.RuntimeVersion{Version: Version},
	}, nil
}

var _ = errdefs.ErrNotFound
