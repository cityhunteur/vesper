// Package task implements the containerd runtime v2 Task service for Vesper: one task per shim, each task
// a jailed Firecracker microVM booted from an ext4 image built out of the OCI bundle rootfs.
package task

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"syscall"
	"time"

	"github.com/cityhunteur/vesper/shim/internal/backend"
	"github.com/cityhunteur/vesper/shim/internal/bundle"
	"github.com/cityhunteur/vesper/shim/internal/ctrns"
	"github.com/cityhunteur/vesper/shim/internal/shares"
	eventstypes "github.com/containerd/containerd/api/events"
	taskAPI "github.com/containerd/containerd/api/runtime/task/v3"
	"github.com/containerd/containerd/api/types"
	tasktypes "github.com/containerd/containerd/api/types/task"
	"github.com/containerd/containerd/v2/core/mount"
	"github.com/containerd/containerd/v2/core/runtime"
	"github.com/containerd/containerd/v2/pkg/namespaces"
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
	// freePorts are exec session ports given back by deleted execs, used before nextPort (TD-041).
	freePorts []uint32
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
	if root, ok := bundle.DockerRoot(spec); ok && len(mounts) == 0 {
		return s.createDocker(ctx, r, spec, root)
	}
	// Bind mounts from macOS shares become NFS mounts from the microVM's own server (ADR 0009, GH #160 M5); other
	// bind mounts keep today's behaviour.
	plan, annotations, err := planShares(spec)
	if err != nil {
		return nil, errgrpc.ToGRPCf(errdefs.ErrInvalidArgument, "%v", err)
	}
	// The machine and the limits the guest applies both come from the spec's resources (TD-048).
	size, err := bundle.SizeOf(spec, bundle.SystemCPUs())
	if err != nil {
		return nil, errgrpc.ToGRPCf(errdefs.ErrInvalidArgument, "%v", err)
	}
	config, err := bundle.GuestConfig(spec.Process, annotations, bundle.GuestResources(spec))
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
	t.LaunchArgs, t.SizeArgs = plan.LaunchArgs(), size.LaunchArgs()
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

// planShares maps spec's binds from macOS shares onto the microVM's share server (ADR 0009) and returns the plan with
// the guest config's annotation for it (none when there is no share).
func planShares(spec *specs.Spec) (shares.Plan, map[string]string, error) {
	procMounts, _ := os.ReadFile("/proc/mounts")
	plan, err := shares.MakePlan(spec.Mounts, shares.ShareMounts(string(procMounts)), shares.HostResolver)
	if err != nil || len(plan.Shares) == 0 {
		return plan, nil, err
	}
	v, err := shares.Encode(plan.Shares)
	if err != nil {
		return plan, nil, err
	}
	return plan, map[string]string{shares.Annotation: v}, nil
}

// hasShareNet reports whether a launch has a share server to stop.
func hasShareNet(args []string) bool {
	for _, a := range args {
		if a == "--share-net" {
			return true
		}
	}
	return false
}

// The namespace holder, replaceable in tests.
var (
	startNetns   = ctrns.Start
	releaseNetns = ctrns.Release
)

