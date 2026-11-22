//go:build !linux

package ctrns

import (
	"context"
	"errors"
	"path/filepath"
	"time"
)

const Dir = "/run/vesper/microvm/ctrns"

const HoldArg = "__vesper_ctrns_hold"

var errLinuxOnly = errors.New("a Docker task's network namespace needs Linux")

func Hold() {
	for {
		time.Sleep(time.Hour)
	}
}

func Path(id string) string { return filepath.Join(Dir, id) }

func Start(context.Context, string, func()) (int, error) { return 0, errLinuxOnly }

func Release(context.Context, string) error { return nil }

func Exists(string) bool { return false }
