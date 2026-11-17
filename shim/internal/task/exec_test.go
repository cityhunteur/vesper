package task

import (
	"context"
	"encoding/json"
	"errors"
	"github.com/cityhunteur/vesper/shim/internal/backend"
	"os"
	"path/filepath"
	"testing"
	"time"

	taskAPI "github.com/containerd/containerd/api/runtime/task/v3"
	specs "github.com/opencontainers/runtime-spec/specs-go"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
	"google.golang.org/protobuf/types/known/anypb"
)

func TestExecLifecycleDoesNotOverwriteInit(t *testing.T) {
	ctx := context.Background()
	s, err := NewService(ctx, nil, nil, nil)
	if err != nil {
		t.Fatal(err)
	}
	parent := newTask("init", t.TempDir())
	if err := parent.MarkRunning(42); err != nil {
		t.Fatal(err)
	}
	s.task = parent
	if err := parent.persist(); err != nil {
		t.Fatal(err)
	}
	before, err := os.ReadFile(filepath.Join(parent.Bundle, StateFileName))
	if err != nil {
		t.Fatal(err)
	}
	raw, _ := json.Marshal(&specs.Process{Args: []string{"/app", "one two"}, Cwd: "/"})
	request := &taskAPI.ExecProcessRequest{ID: "init", ExecID: "exec", Spec: &anypb.Any{Value: raw}, Stdin: "input", Stdout: "out", Stderr: "err"}
	if _, err := s.Exec(ctx, request); err != nil {
		t.Fatal(err)
	}
	if _, err := s.Exec(ctx, request); status.Code(err) != codes.AlreadyExists {
		t.Fatalf("duplicate exec: %v", err)
	}
	process, err := s.getProcess("init", "exec")
	if err != nil {
		t.Fatal(err)
	}
	if process.Port != 1032 || process.Stdin != "input" || process.Stdout != "out" || process.Stderr != "err" {
		t.Fatalf("wrong process: %+v", process)
	}
	if _, err := s.getProcess("wrong", "exec"); status.Code(err) != codes.NotFound {
		t.Fatalf("wrong parent: %v", err)
	}
	if _, err := s.CloseIO(ctx, &taskAPI.CloseIORequest{ID: "init", ExecID: "exec", Stdin: true}); err != nil {
		t.Fatal(err)
	}
	if !process.stdinClosed {
		t.Fatal("CloseIO before Start was forgotten")
	}
	if _, err := s.Kill(ctx, &taskAPI.KillRequest{ID: "init", ExecID: "exec", Signal: 9}); err != nil {
		t.Fatal(err)
	}
	wait, err := s.Wait(ctx, &taskAPI.WaitRequest{ID: "init", ExecID: "exec"})
	if err != nil || wait.ExitStatus != 137 {
		t.Fatalf("wait: %v %v", wait, err)
	}
	if _, err := s.Kill(ctx, &taskAPI.KillRequest{ID: "init", ExecID: "exec", Signal: 0}); status.Code(err) != codes.NotFound {
		t.Fatalf("signal0 on stopped process: %v", err)
	}
	if _, err := s.Delete(ctx, &taskAPI.DeleteRequest{ID: "init", ExecID: "exec"}); err != nil {
		t.Fatal(err)
	}
	after, err := os.ReadFile(filepath.Join(parent.Bundle, StateFileName))
	if err != nil {
		t.Fatal(err)
	}
	if string(before) != string(after) || parent.Phase() != PhaseRunning {
		t.Fatal("exec lifecycle changed the init recovery record")
	}
	if _, err := s.getProcess("init", "exec"); status.Code(err) != codes.NotFound {
		t.Fatalf("deleted exec remains: %v", err)
	}
}

type cleanupBackend struct{ backend.Backend }

func (cleanupBackend) Cleanup(context.Context, string, bool) error { return nil }

type shutdownProbe struct{ called bool }

func (s *shutdownProbe) Shutdown()                                  { s.called = true }
func (*shutdownProbe) RegisterCallback(func(context.Context) error) {}
func (*shutdownProbe) Done() <-chan struct{}                        { return nil }
func (*shutdownProbe) Err() error                                   { return nil }

