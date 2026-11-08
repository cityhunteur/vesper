package task

import (
	"encoding/json"
	"os"
	"path/filepath"
	"time"
)

// StateFileName is the durable task record in the bundle: containerd reconnects to a live shim through
// its bootstrap record, and when the shim is gone containerd's `delete` reads this to report the true
// exit instead of guessing (GH #25).
const StateFileName = "vesper-task.json"

// State is the persisted view of a Task.
type State struct {
	ID         string    `json:"id"`
	Phase      string    `json:"phase"`
	Pid        int       `json:"pid"`
	Image      string    `json:"image"`
	ExitStatus uint32    `json:"exit_status"`
	ExitedAt   time.Time `json:"exited_at,omitempty"`
	ShimPid    int       `json:"shim_pid"`
	UpdatedAt  time.Time `json:"updated_at"`
}

// persist writes the task's current state atomically; failures are reported, never fatal.
func (t *Task) persist() error {
	status, at := t.Exit()
	s := State{ID: t.ID, Phase: t.Phase().String(), Pid: t.Pid(), Image: t.Image, ExitStatus: status, ExitedAt: at, ShimPid: os.Getpid(), UpdatedAt: time.Now().UTC()}
	raw, err := json.Marshal(s)
	if err != nil {
		return err
	}
	tmp := filepath.Join(t.Bundle, StateFileName+".tmp")
	if err := os.WriteFile(tmp, raw, 0o644); err != nil {
		return err
	}
	return os.Rename(tmp, filepath.Join(t.Bundle, StateFileName))
}

// ReadState loads the record from a bundle; a missing file yields os.ErrNotExist.
func ReadState(bundle string) (State, error) {
	var s State
	raw, err := os.ReadFile(filepath.Join(bundle, StateFileName))
	if err != nil {
		return s, err
	}
	return s, json.Unmarshal(raw, &s)
}
