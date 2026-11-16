//go:build !linux

package runcwrap

import (
	"errors"
	"time"
)

// Refresh works in the System VM only.
func Refresh(Target, time.Duration) Result {
	return Result{Errors: []error{errors.New("refresh needs Linux")}}
}
