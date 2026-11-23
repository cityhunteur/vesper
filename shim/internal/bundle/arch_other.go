//go:build !linux

package bundle

import specs "github.com/opencontainers/runtime-spec/specs-go"

// CheckArch needs Linux's openat2; elsewhere no microVM runs, so there is nothing to check.
func CheckArch(string, *specs.Process) error { return nil }
