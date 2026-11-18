package bundle

import (
	"encoding/json"
	"math"
	"strconv"
	"strings"
	"testing"
	"time"

	specs "github.com/opencontainers/runtime-spec/specs-go"
)

func resources(mem, quota int64, period uint64, cpus string, pids int64) *specs.Spec {
	r := &specs.LinuxResources{}
	if mem != 0 {
		r.Memory = &specs.LinuxMemory{Limit: &mem}
	}
	if quota != 0 || period != 0 || cpus != "" {
		r.CPU = &specs.LinuxCPU{Cpus: cpus}
		if quota != 0 {
			r.CPU.Quota = &quota
		}
		if period != 0 {
			r.CPU.Period = &period
		}
	}
	if pids != 0 {
		r.Pids = &specs.LinuxPids{Limit: &pids}
	}
	return &specs.Spec{Linux: &specs.Linux{Resources: r}}
}

// The machine follows the spec's limits; a spec without them keeps the default machine and adds no argument
// (TD-048).
func TestSizeOf(t *testing.T) {
	const m = 1 << 20
	for _, c := range []struct {
		name string
		spec *specs.Spec
		want Size
		args string
	}{
		{"no linux section", &specs.Spec{}, Size{256, 1, 0, 0}, ""},
		{"nil spec", nil, Size{256, 1, 0, 0}, ""},
		{"empty resources", resources(0, 0, 0, "", 0), Size{256, 1, 0, 0}, ""},
		{"unlimited memory", resources(-1, -1, 100000, "", -1), Size{256, 1, 0, 0}, ""},
		// 64 MiB fits the default guest, whose workload has about 149 MiB. Each vCPU after the first adds 8 MiB.
		{"a small limit keeps the default guest", resources(64*m, 0, 0, "", 0), Size{256, 1, 0, 0}, ""},
		{"128 MiB", resources(128*m, 0, 0, "", 0), Size{284, 1, 0, 0}, "--mem-mib 284"},
		{"512 MiB", resources(512*m, 0, 0, "", 0), Size{680, 1, 0, 0}, "--mem-mib 680"},
		{"a limit that is not whole MiB rounds up", resources(512*m+1, 0, 0, "", 0), Size{682, 1, 0, 0}, "--mem-mib 682"},
		// A guest over 2048 MiB gets 112 MiB more, for the bounce buffer the kernel keeps then.
		{"the largest limit under the step", resources(1838*m, 0, 0, "", 0), Size{2048, 1, 0, 0}, "--mem-mib 2048"},
		{"the first limit over the step", resources(1839*m, 0, 0, "", 0), Size{2161, 1, 0, 0}, "--mem-mib 2161"},
		{"4 GiB", resources(4096*m, 0, 0, "", 0), Size{4488, 1, 0, 0}, "--mem-mib 4488"},
		{"half a CPU", resources(0, 50000, 100000, "", 0), Size{256, 1, 50000, 100000}, "--cpu-quota 50000:100000"},
		{"one and a half CPUs", resources(0, 150000, 100000, "", 0), Size{264, 2, 150000, 100000}, "--mem-mib 264 --vcpus 2 --cpu-quota 150000:100000"},
		{"two CPUs, default period", resources(0, 200000, 0, "", 0), Size{264, 2, 200000, 100000}, "--mem-mib 264 --vcpus 2 --cpu-quota 200000:100000"},
		{"a period of its own", resources(0, 30000, 10000, "", 0), Size{272, 3, 30000, 10000}, "--mem-mib 272 --vcpus 3 --cpu-quota 30000:10000"},
		{"a CPU set", resources(0, 0, 0, "0-2,4", 0), Size{280, 4, 0, 0}, "--mem-mib 280 --vcpus 4"},
		{"a CPU set decides the count, the quota the share", resources(0, 50000, 100000, "0,1", 0), Size{264, 2, 50000, 100000}, "--mem-mib 264 --vcpus 2 --cpu-quota 50000:100000"},
		{"a CPU set naming one CPU twice", resources(0, 0, 0, "1,1,0-1", 0), Size{264, 2, 0, 0}, "--mem-mib 264 --vcpus 2"},
		{"both", resources(1024*m, 400000, 100000, "", 0), Size{1232, 4, 400000, 100000}, "--mem-mib 1232 --vcpus 4 --cpu-quota 400000:100000"},
		// Only the count matters, so the loop must end at the largest int without wrapping.
		{"a set at the largest numbers", resources(0, 0, 0, "9223372036854775800-9223372036854775807", 0), Size{312, 8, 0, 0}, "--mem-mib 312 --vcpus 8"},
		{"the largest limit", resources(maxLimitMiB*m, 0, 0, "", 0), Size{1048576 + 152 + 32768 + 112, 1, 0, 0}, "--mem-mib 1081608"},
	} {
		got, err := SizeOf(c.spec, 0)
		if err != nil || got != c.want {
			t.Errorf("%s: %+v, %v; want %+v", c.name, got, err, c.want)
		}
		if a := strings.Join(got.LaunchArgs(), " "); a != c.args {
			t.Errorf("%s: arguments %q, want %q", c.name, a, c.args)
		}
	}
}

