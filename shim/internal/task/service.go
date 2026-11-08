// Package task implements the containerd runtime v2 Task service for Vesper: one task per shim, each task
// a jailed Firecracker microVM booted from an ext4 image built out of the OCI bundle rootfs.
package task

import (
	"bufio"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/cityhunteur/vesper/shim/internal/backend"
	"github.com/cityhunteur/vesper/shim/internal/bundle"
	eventstypes "github.com/containerd/containerd/api/events"
	taskAPI "github.com/containerd/containerd/api/runtime/task/v3"
	"github.com/containerd/containerd/api/types"
	tasktypes "github.com/containerd/containerd/api/types/task"
	"github.com/containerd/containerd/v2/core/mount"
	"github.com/containerd/containerd/v2/core/runtime"
	"github.com/containerd/containerd/v2/pkg/protobuf"
	"github.com/containerd/containerd/v2/pkg/shim"
	"github.com/containerd/containerd/v2/pkg/shutdown"
	"github.com/containerd/errdefs"
	"github.com/containerd/errdefs/pkg/errgrpc"
	"github.com/containerd/log"
	"github.com/containerd/ttrpc"
	specs "github.com/opencontainers/runtime-spec/specs-go"
	"google.golang.org/protobuf/types/known/emptypb"
)

const (
	markerStart = "VESPER-WORKLOAD-START"
	markerExit  = "VESPER-WORKLOAD-EXIT "
	// exitKilled is reported when the VMM disappears without a workload exit marker (kill or crash).
	exitKilled = 137
	pollEvery  = 100 * time.Millisecond
)

// Service is the ttrpc Task service.
type Service struct {
	mu        sync.Mutex
	backend   backend.Backend
	publisher shim.Publisher
	shutdown  shutdown.Service
	task      *Task
}

// NewService wires the service; `b` is the production microVM backend unless a test injects a fake.
func NewService(ctx context.Context, publisher shim.Publisher, sd shutdown.Service, b backend.Backend) (*Service, error) {
	s := &Service{backend: b, publisher: publisher, shutdown: sd}
	if sd != nil {
		sd.RegisterCallback(func(context.Context) error {
			if publisher != nil {
				return publisher.Close()
			}
			return nil
		})
	}
	return s, nil
}

// RegisterTTRPC registers the Task service on the shim's ttrpc server.
func (s *Service) RegisterTTRPC(server *ttrpc.Server) error {
	taskAPI.RegisterTTRPCTaskService(server, s)
	return nil
}

func (s *Service) getTask(id string) (*Task, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.task == nil || s.task.ID != id || s.task.Phase() == PhaseDeleted {
		return nil, errgrpc.ToGRPCf(errdefs.ErrNotFound, "task %s", id)
	}
	return s.task, nil
}

func (s *Service) publish(ctx context.Context, topic string, event interface{}) {
	if s.publisher == nil {
		return
	}
	if ev, ok := event.(eventMessage); ok {
		if err := s.publisher.Publish(ctx, topic, ev); err != nil {
			log.G(ctx).WithError(err).WithField("topic", topic).Warn("publish event")
		}
	}
}