// createDocker is Create for a dockerd task (ADR 0011): the spec is checked against the field table, the
// container's own drive is built or reused, and a network namespace is made for dockerd to wire; Create reports the
// holder's pid, through which dockerd finds that namespace.
func (s *Service) createDocker(ctx context.Context, r *taskAPI.CreateTaskRequest, spec *specs.Spec, root string) (*taskAPI.CreateTaskResponse, error) {
	// Only dockerd's tasks: its containerd namespaces are moby and moby-<uid>.<gid>. A task in another namespace with
	// a Docker container's id and a root.path shaped like dockerd's would otherwise take that container's drive
	// (council SEC-6).
	if ns, _ := namespaces.Namespace(ctx); ns != "moby" && !strings.HasPrefix(ns, "moby-") {
		return nil, errgrpc.ToGRPCf(errdefs.ErrInvalidArgument, "a dockerd root filesystem in containerd namespace %q is refused (ADR 0011)", ns)
	}
	// Binds from macOS shares reach the microVM through its share server (ADR 0011 slice 4); CheckDocker refuses
	// every other bind of a System VM path.
	plan, annotations, err := planShares(spec)
	if err != nil {
		return nil, errgrpc.ToGRPCf(errdefs.ErrInvalidArgument, "%v", err)
	}
	shareDests := map[string]bool{}
	for _, sh := range plan.Shares {
		shareDests[sh.Dest] = true
	}
	files, err := bundle.CheckDocker(spec, r.ID, shareDests)
	if err != nil {
		return nil, errgrpc.ToGRPCf(errdefs.ErrInvalidArgument, "%v", err)
	}
	size, err := bundle.SizeOf(spec, bundle.SystemCPUs())
	if err != nil {
		return nil, errgrpc.ToGRPCf(errdefs.ErrInvalidArgument, "%v", err)
	}
	t := newTask(r.ID, r.Bundle)
	t.Port = 1028
	// The guest config is built at Start: dockerd writes the container's address into its hosts file while it wires
	// the namespace, after Create. Built here once with no files, it proves the spec converts.
	if _, err := bundle.GuestDockerConfig(spec, nil, bundle.GuestResources(spec), fmt.Sprintf("/run/sessions/p%d", t.Port), annotations); err != nil {
		return nil, errgrpc.ToGRPCf(errdefs.ErrInvalidArgument, "%v", err)
	}
	t.dockerSpec, t.dockerFiles, t.dockerAnnotations = spec, files, annotations
	var uids, gids []specs.LinuxIDMapping
	if spec.Linux != nil {
		uids, gids = spec.Linux.UIDMappings, spec.Linux.GIDMappings
	}
	image, built, err := s.backend.PrepareDocker(ctx, r.ID, root, uids, gids)
	if err != nil {
		return nil, errgrpc.ToGRPCf(errdefs.ErrFailedPrecondition, "%v", err)
	}
	log.G(ctx).WithFields(log.Fields{"task": r.ID, "drive": image, "built": built}).Info("docker drive ready")
	holder, err := startNetns(ctx, r.ID, func() { s.holderGone(r.ID) })
	if err != nil {
		return nil, errgrpc.ToGRPCf(errdefs.ErrFailedPrecondition, "%v", err)
	}
	t.Image, t.Docker, t.Holder = image, true, holder
	t.SetCreatedPid(holder)
	t.LaunchArgs, t.SizeArgs = append([]string{"--ctrns"}, plan.LaunchArgs()...), size.LaunchArgs()
	t.Stdin, t.Stdout, t.Stderr, t.Terminal = r.Stdin, r.Stdout, r.Stderr, r.Terminal
	s.task = t
	s.persist(ctx, t)
	s.publish(ctx, runtime.TaskCreateEventTopic, &eventstypes.TaskCreate{
		ContainerID: r.ID,
		Bundle:      r.Bundle,
		Rootfs:      r.Rootfs,
		IO:          &eventstypes.TaskIO{Stdin: r.Stdin, Stdout: r.Stdout, Stderr: r.Stderr, Terminal: r.Terminal},
		Pid:         uint32(holder),
	})
	return &taskAPI.CreateTaskResponse{Pid: uint32(holder)}, nil
}

