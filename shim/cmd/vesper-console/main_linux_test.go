package main

import (
	"golang.org/x/sys/unix"
	"testing"
)

func TestExitCode(t *testing.T) {
	for _, tc := range []struct {
		status unix.WaitStatus
		want   int
	}{{0, 0}, {7 << 8, 7}, {unix.WaitStatus(unix.SIGKILL), 137}, {unix.WaitStatus(unix.SIGTERM), 143}} {
		if got := exitCode(tc.status); got != tc.want {
			t.Errorf("%v: got %d want %d", tc.status, got, tc.want)
		}
	}
}