// Create mounts the bundle rootfs, builds the root drive image and records the process to run.
func (s *Service) Create(ctx context.Context, r *taskAPI.CreateTaskRequest) (_ *taskAPI.CreateTaskResponse, retErr error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.task != nil && s.task.Phase() != PhaseDeleted {
		return nil, errgrpc.ToGRPCf(errdefs.ErrAlreadyExists, "task %s already exists in this shim", s.task.ID)
	}
	if r.Terminal {
		return nil, errgrpc.ToGRPCf(errdefs.ErrNotImplemented, "terminal tasks are not supported yet (P3.2)")
	}
	if r.Checkpoint != "" || r.ParentCheckpoint != "" {
		return nil, errgrpc.ToGRPCf(errdefs.ErrNotImplemented, "checkpoint restore is not supported")
	}
	rootfs := filepath.Join(r.Bundle, "rootfs")
	if err := os.MkdirAll(rootfs, 0o711); err != nil {
		return nil, err
	}
	mounts := make([]mount.Mount, 0, len(r.Rootfs))
	for _, m := range r.Rootfs {
		mounts = append(mounts, mount.Mount{Type: m.Type, Source: m.Source, Target: m.Target, Options: m.Options})
	}
	spec, err := bundle.Validate(r.Bundle, mounts)
	if err != nil {
		return nil, errgrpc.ToGRPCf(errdefs.ErrInvalidArgument, "%v", err)
	}
	if len(mounts) > 0 {
		if err := mount.All(mounts, rootfs); err != nil {
			return nil, fmt.Errorf("mount rootfs: %w", err)
		}
	}
	defer func() {
		if len(mounts) > 0 {
			if err := mount.UnmountAll(rootfs, 0); err != nil {
				log.G(ctx).WithError(err).Warn("unmount rootfs after image build")
			}
		}
	}()
	image, meta, err := s.backend.Prepare(ctx, r.ID, r.Bundle, mounts)
	if err != nil {
		return nil, errgrpc.ToGRPCf(errdefs.ErrFailedPrecondition, "%v", err)
	}
	log.G(ctx).WithFields(log.Fields{"task": r.ID, "strategy": meta.Strategy, "cached": meta.Cached, "layers": len(meta.Layers.Lower), "build_ms": meta.BuildMS}).Info("root drive ready")
	cmdB64, err := backend.EncodeCommand(spec.Process.Args, spec.Process.Env, spec.Process.Cwd)
	if err != nil {
		return nil, errgrpc.ToGRPCf(errdefs.ErrInvalidArgument, "%v", err)
	}
	t := newTask(r.ID, r.Bundle)
	t.Image, t.CmdB64 = image, cmdB64
	t.Stdin, t.Stdout, t.Stderr, t.Terminal = r.Stdin, r.Stdout, r.Stderr, r.Terminal
	s.task = t
	s.publish(ctx, runtime.TaskCreateEventTopic, &eventstypes.TaskCreate{
		ContainerID: r.ID,
		Bundle:      r.Bundle,
		Rootfs:      r.Rootfs,
		IO:          &eventstypes.TaskIO{Stdin: r.Stdin, Stdout: r.Stdout, Stderr: r.Stderr, Terminal: r.Terminal},
		Pid:         uint32(os.Getpid()),
	})
	return &taskAPI.CreateTaskResponse{Pid: uint32(os.Getpid())}, nil
}

// Start boots the microVM and streams the workload output to the task's stdout until it exits.
func (s *Service) Start(ctx context.Context, r *taskAPI.StartRequest) (*taskAPI.StartResponse, error) {
	if r.ExecID != "" {
		return nil, errgrpc.ToGRPCf(errdefs.ErrNotImplemented, "exec is not supported yet (P3.2)")
	}
	t, err := s.getTask(r.ID)
	if err != nil {
		return nil, err
	}
	if t.Phase() != PhaseCreated {
		return nil, errgrpc.ToGRPCf(errdefs.ErrFailedPrecondition, "task %s is %s, not created", t.ID, t.Phase())
	}
	pid, err := s.backend.Launch(ctx, t.ID, t.Image, t.CmdB64)
	if err != nil {
		return nil, errgrpc.ToGRPCf(errdefs.ErrFailedPrecondition, "launch microVM: %v", err)
	}
	if err := t.MarkRunning(pid); err != nil {
		_ = s.backend.Cleanup(ctx, t.ID)
		return nil, errgrpc.ToGRPC(err)
	}
	go s.watch(t)
	s.publish(ctx, runtime.TaskStartEventTopic, &eventstypes.TaskStart{ContainerID: t.ID, Pid: uint32(pid)})
	return &taskAPI.StartResponse{Pid: uint32(pid)}, nil
}

// watch follows the microVM console log, copies the workload output between the markers to the task's
// stdout, and records the exit status from the exit marker or from the VMM disappearing.
func (s *Service) watch(t *Task) {
	ctx := context.Background()
	var out io.WriteCloser
	if t.Stdout != "" {
		if f, err := os.OpenFile(t.Stdout, os.O_WRONLY, 0); err == nil {
			out = f
		} else {
			log.G(ctx).WithError(err).WithField("path", t.Stdout).Warn("open task stdout")
		}
	}
	status, at := followLog(s.backend.LogPath(t.ID), out, func() bool { return s.backend.Alive(t.ID) })
	if out != nil {
		out.Close()
	}
	if t.MarkExited(status, at) {
		s.publish(ctx, runtime.TaskExitEventTopic, &eventstypes.TaskExit{
			ContainerID: t.ID, ID: t.ID, Pid: uint32(t.Pid()), ExitStatus: status, ExitedAt: protobuf.ToTimestamp(at),
		})
	}
}

