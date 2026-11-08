//go:build linux

package task

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"strconv"
	"strings"

	"github.com/containerd/cgroups/v3/cgroup2/stats"
	taskAPI "github.com/containerd/containerd/api/runtime/task/v3"
	"github.com/containerd/errdefs"
	"github.com/containerd/errdefs/pkg/errgrpc"
	"github.com/containerd/typeurl/v2"
	"google.golang.org/protobuf/types/known/anypb"
)

const cgroupRoot = "/sys/fs/cgroup"

func init() {
	// ctr decodes task metrics by this URL (the cgroups v2 metrics message).
	if _, err := typeurl.TypeURL(&stats.Metrics{}); err != nil {
		typeurl.Register(&stats.Metrics{}, "io.containerd.cgroups.v2.Metrics")
	}
}

// cgroupPath returns the VMM's cgroup v2 path (e.g. "/firecracker/<id>") from /proc/<pid>/cgroup.
func cgroupPath(pid int) (string, error) {
	raw, err := os.ReadFile(fmt.Sprintf("/proc/%d/cgroup", pid))
	if err != nil {
		return "", err
	}
	for _, line := range strings.Split(string(raw), "\n") {
		if rest, ok := strings.CutPrefix(line, "0::"); ok && rest != "" {
			return rest, nil
		}
	}
	return "", fmt.Errorf("no cgroup v2 entry for pid %d", pid)
}

func readUint(dir, name string) uint64 {
	raw, err := os.ReadFile(filepath.Join(dir, name))
	if err != nil {
		return 0
	}
	v, _ := strconv.ParseUint(strings.TrimSpace(string(raw)), 10, 64)
	return v
}

func readKV(dir, name string) map[string]uint64 {
	out := map[string]uint64{}
	raw, err := os.ReadFile(filepath.Join(dir, name))
	if err != nil {
		return out
	}
	for _, line := range strings.Split(string(raw), "\n") {
		if k, v, ok := strings.Cut(line, " "); ok {
			n, _ := strconv.ParseUint(strings.TrimSpace(v), 10, 64)
			out[k] = n
		}
	}
	return out
}

// cgroupMetrics reads the jailer's cgroup v2 files directly (memory, cpu, pids) into containerd's
// metrics message; "max" limits are reported as 0 (unlimited), as the cgroups library does.
func cgroupMetrics(dir string) *stats.Metrics {
	mem := readKV(dir, "memory.stat")
	cpu := readKV(dir, "cpu.stat")
	m := &stats.Metrics{
		Memory: &stats.MemoryStat{
			Usage:       readUint(dir, "memory.current"),
			UsageLimit:  readUint(dir, "memory.max"),
			SwapUsage:   readUint(dir, "memory.swap.current"),
			Anon:        mem["anon"],
			File:        mem["file"],
			KernelStack: mem["kernel_stack"],
			Slab:        mem["slab"],
			Sock:        mem["sock"],
			Shmem:       mem["shmem"],
			FileMapped:  mem["file_mapped"],
			Pgfault:     mem["pgfault"],
			Pgmajfault:  mem["pgmajfault"],
		},
		CPU: &stats.CPUStat{
			UsageUsec:     cpu["usage_usec"],
			UserUsec:      cpu["user_usec"],
			SystemUsec:    cpu["system_usec"],
			NrPeriods:     cpu["nr_periods"],
			NrThrottled:   cpu["nr_throttled"],
			ThrottledUsec: cpu["throttled_usec"],
		},
		Pids: &stats.PidsStat{Current: readUint(dir, "pids.current"), Limit: readUint(dir, "pids.max")},
	}
	return m
}

// Stats reports the jailed microVM's cgroup v2 metrics as containerd's standard metrics message, so
// `ctr task metrics` and the metrics API work as with runc.
func (s *Service) Stats(ctx context.Context, r *taskAPI.StatsRequest) (*taskAPI.StatsResponse, error) {
	t, err := s.getTask(r.ID)
	if err != nil {
		return nil, err
	}
	pid := t.Pid()
	if t.Phase() != PhaseRunning || pid == 0 {
		return nil, errgrpc.ToGRPCf(errdefs.ErrNotFound, "task %s has no running microVM", t.ID)
	}
	path, err := cgroupPath(pid)
	if err != nil {
		return nil, errgrpc.ToGRPCf(errdefs.ErrNotFound, "cgroup of task %s: %v", t.ID, err)
	}
	dir := filepath.Join(cgroupRoot, path)
	if _, err := os.Stat(dir); err != nil {
		return nil, errgrpc.ToGRPCf(errdefs.ErrNotFound, "cgroup %s: %v", dir, err)
	}
	data, err := typeurl.MarshalAny(cgroupMetrics(dir))
	if err != nil {
		return nil, err
	}
	return &taskAPI.StatsResponse{Stats: &anypb.Any{TypeUrl: data.GetTypeUrl(), Value: data.GetValue()}}, nil
}
