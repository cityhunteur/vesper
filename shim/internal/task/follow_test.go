package task

import (
	"bytes"
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestFollowLogExitMarker(t *testing.T) {
	path := filepath.Join(t.TempDir(), "vm.log")
	var out bytes.Buffer
	done := make(chan [2]interface{})
	go func() {
		st, at := followLog(path, &out, func() bool { return true })
		done <- [2]interface{}{st, at}
	}()
	time.Sleep(150 * time.Millisecond) // the log does not exist yet: the follower must wait for it
	f, _ := os.Create(path)
	f.WriteString("VESPER-MICROVM-OK\nVESPER-ROOTFS-OK x\nVESPER-WORKLOAD-START\nhello\n")
	f.Sync()
	time.Sleep(250 * time.Millisecond)
	f.WriteString("wor")
	f.Sync()
	time.Sleep(150 * time.Millisecond)
	f.WriteString("ld\nVESPER-WORKLOAD-EXIT 3\n[    1.0] reboot: Power down\n")
	f.Close()
	select {
	case r := <-done:
		if r[0].(uint32) != 3 {
			t.Fatalf("exit status %v", r[0])
		}
	case <-time.After(5 * time.Second):
		t.Fatal("follower did not finish")
	}
	if out.String() != "hello\nworld\n" {
		t.Fatalf("output %q", out.String())
	}
}

func TestFollowLogVMMGone(t *testing.T) {
	path := filepath.Join(t.TempDir(), "vm.log")
	os.WriteFile(path, []byte("VESPER-MICROVM-OK\nVESPER-WORKLOAD-START\npartial\n"), 0o644)
	var out bytes.Buffer
	st, _ := followLog(path, &out, func() bool { return false })
	if st != exitKilled {
		t.Fatalf("expected %d, got %d", exitKilled, st)
	}
	if out.String() != "partial\n" {
		t.Fatalf("output %q", out.String())
	}
}
