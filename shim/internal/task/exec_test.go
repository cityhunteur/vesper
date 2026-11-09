package task

import (
	"context"
	"encoding/json"
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

func (cleanupBackend) Cleanup(context.Context, string) error { return nil }

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
