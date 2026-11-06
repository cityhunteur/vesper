package task

import (
	"fmt"
	"sync"
	"time"

	"github.com/containerd/errdefs"
)

// Phase is the task state machine: created -> running -> stopped -> deleted, with kill allowed from
// created (the task exits without ever running) and running.
type Phase int

const (
	PhaseCreated Phase = iota
	PhaseRunning
	PhaseStopped
	PhaseDeleted
)

func (p Phase) String() string {
	switch p {
	case PhaseCreated:
		return "created"
	case PhaseRunning:
		return "running"
	case PhaseStopped:
		return "stopped"
	case PhaseDeleted:
		return "deleted"
	}
	return fmt.Sprintf("phase(%d)", int(p))
}

// Task is one container task; the shim serves exactly one.
type Task struct {
	mu         sync.Mutex
	ID         string
	Bundle     string
	Image      string
	CmdB64     string
	Stdout     string
	Stderr     string
	Stdin      string
	Terminal   bool
	phase      Phase
	pid        int
	exitStatus uint32
	exitedAt   time.Time
	exited     chan struct{}
}

func newTask(id, bundle string) *Task {
	return &Task{ID: id, Bundle: bundle, phase: PhaseCreated, exited: make(chan struct{})}
}

func (t *Task) Phase() Phase {
	t.mu.Lock()
	defer t.mu.Unlock()
	return t.phase
}

func (t *Task) Pid() int {
	t.mu.Lock()
	defer t.mu.Unlock()
	return t.pid
}

// MarkRunning records the VMM pid; only valid from created.
func (t *Task) MarkRunning(pid int) error {
	t.mu.Lock()
	defer t.mu.Unlock()
	if t.phase != PhaseCreated {
		return fmt.Errorf("task %s is %s, not created: %w", t.ID, t.phase, errdefs.ErrFailedPrecondition)
	}
	t.phase = PhaseRunning
	t.pid = pid
	return nil
}

// MarkExited records the exit once; later calls are ignored so a kill racing the natural exit is safe.
func (t *Task) MarkExited(status uint32, at time.Time) bool {
	t.mu.Lock()
	defer t.mu.Unlock()
	if t.phase == PhaseStopped || t.phase == PhaseDeleted {
		return false
	}
	t.phase = PhaseStopped
	t.exitStatus = status
	t.exitedAt = at
	close(t.exited)
	return true
}

// MarkDeleted is only valid from created or stopped.
func (t *Task) MarkDeleted() error {
	t.mu.Lock()
	defer t.mu.Unlock()
	switch t.phase {
	case PhaseRunning:
		return fmt.Errorf("task %s is running; kill it before delete: %w", t.ID, errdefs.ErrFailedPrecondition)
	case PhaseDeleted:
		return fmt.Errorf("task %s already deleted: %w", t.ID, errdefs.ErrNotFound)
	}
	if t.phase == PhaseCreated {
		t.exitStatus = 0
		t.exitedAt = time.Now()
		close(t.exited)
	}
	t.phase = PhaseDeleted
	return nil
}

// Exited is closed once the task has an exit status.
func (t *Task) Exited() <-chan struct{} { return t.exited }

func (t *Task) Exit() (uint32, time.Time) {
	t.mu.Lock()
	defer t.mu.Unlock()
	return t.exitStatus, t.exitedAt
}
