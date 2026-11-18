# Council review of TD-048 (microVM size from the task's limits), 2026-11-18

Working paper (D-007). Target: the uncommitted change on `feat/td048-microvm-size`, base `e5b919a`.
Three reviewers, each in its own context, read the first version of the diff: correctness, security, reliability.
No critical or high finding. A separate judge then checked each disposition against commit `7b3daf0` (last section).

| ID | Severity | Finding | Disposition |
|---|---|---|---|
| REL-1 | medium | The guest formula (L + 128 + L/64) has no term for the 64 MiB bounce buffer of a guest over 2 GiB, nor for vCPUs | Confirmed by measurement and worse than claimed: the workload had less than its limit in all 21 cases (`measurements.txt` 4). Formula refitted (L + 152 + L/32 + 8 per vCPU + 112 above 2,048 MiB); 24 cases re-measured, 16 to 81 MiB over the limit (`measurements.txt` 5). The suite now asserts MemAvailable is at least the limit. |
| COR-1 | medium | A CPU quota is an upper bound, yet a quota above the System VM's CPUs was refused | Fixed: the quota's vCPUs are clamped to the System VM's CPUs (`SizeOf(spec, systemCPUs)`). A CPU set is still refused. The memory refusal is kept and recorded as a deviation in design-v0; no compat-exception entry was added, because those carry the owner's approval. |
| SEC-1 | low | The CPU quota held only inside the guest, where the workload can lift it | Fixed: `--cpu-quota` gives the microVM's cgroup `cpu.max` of the quota plus a quarter of a CPU. Suite step `lifted_quota_held_by_the_microvm_cgroup`: 1.75 CPUs used with the guest's quota lifted. |
| SEC-2, COR-2 | low | Two sums in `SizeOf` could wrap (a limit near the largest int64; a period near 2^64) | Fixed: the limit is compared before any sum; the period must be 1 ms to 1 s and the quota at least 1 ms; unit tests for each. |
| SEC-3, COR-3, REL-6 | low | `--mem-mib 0000300` passed the decimal check and was read as octal in the ceiling | Fixed: `number` refuses a leading zero; the ceiling is checked to be digits before the lock. |
| SEC-4 | low | `countCPUs` bounded each range but not the total | Fixed: refused as soon as the count passes 32; unit test with 200,000 ranges. |
| SEC-5 | low | One task can book the whole shared budget; no cap per task | Recorded (TD-048 residuals, SECURITY.md): needed before a less trusted client reaches the runtime. |
| SEC-6 | low | `machine` counted vCPUs from thread names the jailed process sets | Fixed: it counts the kernel-named `kvm-vcpu` descriptors. |
| COR-4 | low | Suite preconditions unchecked; the quota refusal path untested; no unit test that Start passes the size | Fixed: the suite reads the System VM's CPUs and free budget first; a quota of one CPU more than there are is a step; `TestStartPassesTheSizeToTheLaunch`. |
| COR-5, REL-5 | low | `ctr task metrics` reports the microVM's cgroup, so a limited task's limit reads as the ceiling | Recorded (TD-048 residuals, RELIABILITY.md, shim README). |
| REL-2 | low | Every memory death is exit 137, and cleanup removed the cgroup's OOM count | Partly fixed: `cleanup` writes a `VESPER-MICROVM-OOM` line before it removes the cgroup (read by inspection; no runtime test, because a guest cannot pass its ceiling on demand). No OOM event: recorded. |
| REL-3 | low | A limit is booked in full at launch and held until Delete | Recorded (RELIABILITY.md, D-051, design-v0 deviations). |
| REL-4 | low | The suite left detached tasks behind when interrupted | Fixed: a sweep before the first step and on any exit; a failed kill or delete fails the step. |

Checks run on the final tree: `go vet` and `go test ./...` in `shim/`; `cargo test`; `make systemvm-microvm-size-suite`
9 of 9 twice on the default System VM (2 CPUs, 2,048 MiB) and once with 6 CPUs and 8,192 MiB; shim suite 22, exec
suite 19, bounds suite and microVM shares suite 11, all passing (`regression/`).

`python3 harness/check_docs.py` reports 0 errors and 0 warnings.

## Judge

A fresh context that wrote neither the code nor the reviews read commit `7b3daf0`. It confirmed all 13 dispositions
(the measured margins, the CPU figure and the suite runs as the author's runtime claims), recomputed the guest sizes,
ceilings and CPU limits by hand, and found the change fit to merge. It added eight low findings, all applied in the
follow-up commit:

| ID | Finding | What was done |
|---|---|---|
| JDG-1 | `--cpu-quota` with no colon was read as QUOTA = PERIOD | The parser takes exactly one colon. |
| JDG-2 | The vCPU count matched a file the jail could name `…anon_inode:kvm-vcpu:9` | Each link is read and matched whole. |
| JDG-3 | The tracker gave the host peak as if measured on the refitted guest; `size.go` said 7.6 MiB a vCPU where single rows show 5 to 10 | Both reworded. The peak was not measured again. |
| JDG-4 | This paper named a judge section it did not have | This section. |
| JDG-5 | `scripts/systemvm/README.md` did not list the suite or the two size variables | Added. |
| JDG-6 | The suite accepted 32 to 64 CPUs, where its step 4 cannot pass | It takes 2 to 31. |
| JDG-7 | A quota of 10^9 µs or more passed Create and failed Start on the launcher's digit limit | The quota given to the launcher is at most the vCPUs' worth; the guest still gets the quota as asked. |
| JDG-8 | A comment said the 560 MiB block shows the cgroup, not the guest, killed `dd` | Comment corrected: the cgroup acts at 512 MiB, and the step asserts more than that was available. |

Not exercised at runtime: the `VESPER-MICROVM-OOM` line (REL-2).

Checked on the System VM after the follow-up: `vesper-microvm launch` refuses `--cpu-quota 50000`, `--cpu-quota` with no
value, `--cpu-quota 1:2:3`, `--cpu-quota 50000:0100000`, `--mem-mib 0000300` and `--vcpus 02`, each with its `ERR bad …`
line and nothing booked; the size suite passes 9 of 9 again.