// holderGone stops a Docker task whose namespace holder exited: the holder's pid is the task's, which dockerd uses to
// reach its namespaces, and a reused pid must never stand for it (ADR 0011 §2). After Delete it does nothing.
func (s *Service) holderGone(id string) {
	s.mu.Lock()
	t := s.task
	s.mu.Unlock()
	if t == nil || t.ID != id {
		return
	}
	t.mu.Lock()
	t.holderDead = true
	t.mu.Unlock()
	switch t.Phase() {
	case PhaseRunning:
		log.G(context.Background()).WithField("task", id).Warn("namespace holder exited; stopping the microVM")
		_ = s.backend.Stop(context.Background(), id)
	}
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
	if r.ExecID == "" && t.Docker && t.HolderDead() {
		return nil, errgrpc.ToGRPCf(errdefs.ErrFailedPrecondition, "the task's network namespace holder has exited")
	}
	if r.ExecID == "" && t.Docker && t.dockerSpec != nil {
		contents, err := backend.ReadDockerFiles(t.dockerFiles)
		if err != nil {
			return nil, errgrpc.ToGRPCf(errdefs.ErrFailedPrecondition, "%v", err)
		}
		if t.Config, err = bundle.GuestDockerConfig(t.dockerSpec, contents, bundle.GuestResources(t.dockerSpec), fmt.Sprintf("/run/sessions/p%d", t.Port), t.dockerAnnotations); err != nil {
			return nil, errgrpc.ToGRPCf(errdefs.ErrInvalidArgument, "%v", err)
		}
	}
	if r.ExecID == "" {
		pid, err = s.backend.Launch(ctx, t.ID, t.Image, append(append([]string(nil), t.LaunchArgs...), t.SizeArgs...))
		if err != nil {
			_ = s.backend.Cleanup(context.Background(), r.ID, false)
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
		if parent.paused.Load() {
			return nil, errgrpc.ToGRPCf(errdefs.ErrFailedPrecondition, "task is paused; resume it first")
		}
		pid = parent.Pid()
	}
	t.guestKnows = true
	session, err := s.backend.OpenSession(ctx, r.ID, t.Port, t.Config, r.ExecID != "", backend.IO{Stdin: t.Stdin, Stdout: t.Stdout, Stderr: t.Stderr, Terminal: t.Terminal})
	if err != nil {
		if r.ExecID == "" {
			_ = s.backend.Cleanup(context.Background(), r.ID, false)
		} else {
			_ = s.backend.Abort(context.Background(), r.ID, t.Port)
			t.MarkExited(137, time.Now())
		}
		return nil, errgrpc.ToGRPCf(errdefs.ErrFailedPrecondition, "guest session p%d: %v (console: %s)", t.Port, err, s.backend.LogPath(r.ID))
	}
	t.Session = session
	if r.ExecID == "" && t.Docker {
		// The holder's pid stays the task's: a pid or ipc join that races the plugin's check lands in the holder's
		// empty namespaces, not in Firecracker's, which are the System VM's (council SEC-1). The VMM's pid is kept
		// for its cgroup (Stats, judge N1).
		t.SetVMMPid(pid)
		pid = t.Holder
	}
	if t.stdinClosed {
		session.CloseInput()
	}
	if err := t.MarkRunning(pid); err != nil {
		session.Close()
		// No watcher is started below, so this session's watchDone never closes. Clear the field
		// so the init watcher's child-wait (keyed on Session != nil) does not block on it forever.
		t.Session = nil
		return nil, errgrpc.ToGRPC(err)
	}
	if r.ExecID == "" && t.Docker && t.HolderDead() {
		// The holder died while the microVM booted; holderGone saw the task created and left it (judge N2).
		_ = s.backend.Stop(context.Background(), r.ID)
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
			_ = s.backend.Cleanup(ctx, containerID, false)
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
			// The guest unmounted its shares before EXIT, so the server has nothing left to serve (ADR 0009,
			// Lifecycle): stop it now rather than at Delete, which may come much later.
			if hasShareNet(t.LaunchArgs) {
				go func() {
					if err := s.backend.StopShares(context.Background(), containerID); err != nil {
						log.L.WithError(err).WithField("task", containerID).Warn("stop share server")
					}
				}()
			}
		}
		exit := &eventstypes.TaskExit{ContainerID: containerID, ID: t.ID, Pid: uint32(t.Pid()), ExitStatus: status, ExitedAt: protobuf.ToTimestamp(at)}
		s.publish(ctx, runtime.TaskExitEventTopic, exit)
		if containerID == t.ID {
			// Detached from the request, but in its containerd namespace: an event without one is refused.
			ns, _ := namespaces.Namespace(ctx)
			go s.republishExit(namespaces.WithNamespace(context.Background(), ns), t, exit)
		}
	}
}

// Republishing of an init task's exit (TD-098): dockerd deletes an exited task within milliseconds of the event,
// but an exit published while containerd restarts reaches no subscriber, and dockerd does not look again when it
// resubscribes, so the container showed running until docker stop (with runc as well). Until the task is deleted,
// the exit is published again every ExitRepublishInterval, at most ExitRepublishTimes times.
var (
	ExitRepublishInterval = 10 * time.Second
	ExitRepublishTimes    = 6
)

