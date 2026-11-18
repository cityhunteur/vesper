package bundle

import (
	"fmt"
	"runtime"
	"strconv"
	"strings"

	specs "github.com/opencontainers/runtime-spec/specs-go"
)

// Size is the machine a task's microVM is launched with (TD-048, D-051). The OCI resources decide it: a memory
// limit makes the guest large enough for the workload to reach that limit, and a CPU quota or a CPU set gives the
// guest that many vCPUs. The workload's cgroup inside the guest carries the limits too (GuestResources), so the
// workload meets them where a container would. That cgroup is not a bound against the workload, which is root in
// its guest and can lift it: the bounds are the guest's memory and vCPUs and the limits vesper-microvm sets on
// the microVM's own cgroup in the System VM, the memory ceiling and, from CPUQuota, a CPU limit.
type Size struct {
	MemMiB int
	VCPUs  int
	// CPUQuota is what the microVM's own cgroup is held to, in microseconds for each CPUPeriod: the task's CFS quota,
	// at most its vCPUs' worth, which is all the microVM can use. Both are zero when the task has no quota.
	CPUQuota  uint64
	CPUPeriod uint64
}

const (
	// DefaultMemMiB and DefaultVCPUs are the machine of a task whose spec sets no limit.
	DefaultMemMiB = 256
	DefaultVCPUs  = 1
	// MaxVCPUs is Firecracker's limit; vesper-microvm also refuses more than the System VM has.
	MaxVCPUs = 32
	// What the guest needs beside the workload, fitted to MemAvailable as the workload read it in guests for limits
	// of 512 MiB to 4 GiB with 1, 4 and 6 vCPUs (run/TD-048/measurements.txt, 2026-11-18). The terms were chosen
	// so that MemAvailable was above the limit by 16 MiB or more in all 24 cases measured again with them.
	//
	// guestBaseMiB: the kernel, the unpacked initramfs and the session tools (151 MiB at a 512 MiB limit).
	guestBaseMiB = 152
	// guestPerMiB: one MiB more for every guestPerMiB MiB of limit, for the kernel's page array and the memory it
	// keeps free (overhead grew by 23 MiB from a 512 MiB limit to a 1536 MiB one).
	guestPerMiB = 32
	// guestPerVCPUMiB: for each vCPU after the first (5 to 10 MiB in single rows of the measurements).
	guestPerVCPUMiB = 8
	// guestHighMiB: a guest over guestHighAtMiB has memory above 4 GiB, because its RAM starts at 2 GiB. The kernel
	// then keeps a 64 MiB bounce buffer, and about 40 MiB more went at the same step.
	guestHighMiB, guestHighAtMiB = 112, 2048
	// maxLimitMiB bounds the arithmetic; the System VM's memory budget refuses a microVM long before it.
	maxLimitMiB = 1 << 20
	mib         = 1 << 20
	// The kernel accepts a CFS period from 1 ms to 1 s and a quota of at least 1 ms.
	minCPUPeriod, maxCPUPeriod, minCPUQuota = 1000, 1000000, 1000
)

// SizeOf reads the machine from a spec. systemCPUs is how many CPUs the System VM has (zero when unknown).
//
// A memory limit and a CPU set are refused when they cannot be honoured: the workload could not reach the limit in
// a smaller guest, and a set names CPUs that must exist. A CPU quota is only an upper bound, so a quota of more
// CPUs than there are gets as many vCPUs as there are, as a container with that quota would (council COR-1).
func SizeOf(spec *specs.Spec, systemCPUs int) (Size, error) {
	s := Size{MemMiB: DefaultMemMiB, VCPUs: DefaultVCPUs}
	if spec == nil || spec.Linux == nil || spec.Linux.Resources == nil {
		return s, nil
	}
	r := spec.Linux.Resources
	if err := s.readCPUs(r, systemCPUs); err != nil {
		return Size{}, err
	}
	if most := uint64(s.VCPUs) * s.CPUPeriod; s.CPUQuota > most { // also keeps the number short for the launcher
		s.CPUQuota = most
	}
	perVCPU := int64(s.VCPUs-1) * guestPerVCPUMiB
	s.MemMiB = DefaultMemMiB + int(perVCPU)
	if l := memoryLimit(r); l > 0 {
		if l > maxLimitMiB*mib { // before any sum, so no limit can wrap into a small machine
			return Size{}, fmt.Errorf("memory limit %d bytes is too large (at most %d MiB)", l, maxLimitMiB)
		}
		lm := (l + mib - 1) / mib
		m := lm + guestBaseMiB + (lm+guestPerMiB-1)/guestPerMiB + perVCPU
		if m > guestHighAtMiB {
			m += guestHighMiB
		}
		if int(m) > s.MemMiB {
			s.MemMiB = int(m)
		}
	}
	return s, nil
}

