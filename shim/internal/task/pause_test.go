package task

import (
	"context"
	"errors"
	"syscall"
	"testing"

	taskAPI "github.com/containerd/containerd/api/runtime/task/v3"
	tasktypes "github.com/containerd/containerd/api/types/task"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"

	"github.com/cityhunteur/vesper/shim/internal/backend"
)

// pauseBackend records pause and signal calls (ADR 0011 slice 5); failResume makes resuming fail.
type pauseBackend struct {
	backend.Backend
	paused     []bool
	signals    []uint32
	failResume bool
}

func (b *pauseBackend) SetPaused(_ context.Context, _ string, paused bool) error {
	if !paused && b.failResume {
		return errors.New("firecracker does not answer")
	}
	b.paused = append(b.paused, paused)
	return nil
}

func (b *pauseBackend) Signal(_ context.Context, _ string, _, signal uint32, _ bool) error {
	b.signals = append(b.signals, signal)
	return nil
}

func (b *pauseBackend) Stop(context.Context, string) error { return nil }

func runningService(t *testing.T) (*Service, *pauseBackend) {
	t.Helper()
	b := &pauseBackend{}
	s, err := NewService(context.Background(), nil, nil, b)
	if err != nil {
		t.Fatal(err)
	}
	s.task = newTask("init", t.TempDir())
	if err := s.task.MarkRunning(4242); err != nil {
		t.Fatal(err)
	}
	return s, b
}

func state(t *testing.T, s *Service) tasktypes.Status {
	t.Helper()
	r, err := s.State(context.Background(), &taskAPI.StateRequest{ID: "init"})
	if err != nil {
		t.Fatal(err)
	}
	return r.Status
}

func TestPauseAndResume(t *testing.T) {
	ctx := context.Background()
	s, b := runningService(t)
	if _, err := s.Pause(ctx, &taskAPI.PauseRequest{ID: "init"}); err != nil {
		t.Fatal(err)
	}
	if got := state(t, s); got != tasktypes.Status_PAUSED {
		t.Fatalf("state %v after Pause", got)
	}
	if _, err := s.Pause(ctx, &taskAPI.PauseRequest{ID: "init"}); status.Code(err) != codes.FailedPrecondition {
		t.Errorf("a second Pause: %v", err)
	}
	if _, err := s.Exec(ctx, &taskAPI.ExecProcessRequest{ID: "init", ExecID: "e"}); status.Code(err) != codes.FailedPrecondition {
		t.Errorf("Exec while paused: %v", err)
	}
	if _, err := s.Resume(ctx, &taskAPI.ResumeRequest{ID: "init"}); err != nil {
		t.Fatal(err)
	}
	if got := state(t, s); got != tasktypes.Status_RUNNING {
		t.Fatalf("state %v after Resume", got)
	}
	if _, err := s.Resume(ctx, &taskAPI.ResumeRequest{ID: "init"}); status.Code(err) != codes.FailedPrecondition {
		t.Errorf("Resume when not paused: %v", err)
	}
	if len(b.paused) != 2 || !b.paused[0] || b.paused[1] {
		t.Errorf("backend calls %v", b.paused)
	}
}

// A signal to a paused task resumes it first; signal 0 is answered without resuming.
func TestKillWhilePaused(t *testing.T) {
	ctx := context.Background()
	s, b := runningService(t)
	_, _ = s.Pause(ctx, &taskAPI.PauseRequest{ID: "init"})
	if _, err := s.Kill(ctx, &taskAPI.KillRequest{ID: "init", Signal: 0}); err != nil {
		t.Fatal(err)
	}
	if got := state(t, s); got != tasktypes.Status_PAUSED || len(b.signals) != 0 {
		t.Fatalf("signal 0 changed the task: %v, signals %v", got, b.signals)
	}
	if _, err := s.Kill(ctx, &taskAPI.KillRequest{ID: "init", Signal: uint32(syscall.SIGTERM)}); err != nil {
		t.Fatal(err)
	}
	if got := state(t, s); got != tasktypes.Status_RUNNING || len(b.signals) != 1 || b.signals[0] != uint32(syscall.SIGTERM) {
		t.Fatalf("after SIGTERM: %v, signals %v", got, b.signals)
	}
}

// A SIGKILL to a paused task whose microVM cannot be resumed still succeeds: the bounded wait stops the VMM.
func TestKillWhenResumeFails(t *testing.T) {
	ctx := context.Background()
	s, b := runningService(t)
	_, _ = s.Pause(ctx, &taskAPI.PauseRequest{ID: "init"})
	b.failResume = true
	if _, err := s.Kill(ctx, &taskAPI.KillRequest{ID: "init", Signal: uint32(syscall.SIGTERM)}); err == nil {
		t.Error("SIGTERM succeeded though the microVM could not be resumed")
	}
	if _, err := s.Kill(ctx, &taskAPI.KillRequest{ID: "init", Signal: uint32(syscall.SIGKILL)}); err != nil {
		t.Errorf("SIGKILL failed: %v", err)
	}
}
