package task

import (
	"os"
	"testing"
	"time"
)

func TestPersistAndReadState(t *testing.T) {
	dir := t.TempDir()
	tk := newTask("p1", dir)
	tk.Image = "/img"
	if err := tk.persist(); err != nil {
		t.Fatal(err)
	}
	s, err := ReadState(dir)
	if err != nil || s.Phase != "created" || s.ID != "p1" || s.ShimPid != os.Getpid() {
		t.Fatalf("created state: %+v %v", s, err)
	}
	tk.MarkRunning(77)
	tk.persist()
	if s, _ = ReadState(dir); s.Phase != "running" || s.Pid != 77 {
		t.Fatalf("running state: %+v", s)
	}
	tk.MarkExited(9, time.Now())
	tk.persist()
	if s, _ = ReadState(dir); s.Phase != "stopped" || s.ExitStatus != 9 || s.ExitedAt.IsZero() {
		t.Fatalf("stopped state: %+v", s)
	}
	if _, err := ReadState(t.TempDir()); !os.IsNotExist(err) {
		t.Fatalf("missing state must be not-exist: %v", err)
	}
}
