package task

import (
	"testing"
	"time"

	"github.com/containerd/errdefs"
)

func TestLifecycle(t *testing.T) {
	tk := newTask("t1", "/b")
	if tk.Phase() != PhaseCreated {
		t.Fatal("new task must be created")
	}
	if err := tk.MarkDeleted(); err != nil {
		t.Fatalf("delete from created is allowed: %v", err)
	}
	select {
	case <-tk.Exited():
	default:
		t.Fatal("deleting a created task must settle waiters")
	}
	if err := tk.MarkDeleted(); !errdefs.IsNotFound(err) {
		t.Fatalf("second delete must be not found, got %v", err)
	}

	tk = newTask("t2", "/b")
	if err := tk.MarkRunning(42); err != nil || tk.Pid() != 42 {
		t.Fatalf("start: %v", err)
	}
	if err := tk.MarkRunning(43); !errdefs.IsFailedPrecondition(err) {
		t.Fatalf("second start must be a failed precondition, got %v", err)
	}
	if err := tk.MarkDeleted(); !errdefs.IsFailedPrecondition(err) {
		t.Fatalf("delete while running must be a failed precondition, got %v", err)
	}
	at := time.Now()
	if !tk.MarkExited(3, at) {
		t.Fatal("first exit must be recorded")
	}
	if tk.MarkExited(137, time.Now()) {
		t.Fatal("second exit must be ignored")
	}
	st, when := tk.Exit()
	if st != 3 || !when.Equal(at) || tk.Phase() != PhaseStopped {
		t.Fatalf("exit not recorded: %d %v %s", st, when, tk.Phase())
	}
	if err := tk.MarkDeleted(); err != nil || tk.Phase() != PhaseDeleted {
		t.Fatalf("delete after exit: %v", err)
	}
}
