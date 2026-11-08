//go:build linux

package task

import (
	"os"
	"path/filepath"
	"testing"
)

func TestCgroupMetricsFromFiles(t *testing.T) {
	dir := t.TempDir()
	os.WriteFile(filepath.Join(dir, "memory.current"), []byte("1048576\n"), 0o644)
	os.WriteFile(filepath.Join(dir, "memory.max"), []byte("671088640\n"), 0o644)
	os.WriteFile(filepath.Join(dir, "memory.stat"), []byte("anon 4096\nfile 8192\n"), 0o644)
	os.WriteFile(filepath.Join(dir, "cpu.stat"), []byte("usage_usec 1500\nuser_usec 1000\nsystem_usec 500\n"), 0o644)
	os.WriteFile(filepath.Join(dir, "pids.current"), []byte("3\n"), 0o644)
	os.WriteFile(filepath.Join(dir, "pids.max"), []byte("max\n"), 0o644)
	m := cgroupMetrics(dir)
	if m.Memory.Usage != 1048576 || m.Memory.UsageLimit != 671088640 || m.Memory.Anon != 4096 || m.Memory.File != 8192 {
		t.Fatalf("memory: %+v", m.Memory)
	}
	if m.CPU.UsageUsec != 1500 || m.CPU.UserUsec != 1000 || m.CPU.SystemUsec != 500 {
		t.Fatalf("cpu: %+v", m.CPU)
	}
	if m.Pids.Current != 3 || m.Pids.Limit != 0 {
		t.Fatalf("pids: %+v", m.Pids)
	}
}
