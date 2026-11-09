// Package task implements the containerd runtime v2 Task service for Vesper: one task per shim, each task
// a jailed Firecracker microVM booted from an ext4 image built out of the OCI bundle rootfs.
package task

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
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
	// exitKilled is reported when the VMM disappears without a workload exit marker (kill or crash).
	exitKilled = 137
)

// Service is the ttrpc Task service.
type Service struct {
	mu        sync.Mutex
	backend   backend.Backend
	publisher shim.Publisher
	shutdown  shutdown.Service
	task      *Task
	execs     map[string]*Task
	nextPort  uint32
}

// NewService wires the service; `b` is the production microVM backend unless a test injects a fake.
func NewService(ctx context.Context, publisher shim.Publisher, sd shutdown.Service, b backend.Backend) (*Service, error) {
	s := &Service{backend: b, publisher: publisher, shutdown: sd, execs: make(map[string]*Task), nextPort: 1032}
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
	if r.Checkpoint != "" || r.ParentCheckpoint != "" {
		return nil, errgrpc.ToGRPCf(errdefs.ErrNotImplemented, "checkpoint restore is not supported")
	}
	rootfs := filepath.Join(r.Bundle, "rootfs")
	mounts := make([]mount.Mount, 0, len(r.Rootfs))
	for _, m := range r.Rootfs {
		mounts = append(mounts, mount.Mount{Type: m.Type, Source: m.Source, Target: m.Target, Options: m.Options})
	}
	spec, err := bundle.Validate(r.Bundle, mounts)
	if err != nil {
		return nil, errgrpc.ToGRPCf(errdefs.ErrInvalidArgument, "%v", err)
	}
	spec.Process.Terminal = r.Terminal
	config, err := bundle.GuestConfig(spec.Process)
	if err != nil {
		return nil, errgrpc.ToGRPCf(errdefs.ErrInvalidArgument, "%v", err)
	}
	if err := os.MkdirAll(rootfs, 0o711); err != nil {
		return nil, fmt.Errorf("create bundle rootfs: %w", err)
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
	t := newTask(r.ID, r.Bundle)
	t.Image, t.Config, t.Port = image, config, 1028
	t.Stdin, t.Stdout, t.Stderr, t.Terminal = r.Stdin, r.Stdout, r.Stderr, r.Terminal
	s.task = t
	s.persist(ctx, t)
	s.publish(ctx, runtime.TaskCreateEventTopic, &eventstypes.TaskCreate{
		ContainerID: r.ID,
		Bundle:      r.Bundle,
		Rootfs:      r.Rootfs,
		IO:          &eventstypes.TaskIO{Stdin: r.Stdin, Stdout: r.Stdout, Stderr: r.Stderr, Terminal: r.Terminal},
		Pid:         uint32(os.Getpid()),
	})
	return &taskAPI.CreateTaskResponse{Pid: uint32(os.Getpid())}, nil
}

// Start boots the microVM for init, then starts a runc process with connected streams.
func (s *Service) Start(ctx context.Context, r *taskAPI.StartRequest) (*taskAPI.StartResponse, error) {
	t, err := s.getProcess(r.ID, r.ExecID)
	if err != nil {
		return nil, err
	}
	t.opMu.Lock()
	defer t.opMu.Unlock()
	if t.Phase() != PhaseCreated {
		return nil, errgrpc.ToGRPCf(errdefs.ErrFailedPrecondition, "process %s is %s, not created", t.ID, t.Phase())
	}
	pid := 0
	if r.ExecID == "" {
		pid, err = s.backend.Launch(ctx, t.ID, t.Image)
		if err != nil {
			_ = s.backend.Cleanup(context.Background(), r.ID)
			return nil, errgrpc.ToGRPCf(errdefs.ErrFailedPrecondition, "launch microVM: %v", err)
		}
	} else {
		parent, e := s.getTask(r.ID)
		if e != nil {
			return nil, e
		}
		parent.opMu.Lock()
		defer parent.opMu.Unlock()
		if parent.Phase() != PhaseRunning || parent.exiting {
			return nil, errgrpc.ToGRPCf(errdefs.ErrFailedPrecondition, "task is not running")
		}
		pid = parent.Pid()
	}
	session, err := s.backend.OpenSession(ctx, r.ID, t.Port, t.Config, r.ExecID != "", backend.IO{Stdin: t.Stdin, Stdout: t.Stdout, Stderr: t.Stderr, Terminal: t.Terminal})
	if err != nil {
		if r.ExecID == "" {
			_ = s.backend.Cleanup(context.Background(), r.ID)
		} else {
			_ = s.backend.Abort(context.Background(), r.ID, t.Port)
			t.MarkExited(137, time.Now())
		}
		return nil, errgrpc.ToGRPCf(errdefs.ErrFailedPrecondition, "guest session p%d: %v (console: %s)", t.Port, err, s.backend.LogPath(r.ID))
	}
	t.Session = session
	if t.stdinClosed {
		session.CloseInput()
	}
	if err := t.MarkRunning(pid); err != nil {
		session.Close()
		return nil, errgrpc.ToGRPC(err)
	}
	if r.ExecID == "" {
		s.persist(ctx, t)
		s.publish(ctx, runtime.TaskStartEventTopic, &eventstypes.TaskStart{ContainerID: r.ID, Pid: uint32(pid)})
	} else {
		s.publish(ctx, runtime.TaskExecStartedEventTopic, &eventstypes.TaskExecStarted{ContainerID: r.ID, ExecID: r.ExecID, Pid: uint32(pid)})
	}
	go s.watch(context.WithoutCancel(ctx), r.ID, t)
	return &taskAPI.StartResponse{Pid: uint32(pid)}, nil
}

func (s *Service) watch(ctx context.Context, containerID string, t *Task) {
	defer close(t.watchDone)
	status, err := t.Session.Wait()
	t.Session.Close()
	if err != nil {
		log.G(ctx).WithError(err).WithFields(log.Fields{"task": containerID, "session": t.ID, "port": t.Port}).Error("guest session failed")
	}
	if containerID == t.ID {
		t.opMu.Lock()
		t.exiting = true
		t.opMu.Unlock()
		if err != nil {
			_ = s.backend.Cleanup(ctx, containerID)
		}
		s.mu.Lock()
		children := make([]*Task, 0, len(s.execs))
		for _, child := range s.execs {
			children = append(children, child)
		}
		s.mu.Unlock()
		for _, child := range children {
			child.opMu.Lock()
			running := child.Session != nil
			child.opMu.Unlock()
			if running {
				<-child.watchDone
			}
		}
	} else if err != nil {
		_ = s.backend.Abort(ctx, containerID, t.Port)
	}
	at := time.Now()
	if t.MarkExited(status, at) {
		if containerID == t.ID {
			s.persist(ctx, t)
		}
		s.publish(ctx, runtime.TaskExitEventTopic, &eventstypes.TaskExit{ContainerID: containerID, ID: t.ID, Pid: uint32(t.Pid()), ExitStatus: status, ExitedAt: protobuf.ToTimestamp(at)})
	}
}

func (s *Service) getProcess(id, execID string) (*Task, error) {
	t, err := s.getTask(id)
	if err != nil || execID == "" {
		return t, err
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	t = s.execs[execID]
	if t == nil {
		return nil, errgrpc.ToGRPCf(errdefs.ErrNotFound, "exec %s not found", execID)
	}
	return t, nil
}

// Kill stops the microVM; the watcher records the exit. A created (never started) task exits immediately.
func (s *Service) Kill(ctx context.Context, r *taskAPI.KillRequest) (*emptypb.Empty, error) {
	t, err := s.getProcess(r.ID, r.ExecID)
	if err != nil {
		return nil, err
	}
	t.opMu.Lock()
	defer t.opMu.Unlock()
	if r.Signal > 64 {
		return nil, errgrpc.ToGRPCf(errdefs.ErrInvalidArgument, "signal must be 0..64")
	}
	switch t.Phase() {
	case PhaseCreated:
		if r.Signal == 0 {
			return nil, errgrpc.ToGRPCf(errdefs.ErrFailedPrecondition, "process is not running")
		}
		if t.MarkExited(exitKilled, time.Now()) {
			if r.ExecID == "" {
				s.persist(ctx, t)
			}
			s.publish(ctx, runtime.TaskExitEventTopic, &eventstypes.TaskExit{ContainerID: r.ID, ID: t.ID, ExitStatus: exitKilled, ExitedAt: protobuf.ToTimestamp(time.Now())})
		}
		return &emptypb.Empty{}, nil
	case PhaseRunning:
		if err := s.backend.Signal(ctx, r.ID, t.Port, r.Signal, r.All); err != nil {
			return nil, errgrpc.ToGRPCf(errdefs.ErrUnknown, "%v", err)
		}
		return &emptypb.Empty{}, nil
	default:
		return nil, errgrpc.ToGRPCf(errdefs.ErrNotFound, "task %s already exited", t.ID)
	}
}

// Delete tears down the microVM and drive; containerd requests Shutdown after receiving the reply.
func (s *Service) Delete(ctx context.Context, r *taskAPI.DeleteRequest) (*taskAPI.DeleteResponse, error) {
	t, err := s.getProcess(r.ID, r.ExecID)
	if err != nil {
		return nil, err
	}
	t.opMu.Lock()
	defer t.opMu.Unlock()
	if t.Phase() == PhaseStopped && t.Session != nil {
		<-t.watchDone
	}
	if err := t.MarkDeleted(); err != nil {
		return nil, errgrpc.ToGRPC(err)
	}
	if r.ExecID != "" {
		s.mu.Lock()
		delete(s.execs, r.ExecID)
		s.mu.Unlock()
		status, at := t.Exit()
		return &taskAPI.DeleteResponse{Pid: uint32(t.Pid()), ExitStatus: status, ExitedAt: protobuf.ToTimestamp(at)}, nil
	}
	s.persist(ctx, t)
	if err := s.backend.Cleanup(ctx, t.ID); err != nil {
		log.G(ctx).WithError(err).Warn("microVM cleanup on delete")
	}
	_ = os.Remove(t.Image)
	_ = mount.UnmountAll(filepath.Join(t.Bundle, "rootfs"), 0)
	status, at := t.Exit()
	s.publish(ctx, runtime.TaskDeleteEventTopic, &eventstypes.TaskDelete{ContainerID: t.ID, ID: t.ID, Pid: uint32(t.Pid()), ExitStatus: status, ExitedAt: protobuf.ToTimestamp(at)})
	return &taskAPI.DeleteResponse{Pid: uint32(t.Pid()), ExitStatus: status, ExitedAt: protobuf.ToTimestamp(at)}, nil
}

// State reports the task.
func (s *Service) State(ctx context.Context, r *taskAPI.StateRequest) (*taskAPI.StateResponse, error) {
	t, err := s.getProcess(r.ID, r.ExecID)
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
	t, err := s.getProcess(r.ID, r.ExecID)
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

func (s *Service) persist(ctx context.Context, t *Task) {
	if err := t.persist(); err != nil {
		log.G(ctx).WithError(err).WithField("task", t.ID).Warn("persist task state")
	}
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
func (s *Service) Exec(ctx context.Context, r *taskAPI.ExecProcessRequest) (*emptypb.Empty, error) {
	parent, err := s.getTask(r.ID)
	if err != nil {
		return nil, err
	}
	parent.opMu.Lock()
	defer parent.opMu.Unlock()
	if parent.Phase() != PhaseRunning || parent.exiting {
		return nil, errgrpc.ToGRPCf(errdefs.ErrFailedPrecondition, "task must be running")
	}
	if r.ExecID == "" || r.ExecID == r.ID || r.Spec == nil {
		return nil, errgrpc.ToGRPCf(errdefs.ErrInvalidArgument, "exec needs a distinct id and process spec")
	}
	var process specs.Process
	if err := json.Unmarshal(r.Spec.Value, &process); err != nil {
		return nil, errgrpc.ToGRPCf(errdefs.ErrInvalidArgument, "exec process: %v", err)
	}
	process.Terminal = r.Terminal
	p, err := bundle.GuestProcess(&process)
	if err != nil {
		return nil, errgrpc.ToGRPCf(errdefs.ErrInvalidArgument, "exec process: %v", err)
	}
	p.Terminal = r.Terminal
	config, err := json.Marshal(p)
	if err != nil {
		return nil, err
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.execs[r.ExecID] != nil {
		return nil, errgrpc.ToGRPCf(errdefs.ErrAlreadyExists, "exec %s exists", r.ExecID)
	}
	// shortcut: ports are not reused; add guest session reclamation if tasks need over 16,125 execs.
	if s.nextPort > 65528 {
		return nil, errgrpc.ToGRPCf(errdefs.ErrResourceExhausted, "guest session ports exhausted")
	}
	t := newTask(r.ExecID, parent.Bundle)
	t.Config, t.Port = config, s.nextPort
	s.nextPort += 4
	t.Stdin, t.Stdout, t.Stderr, t.Terminal = r.Stdin, r.Stdout, r.Stderr, r.Terminal
	s.execs[r.ExecID] = t
	s.publish(ctx, runtime.TaskExecAddedEventTopic, &eventstypes.TaskExecAdded{ContainerID: r.ID, ExecID: r.ExecID})
	return &emptypb.Empty{}, nil
}
func (s *Service) ResizePty(ctx context.Context, r *taskAPI.ResizePtyRequest) (*emptypb.Empty, error) {
	t, err := s.getProcess(r.ID, r.ExecID)
	if err != nil {
		return nil, err
	}
	t.opMu.Lock()
	defer t.opMu.Unlock()
	if !t.Terminal || t.Phase() != PhaseRunning {
		return nil, errgrpc.ToGRPCf(errdefs.ErrFailedPrecondition, "terminal session is not running")
	}
	if err := s.backend.Resize(ctx, r.ID, t.Port, r.Height, r.Width); err != nil {
		return nil, errgrpc.ToGRPCf(errdefs.ErrInvalidArgument, "resize: %v", err)
	}
	return &emptypb.Empty{}, nil
}
func (s *Service) CloseIO(ctx context.Context, r *taskAPI.CloseIORequest) (*emptypb.Empty, error) {
	t, err := s.getProcess(r.ID, r.ExecID)
	if err != nil {
		return nil, err
	}
	t.opMu.Lock()
	defer t.opMu.Unlock()
	if r.Stdin {
		t.stdinClosed = true
		if t.Session != nil {
			t.Session.CloseInput()
		}
	}
	return &emptypb.Empty{}, nil
}
func (s *Service) Update(context.Context, *taskAPI.UpdateTaskRequest) (*emptypb.Empty, error) {
	return nil, notImplemented("update")
}

var _ types.Mount