func (s *Service) republishExit(ctx context.Context, t *Task, exit *eventstypes.TaskExit) {
	for i := 0; i < ExitRepublishTimes; i++ {
		time.Sleep(ExitRepublishInterval)
		if t.Phase() == PhaseDeleted {
			return
		}
		log.L.WithField("task", t.ID).Info("the exit is not yet acted on; publishing it again (TD-098)")
		s.publish(ctx, runtime.TaskExitEventTopic, exit)
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
		// A paused guest cannot take a signal: its vCPUs are stopped. As a frozen runc container gets SIGKILL when it
		// thaws, the microVM is resumed first (ADR 0011 slice 5).
		// Signal 0 only asks whether the process lives: it is answered without resuming.
		if task, _ := s.getTask(r.ID); task != nil && task.paused.Load() {
			if r.Signal == 0 {
				return &emptypb.Empty{}, nil
			}
			if r.ExecID != "" {
				// The task's lock is not held here; its own Kill or Resume changes the pause.
				return nil, errgrpc.ToGRPCf(errdefs.ErrFailedPrecondition, "task is paused; resume it first")
			}
			if err := s.backend.SetPaused(ctx, r.ID, false); err != nil {
				if r.Signal == uint32(syscall.SIGKILL) {
					// A paused guest that cannot be resumed still dies: the bounded wait stops its VMM (review of D-090).
					go s.killFallback(r.ID, t)
					return &emptypb.Empty{}, nil
				}
				return nil, errgrpc.ToGRPCf(errdefs.ErrUnknown, "resume before the signal: %v", err)
			}
			task.paused.Store(false)
			s.publish(ctx, runtime.TaskResumedEventTopic, &eventstypes.TaskResumed{ContainerID: r.ID})
		}
		// A SIGKILL to the init process starts a bounded wait for its EXIT, outside the operation lock. The guest
		// unmounts its shares before it reports EXIT, so a process stuck in a hard NFS call to a dead server could
		// hold the task forever; when the wait runs out the VMM is stopped (ADR 0009, Lifecycle; GH #160 M5). The
		// wait starts even when the guest refuses the signal: a workload that exited by itself while its share
		// server was dead leaves the guest stuck in that unmount, refusing signals as no longer running, and the
		// task could then be neither killed nor deleted (council REL-1).
		fallback := r.ExecID == "" && r.Signal == uint32(syscall.SIGKILL)
		if err := s.backend.Signal(ctx, r.ID, t.Port, r.Signal, r.All); err != nil {
			if !fallback {
				return nil, errgrpc.ToGRPCf(errdefs.ErrUnknown, "%v", err)
			}
			go s.killFallback(r.ID, t)
			// The guest says its workload is no longer running: the process the SIGKILL was for is gone, and the
			// fallback ends the session it left behind, so the kill succeeds.
			if !strings.Contains(err.Error(), "session-not-running") {
				return nil, errgrpc.ToGRPCf(errdefs.ErrUnknown, "%v", err)
			}
		}
		if fallback {
			go s.killFallback(r.ID, t)
		}
		return &emptypb.Empty{}, nil
	default:
		return nil, errgrpc.ToGRPCf(errdefs.ErrNotFound, "task %s already exited", t.ID)
	}
}

// KillGrace bounds the wait for a SIGKILLed init process to report EXIT before the VMM is stopped. It covers the
// guest's unmount, which flushes the writes it still buffers to the share server.
var KillGrace = 10 * time.Second

func (s *Service) killFallback(id string, t *Task) {
	select {
	case <-t.Exited():
		return
	case <-time.After(KillGrace):
	}
	log.L.WithField("task", id).Warn("no EXIT after SIGKILL; stopping the microVM")
	if err := s.backend.Stop(context.Background(), id); err != nil {
		log.L.WithError(err).WithField("task", id).Warn("stop microVM after SIGKILL")
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
		s.releasePort(ctx, r.ID, t)
		status, at := t.Exit()
		return &taskAPI.DeleteResponse{Pid: uint32(t.Pid()), ExitStatus: status, ExitedAt: protobuf.ToTimestamp(at)}, nil
	}
	s.persist(ctx, t)
	if err := s.backend.Cleanup(ctx, t.ID, true); err != nil {
		log.G(ctx).WithError(err).Warn("microVM cleanup on delete")
	}
	if t.Docker {
		// The drive is the container's own and outlives the task (ADR 0011 §5); the namespace goes with it.
		if err := releaseNetns(ctx, t.ID); err != nil {
			log.G(ctx).WithError(err).Warn("network namespace release on delete")
		}
	} else {
		_ = os.Remove(t.Image)
	}
	_ = mount.UnmountAll(filepath.Join(t.Bundle, "rootfs"), 0)
	status, at := t.Exit()
	s.publish(ctx, runtime.TaskDeleteEventTopic, &eventstypes.TaskDelete{ContainerID: t.ID, ID: t.ID, Pid: uint32(t.Pid()), ExitStatus: status, ExitedAt: protobuf.ToTimestamp(at)})
	return &taskAPI.DeleteResponse{Pid: uint32(t.Pid()), ExitStatus: status, ExitedAt: protobuf.ToTimestamp(at)}, nil
}