// readCPUs sets the vCPUs and the quota from the spec's CPU resources.
func (s *Size) readCPUs(r *specs.LinuxResources, systemCPUs int) error {
	if r.CPU == nil {
		return nil
	}
	if r.CPU.Quota != nil && *r.CPU.Quota > 0 {
		q, p := uint64(*r.CPU.Quota), cpuPeriod(r)
		if p < minCPUPeriod || p > maxCPUPeriod || q < minCPUQuota {
			return fmt.Errorf("a CPU quota of %d in each %d µs is outside what the kernel takes (a period of %d to %d µs, a quota of at least %d)", q, p, minCPUPeriod, maxCPUPeriod, minCPUQuota)
		}
		n := q / p
		if q%p != 0 {
			n++
		}
		most := uint64(MaxVCPUs)
		if systemCPUs > 0 && uint64(systemCPUs) < most {
			most = uint64(systemCPUs)
		}
		if n > most {
			n = most
		}
		s.VCPUs, s.CPUQuota, s.CPUPeriod = int(n), q, p
	}
	if r.CPU.Cpus != "" { // the set decides how many CPUs the workload sees, the quota how much of them it gets
		n, err := countCPUs(r.CPU.Cpus)
		if err != nil {
			return err
		}
		s.VCPUs = n
	}
	return nil
}

// SystemCPUs is how many CPUs this process may run on. The shim runs in the System VM, so it is the System VM's
// count, the most vCPUs vesper-microvm gives a microVM.
func SystemCPUs() int { return runtime.NumCPU() }

func memoryLimit(r *specs.LinuxResources) int64 {
	if r.Memory == nil || r.Memory.Limit == nil {
		return 0
	}
	return *r.Memory.Limit
}

// cpuPeriod is the spec's CFS period, or the kernel's default of 100 ms.
func cpuPeriod(r *specs.LinuxResources) uint64 {
	if r.CPU.Period != nil && *r.CPU.Period > 0 {
		return *r.CPU.Period
	}
	return 100000
}

// countCPUs counts the CPUs in a cpuset list such as "0-2,4".
func countCPUs(list string) (int, error) {
	bad := func() (int, error) { return 0, fmt.Errorf("CPU set %q is not a list such as 0-2,4", list) }
	seen := map[int]bool{}
	for _, part := range strings.Split(list, ",") {
		lo, hi, isRange := strings.Cut(strings.TrimSpace(part), "-")
		a, err := strconv.Atoi(lo)
		if err != nil || a < 0 {
			return bad()
		}
		b := a
		if isRange {
			if b, err = strconv.Atoi(hi); err != nil || b < a {
				return bad()
			}
		}
		// Refused as soon as the count passes the limit, so no list, however long, makes this loop or the map large.
		for c := a; c <= b; c++ {
			if seen[c] = true; len(seen) > MaxVCPUs {
				return 0, fmt.Errorf("CPU set %q names more than %d CPUs; a microVM has at most %d", list, MaxVCPUs, MaxVCPUs)
			}
			if c == b { // b may be the largest int
				break
			}
		}
	}
	return len(seen), nil
}

// LaunchArgs are the vesper-microvm launch arguments for a size: none for the default machine.
func (s Size) LaunchArgs() []string {
	var a []string
	if s.MemMiB != DefaultMemMiB {
		a = append(a, "--mem-mib", strconv.Itoa(s.MemMiB))
	}
	if s.VCPUs != DefaultVCPUs {
		a = append(a, "--vcpus", strconv.Itoa(s.VCPUs))
	}
	if s.CPUQuota != 0 {
		a = append(a, "--cpu-quota", strconv.FormatUint(s.CPUQuota, 10)+":"+strconv.FormatUint(s.CPUPeriod, 10))
	}
	return a
}

// GuestResources are the limits runc applies to the workload's cgroup inside the guest: the memory limit and the CPU
// quota with its period. Nothing else is carried. A CPU set names the System VM's CPUs, not the guest's, and is
// answered by the vCPU count; device rules are not the guest's to apply (the microVM is the boundary, ADR 0006);
// the process limit, swap, weights and the other controllers have no runtime test yet (TD-048 residuals). It
// returns nil when the spec sets neither.
func GuestResources(spec *specs.Spec) *specs.LinuxResources {
	if spec == nil || spec.Linux == nil || spec.Linux.Resources == nil {
		return nil
	}
	r, out := spec.Linux.Resources, &specs.LinuxResources{}
	any := false
	if l := memoryLimit(r); l > 0 {
		out.Memory, any = &specs.LinuxMemory{Limit: &l}, true
	}
	if r.CPU != nil && r.CPU.Quota != nil && *r.CPU.Quota > 0 {
		q, p := *r.CPU.Quota, cpuPeriod(r)
		out.CPU, any = &specs.LinuxCPU{Quota: &q, Period: &p}, true
	}
	if !any {
		return nil
	}
	return out
}
