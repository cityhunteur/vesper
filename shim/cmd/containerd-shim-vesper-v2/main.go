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
	shim.Run(context.Background(), manager.New(manager.RuntimeName))
}