// releasePort gives a deleted exec's session port back (TD-041). An exec the guest never heard of frees its port at
// once; any other needs the guest to remove the ended session's directory first (RELEASE), so a new session on the
// port is not refused as a duplicate. A port the guest does not release stays used, as before.
func (s *Service) releasePort(ctx context.Context, id string, t *Task) {
	if t.Port == 0 {
		return
	}
	if t.guestKnows {
		if s.backend == nil {
			return
		}
		if err := s.backend.Release(ctx, id, t.Port); err != nil {
			log.G(ctx).WithError(err).WithField("port", t.Port).Warn("exec session not released; its port is not reused")
			return
		}
	}
	s.mu.Lock()
	s.freePorts = append(s.freePorts, t.Port)
	s.mu.Unlock()
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
		if t.paused.Load() {
			status = tasktypes.Status_PAUSED
		}
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

// Pause stops the microVM's vCPUs through its Firecracker (ADR 0011 slice 5): the whole guest halts, the task's
// processes with it, as the cgroup freezer halts a runc container's. State reports PAUSED until Resume.
func (s *Service) Pause(ctx context.Context, r *taskAPI.PauseRequest) (*emptypb.Empty, error) {
	return s.setPaused(ctx, r.ID, true)
}

// Resume starts the microVM's vCPUs again.
func (s *Service) Resume(ctx context.Context, r *taskAPI.ResumeRequest) (*emptypb.Empty, error) {
	return s.setPaused(ctx, r.ID, false)
}

func (s *Service) setPaused(ctx context.Context, id string, paused bool) (*emptypb.Empty, error) {
	t, err := s.getTask(id)
	if err != nil {
		return nil, err
	}
	t.opMu.Lock()
	defer t.opMu.Unlock()
	if t.Phase() != PhaseRunning || t.exiting {
		return nil, errgrpc.ToGRPCf(errdefs.ErrFailedPrecondition, "task must be running")
	}
	if t.paused.Load() == paused {
		if paused {
			return nil, errgrpc.ToGRPCf(errdefs.ErrFailedPrecondition, "task is already paused")
		}
		return nil, errgrpc.ToGRPCf(errdefs.ErrFailedPrecondition, "task is not paused")
	}
	if err := s.backend.SetPaused(ctx, id, paused); err != nil {
		return nil, errgrpc.ToGRPCf(errdefs.ErrUnknown, "%v", err)
	}
	t.paused.Store(paused)
	if paused {
		s.publish(ctx, runtime.TaskPausedEventTopic, &eventstypes.TaskPaused{ContainerID: id})
	} else {
		s.publish(ctx, runtime.TaskResumedEventTopic, &eventstypes.TaskResumed{ContainerID: id})
	}
	return &emptypb.Empty{}, nil
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
	if parent.paused.Load() {
		return nil, errgrpc.ToGRPCf(errdefs.ErrFailedPrecondition, "task is paused; resume it first")
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
	// A deleted exec's port first (TD-041); then the next unused one. 16,125 execs can be open at once.
	var port uint32
	if n := len(s.freePorts); n > 0 {
		port, s.freePorts = s.freePorts[n-1], s.freePorts[:n-1]
	} else if s.nextPort > 65528 {
		return nil, errgrpc.ToGRPCf(errdefs.ErrResourceExhausted, "guest session ports exhausted: 16,125 execs are open")
	} else {
		port, s.nextPort = s.nextPort, s.nextPort+4
	}
	t := newTask(r.ExecID, parent.Bundle)
	t.Config, t.Port = config, port
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
	// A paused guest would hold this lock through the resize deadline, and with it Resume and Kill.
	if task, _ := s.getTask(r.ID); task != nil && task.paused.Load() {
		return nil, errgrpc.ToGRPCf(errdefs.ErrFailedPrecondition, "task is paused; resume it first")
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