func TestDeleteLeavesShutdownToContainerd(t *testing.T) {
	ctx := context.Background()
	sd := &shutdownProbe{}
	s, err := NewService(ctx, nil, sd, cleanupBackend{})
	if err != nil {
		t.Fatal(err)
	}
	s.task = newTask("init", t.TempDir())
	s.task.MarkExited(7, time.Now())
	if _, err := s.Delete(ctx, &taskAPI.DeleteRequest{ID: "init"}); err != nil {
		t.Fatal(err)
	}
	if sd.called {
		t.Fatal("Delete shut down ttrpc before its reply")
	}
	if _, err := s.Shutdown(ctx, &taskAPI.ShutdownRequest{ID: "init"}); err != nil {
		t.Fatal(err)
	}
	if !sd.called {
		t.Fatal("containerd Shutdown did not stop the shim")
	}
}

type killBackend struct {
	backend.Backend
	stopped chan string
	refuse  error
}

func (b killBackend) Signal(context.Context, string, uint32, uint32, bool) error { return b.refuse }
func (b killBackend) Stop(_ context.Context, id string) error                    { b.stopped <- id; return nil }

// A SIGKILLed init process that never reports EXIT gets its microVM stopped after KillGrace (GH #160 M5); one
// that does exit in time is left alone.
func TestKillFallbackStopsTheMicroVM(t *testing.T) {
	ctx := context.Background()
	old := KillGrace
	KillGrace = 50 * time.Millisecond
	defer func() { KillGrace = old }()
	b := killBackend{stopped: make(chan string, 2)}
	s, err := NewService(ctx, nil, &shutdownProbe{}, b)
	if err != nil {
		t.Fatal(err)
	}
	s.task = newTask("init", t.TempDir())
	if err := s.task.MarkRunning(42); err != nil {
		t.Fatal(err)
	}
	if _, err := s.Kill(ctx, &taskAPI.KillRequest{ID: "init", Signal: 9}); err != nil {
		t.Fatal(err)
	}
	select {
	case id := <-b.stopped:
		if id != "init" {
			t.Fatalf("stopped %q", id)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("the microVM was not stopped after the grace")
	}

	s.task = newTask("init", t.TempDir())
	if err := s.task.MarkRunning(43); err != nil {
		t.Fatal(err)
	}
	if _, err := s.Kill(ctx, &taskAPI.KillRequest{ID: "init", Signal: 9}); err != nil {
		t.Fatal(err)
	}
	s.task.MarkExited(137, time.Now())
	select {
	case <-b.stopped:
		t.Fatal("a task that exited in time had its microVM stopped")
	case <-time.After(200 * time.Millisecond):
	}
}

// A workload that exited by itself while its share server was dead leaves the guest stuck in the unmount before
// EXIT, refusing signals; a SIGKILL still fails, but the microVM is stopped after the grace (council REL-1). A
// refused signal other than SIGKILL starts nothing.
func TestKillFallbackWhenTheGuestRefusesTheSignal(t *testing.T) {
	ctx := context.Background()
	old := KillGrace
	KillGrace = 50 * time.Millisecond
	defer func() { KillGrace = old }()
	b := killBackend{stopped: make(chan string, 2), refuse: errors.New("signal p1028: ERR signal session-not-running")}
	s, err := NewService(ctx, nil, &shutdownProbe{}, b)
	if err != nil {
		t.Fatal(err)
	}
	s.task = newTask("init", t.TempDir())
	if err := s.task.MarkRunning(44); err != nil {
		t.Fatal(err)
	}
	if _, err := s.Kill(ctx, &taskAPI.KillRequest{ID: "init", Signal: 15}); err == nil {
		t.Fatal("a refused SIGTERM reported success")
	}
	select {
	case <-b.stopped:
		t.Fatal("a refused SIGTERM stopped the microVM")
	case <-time.After(200 * time.Millisecond):
	}
	if _, err := s.Kill(ctx, &taskAPI.KillRequest{ID: "init", Signal: 9}); err == nil {
		t.Fatal("a refused SIGKILL reported success")
	}
	select {
	case id := <-b.stopped:
		if id != "init" {
			t.Fatalf("stopped %q", id)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("the microVM was not stopped after a refused SIGKILL")
	}
}
