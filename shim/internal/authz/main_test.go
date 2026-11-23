package authz

import (
	"os"
	"path/filepath"
	"testing"
)

// TestMain points the store at a directory that does not exist: the host's own /var/lib/docker (a CI runner has one,
// unreadable) must never decide a test. Tests that need a store make one with store().
func TestMain(m *testing.M) {
	dir, err := os.MkdirTemp("", "authz")
	if err != nil {
		panic(err)
	}
	StoreRoots = func() []string { return []string{filepath.Join(dir, "none")} }
	code := m.Run()
	os.RemoveAll(dir)
	os.Exit(code)
}
