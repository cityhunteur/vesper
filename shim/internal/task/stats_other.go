//go:build !linux

package task

import (
	"context"

	taskAPI "github.com/containerd/containerd/api/runtime/task/v3"
)

// Stats needs cgroup v2 and only exists on Linux; this stub keeps the package buildable for host-side tests.
func (s *Service) Stats(context.Context, *taskAPI.StatsRequest) (*taskAPI.StatsResponse, error) {
	return nil, notImplemented("stats (non-linux build)")
}