// followLog tails the console log (which may not exist yet), writing lines between the start and exit
// markers to out; it returns the exit status from the marker, or exitKilled once the VMM is gone.
func followLog(path string, out io.Writer, alive func() bool) (uint32, time.Time) {
	var (
		f       *os.File
		rd      *bufio.Reader
		started bool
		partial string
		gone    int
	)
	for {
		if f == nil {
			if h, err := os.Open(path); err == nil {
				f, rd = h, bufio.NewReader(h)
			}
		}
		progressed := false
		if rd != nil {
			for {
				chunk, err := rd.ReadString('\n')
				if chunk != "" {
					progressed = true
				}
				if err != nil {
					partial += chunk
					break
				}
				line := strings.TrimRight(partial+chunk, "\r\n")
				partial = ""
				switch {
				case !started && strings.HasPrefix(line, markerStart):
					started = true
				case started && strings.HasPrefix(line, markerExit):
					f.Close()
					code, _ := strconv.Atoi(strings.Fields(strings.TrimPrefix(line, markerExit))[0])
					return uint32(code), time.Now()
				case started && out != nil:
					_, _ = io.WriteString(out, line+"\n")
				}
			}
		}
		if !progressed {
			if !alive() {
				gone++
				if gone >= 3 { // three quiet polls after the VMM vanished: nothing more will arrive
					if f != nil {
						f.Close()
					}
					return exitKilled, time.Now()
				}
			} else {
				gone = 0
			}
		}
		time.Sleep(pollEvery)
	}
}

// Kill stops the microVM; the watcher records the exit. A created (never started) task exits immediately.
func (s *Service) Kill(ctx context.Context, r *taskAPI.KillRequest) (*emptypb.Empty, error) {
	if r.ExecID != "" {
		return nil, errgrpc.ToGRPCf(errdefs.ErrNotImplemented, "exec is not supported yet (P3.2)")
	}
	t, err := s.getTask(r.ID)
	if err != nil {
		return nil, err
	}
	switch t.Phase() {
	case PhaseCreated:
		if t.MarkExited(exitKilled, time.Now()) {
			s.publish(ctx, runtime.TaskExitEventTopic, &eventstypes.TaskExit{ContainerID: t.ID, ID: t.ID, ExitStatus: exitKilled, ExitedAt: protobuf.ToTimestamp(time.Now())})
		}
		return &emptypb.Empty{}, nil
	case PhaseRunning:
		if err := s.backend.Stop(ctx, t.ID); err != nil {
			return nil, errgrpc.ToGRPCf(errdefs.ErrUnknown, "%v", err)
		}
		return &emptypb.Empty{}, nil
	default:
		return nil, errgrpc.ToGRPCf(errdefs.ErrNotFound, "task %s already exited", t.ID)
	}
}

// Delete tears the microVM and the root drive image down and reports the exit; the shim then shuts down.
func (s *Service) Delete(ctx context.Context, r *taskAPI.DeleteRequest) (*taskAPI.DeleteResponse, error) {
	if r.ExecID != "" {
		return nil, errgrpc.ToGRPCf(errdefs.ErrNotImplemented, "exec is not supported yet (P3.2)")
	}
	t, err := s.getTask(r.ID)
	if err != nil {
		return nil, err
	}
	if err := t.MarkDeleted(); err != nil {
		return nil, errgrpc.ToGRPC(err)
	}
	if err := s.backend.Cleanup(ctx, t.ID); err != nil {
		log.G(ctx).WithError(err).Warn("microVM cleanup on delete")
	}
	_ = os.Remove(t.Image)
	_ = mount.UnmountAll(filepath.Join(t.Bundle, "rootfs"), 0)
	status, at := t.Exit()
	s.publish(ctx, runtime.TaskDeleteEventTopic, &eventstypes.TaskDelete{ContainerID: t.ID, ID: t.ID, Pid: uint32(t.Pid()), ExitStatus: status, ExitedAt: protobuf.ToTimestamp(at)})
	if s.shutdown != nil {
		s.shutdown.Shutdown()
	}
	return &taskAPI.DeleteResponse{Pid: uint32(t.Pid()), ExitStatus: status, ExitedAt: protobuf.ToTimestamp(at)}, nil
}