// A quota is an upper bound: more CPUs than the System VM has, or than a microVM can have, gives as many vCPUs as
// there are and keeps the quota. A CPU set is not clamped (council COR-1).
func TestSizeOfClampsAQuota(t *testing.T) {
	for _, c := range []struct {
		quota  int64
		system int
		want   Size
	}{
		// The quota the microVM's cgroup is held to is at most its vCPUs' worth.
		{400000, 2, Size{264, 2, 200000, 100000}},
		{400000, 8, Size{280, 4, 400000, 100000}},
		{3300000, 64, Size{504, 32, 3200000, 100000}},
		{3300000, 0, Size{504, 32, 3200000, 100000}},
		{150000, 1, Size{256, 1, 100000, 100000}},
		// A quota too long for the launcher's argument is clamped like any other (council JDG-7).
		{10000000000, 4, Size{280, 4, 400000, 100000}},
	} {
		if got, err := SizeOf(resources(0, c.quota, 100000, "", 0), c.system); err != nil || got != c.want {
			t.Errorf("quota %d on %d CPUs: %+v, %v; want %+v", c.quota, c.system, got, err, c.want)
		}
	}
	// The guest still gets the quota as asked.
	if r := GuestResources(resources(0, 10000000000, 100000, "", 0)); r == nil || *r.CPU.Quota != 10000000000 {
		t.Errorf("guest quota: %+v", r)
	}
	if got, err := SizeOf(resources(0, 0, 0, "0-3", 0), 2); err != nil || got.VCPUs != 4 {
		t.Errorf("a CPU set of 4 on 2 CPUs: %+v, %v; the launcher refuses it, not the shim", got, err)
	}
}

func TestSizeOfRefuses(t *testing.T) {
	for name, spec := range map[string]*specs.Spec{
		"a set of 33 CPUs":    resources(0, 0, 0, "0-32", 0),
		"a huge range":        resources(0, 0, 0, "0-2000000000", 0),
		"a reversed range":    resources(0, 0, 0, "3-1", 0),
		"a set with a letter": resources(0, 0, 0, "0,a", 0),
		"a negative CPU":      resources(0, 0, 0, "-1", 0),
		"an empty element":    resources(0, 0, 0, "0,,1", 0),
		"a limit past 1 TiB":  resources(1<<41, 0, 0, "", 0),
		// Sums that would wrap: the largest limit, cgroup v1's "no limit", the largest period.
		"the largest int64":   resources(math.MaxInt64, 0, 0, "", 0),
		"cgroup v1 unlimited": resources(9223372036854771712, 0, 0, "", 0),
		"the largest period":  resources(0, 100000, math.MaxUint64, "", 0),
		"a period under 1 ms": resources(0, 100000, 999, "", 0),
		"a period over 1 s":   resources(0, 100000, 1000001, "", 0),
		"a quota under 1 ms":  resources(0, 999, 100000, "", 0),
	} {
		if got, err := SizeOf(spec, 0); err == nil {
			t.Errorf("%s: accepted as %+v", name, got)
		}
	}
}

// The guest applies the memory limit and the CPU quota, and nothing else of the spec.
func TestGuestResources(t *testing.T) {
	if r := GuestResources(resources(-1, -1, 100000, "0-1", -1)); r != nil {
		t.Errorf("no limit set, got %+v", r)
	}
	if r := GuestResources(&specs.Spec{}); r != nil {
		t.Errorf("no linux section, got %+v", r)
	}
	spec := resources(512<<20, 150000, 0, "0-1", 100)
	swap, shares := int64(1<<30), uint64(512)
	spec.Linux.Resources.Memory.Swap = &swap
	spec.Linux.Resources.CPU.Shares = &shares
	spec.Linux.Resources.Devices = []specs.LinuxDeviceCgroup{{Allow: false, Access: "rwm"}}
	spec.Linux.Resources.Unified = map[string]string{"memory.high": "1"}
	raw, _ := json.Marshal(GuestResources(spec))
	if want := `{"memory":{"limit":536870912},"cpu":{"quota":150000,"period":100000}}`; string(raw) != want {
		t.Errorf("guest resources %s, want %s", raw, want)
	}
	// They reach the guest's config.
	cfg, err := GuestConfig(&specs.Process{Args: []string{"true"}}, nil, GuestResources(spec))
	if err != nil || !strings.Contains(string(cfg), `"resources":{"memory":{"limit":536870912}`) {
		t.Errorf("guest config %s, %v", cfg, err)
	}
	// A spec without limits leaves the guest's config as it was before TD-048.
	cfg, _ = GuestConfig(&specs.Process{Args: []string{"true"}}, nil, GuestResources(&specs.Spec{}))
	if strings.Contains(string(cfg), "resources") {
		t.Errorf("guest config names resources without a limit: %s", cfg)
	}
}

// A CPU set is refused as soon as it names too many CPUs, however long the list is.
func TestCountCPUsIsBounded(t *testing.T) {
	var b strings.Builder
	for i := 0; i < 200000; i++ { // 200,000 ranges of 1,000 CPUs each, none overlapping
		if i > 0 {
			b.WriteByte(',')
		}
		b.WriteString(strconv.Itoa(i*1000) + "-" + strconv.Itoa(i*1000+999))
	}
	start := time.Now()
	if n, err := countCPUs(b.String()); err == nil {
		t.Fatalf("accepted %d CPUs", n)
	}
	if d := time.Since(start); d > 2*time.Second {
		t.Fatalf("took %s", d)
	}
	if n, err := countCPUs("0-31"); err != nil || n != MaxVCPUs {
		t.Fatalf("0-31: %d, %v", n, err)
	}
}
