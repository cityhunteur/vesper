// containerd-shim-vesper-v2 is the containerd runtime v2 shim that runs each container as a jailed
// Firecracker microVM inside the Vesper System VM (design-v0 Phase 3, GH #22).
package main

import (
	"context"

	"github.com/cityhunteur/vesper/shim/internal/manager"
	_ "github.com/cityhunteur/vesper/shim/internal/task/plugin"
	"github.com/containerd/containerd/v2/pkg/shim"
)

func main() {
	// The shim waits on its own children (vesper-microvm, mke2fs) with exec.Cmd; the framework's
	// SIGCHLD reaper would race those waits ("waitid: no child processes"). MicroVM processes are
	// started with setsid by vesper-microvm and reparent to PID 1, so no subreaper is needed either.
	shim.Run(context.Background(), manager.New(manager.RuntimeName), func(c *shim.Config) {
		c.NoReaper = true
		c.NoSubreaper = true
	})
}