// State reports the task.
func (s *Service) State(ctx context.Context, r *taskAPI.StateRequest) (*taskAPI.StateResponse, error) {
	if r.ExecID != "" {
		return nil, errgrpc.ToGRPCf(errdefs.ErrNotImplemented, "exec is not supported yet (P3.2)")
	}
	t, err := s.getTask(r.ID)
	if err != nil {
		return nil, err
	}
	status := tasktypes.Status_UNKNOWN
	switch t.Phase() {
	case PhaseCreated:
		status = tasktypes.Status_CREATED
	case PhaseRunning:
		status = tasktypes.Status_RUNNING
	case PhaseStopped:
		status = tasktypes.Status_STOPPED
	}
	exitStatus, exitedAt := t.Exit()
	resp := &taskAPI.StateResponse{
		ID: t.ID, Bundle: t.Bundle, Pid: uint32(t.Pid()), Status: status,
		Stdin: t.Stdin, Stdout: t.Stdout, Stderr: t.Stderr, Terminal: t.Terminal,
		ExitStatus: exitStatus,
	}
	if !exitedAt.IsZero() {
		resp.ExitedAt = protobuf.ToTimestamp(exitedAt)
	}
	return resp, nil
}

// Wait blocks until the task has exited.
func (s *Service) Wait(ctx context.Context, r *taskAPI.WaitRequest) (*taskAPI.WaitResponse, error) {
	if r.ExecID != "" {
		return nil, errgrpc.ToGRPCf(errdefs.ErrNotImplemented, "exec is not supported yet (P3.2)")
	}
	t, err := s.getTask(r.ID)
	if err != nil {
		return nil, err
	}
	select {
	case <-t.Exited():
	case <-ctx.Done():
		return nil, ctx.Err()
	}
	status, at := t.Exit()
	return &taskAPI.WaitResponse{ExitStatus: status, ExitedAt: protobuf.ToTimestamp(at)}, nil
}

// Pids lists the VMM pid; the guest's own processes are not visible to the host.
func (s *Service) Pids(ctx context.Context, r *taskAPI.PidsRequest) (*taskAPI.PidsResponse, error) {
	t, err := s.getTask(r.ID)
	if err != nil {
		return nil, err
	}
	var procs []*tasktypes.ProcessInfo
	if pid := t.Pid(); pid > 0 && t.Phase() == PhaseRunning {
		procs = append(procs, &tasktypes.ProcessInfo{Pid: uint32(pid)})
	}
	return &taskAPI.PidsResponse{Processes: procs}, nil
}

// Connect reports the shim and VMM pids.
func (s *Service) Connect(ctx context.Context, r *taskAPI.ConnectRequest) (*taskAPI.ConnectResponse, error) {
	var pid int
	if t, err := s.getTask(r.ID); err == nil {
		pid = t.Pid()
	}
	return &taskAPI.ConnectResponse{ShimPid: uint32(os.Getpid()), TaskPid: uint32(pid)}, nil
}

// Shutdown stops the shim once its task is gone.
func (s *Service) Shutdown(ctx context.Context, r *taskAPI.ShutdownRequest) (*emptypb.Empty, error) {
	s.mu.Lock()
	busy := s.task != nil && s.task.Phase() != PhaseDeleted
	s.mu.Unlock()
	if busy && !r.Now {
		return &emptypb.Empty{}, nil
	}
	if s.shutdown != nil {
		s.shutdown.Shutdown()
	}
	return &emptypb.Empty{}, nil
}

func notImplemented(what string) error {
	return errgrpc.ToGRPCf(errdefs.ErrNotImplemented, "%s is not supported by %s yet", what, "io.containerd.vesper.v2")
}

func (s *Service) Pause(context.Context, *taskAPI.PauseRequest) (*emptypb.Empty, error) {
	return nil, notImplemented("pause")
}
func (s *Service) Resume(context.Context, *taskAPI.ResumeRequest) (*emptypb.Empty, error) {
	return nil, notImplemented("resume")
}
func (s *Service) Checkpoint(context.Context, *taskAPI.CheckpointTaskRequest) (*emptypb.Empty, error) {
	return nil, notImplemented("checkpoint")
}
func (s *Service) Exec(context.Context, *taskAPI.ExecProcessRequest) (*emptypb.Empty, error) {
	return nil, notImplemented("exec (P3.2)")
}
func (s *Service) ResizePty(context.Context, *taskAPI.ResizePtyRequest) (*emptypb.Empty, error) {
	return nil, notImplemented("resize pty (P3.2)")
}
func (s *Service) CloseIO(context.Context, *taskAPI.CloseIORequest) (*emptypb.Empty, error) {
	return nil, notImplemented("close io (P3.2)")
}
func (s *Service) Update(context.Context, *taskAPI.UpdateTaskRequest) (*emptypb.Empty, error) {
	return nil, notImplemented("update")
}
func (s *Service) Stats(context.Context, *taskAPI.StatsRequest) (*taskAPI.StatsResponse, error) {
	return nil, notImplemented("stats")
}

var _ types.Mount
var _ = json.Marshal
var _ specs.Spec
