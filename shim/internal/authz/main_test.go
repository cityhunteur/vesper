package authz

import (
	"os"
	"path/filepath"
	"testing"
)

// TestMain points the store at a directory that does not exist: the host's own /var/lib/docker (a CI runner has one,
// unreadable) must never decide a test. Tests that need a store make one with store(). The daemon's configuration is
// the System VM's, with vesper-runc the default; tests of the default runtime write their own with daemonDefault().
func TestMain(m *testing.M) {
	dir, err := os.MkdirTemp("", "authz")
	if err != nil {
		panic(err)
	}
	StoreRoots = func() []string { return []string{filepath.Join(dir, "none")} }
	DaemonConfig = filepath.Join(dir, "daemon.json")
	if err := os.WriteFile(DaemonConfig, []byte(`{"default-runtime":"vesper-runc"}`), 0o600); err != nil {
		panic(err)
	}
	code := m.Run()
	os.RemoveAll(dir)
	os.Exit(code)
}
