# Feasibility run of the upstream Firecracker suite in the System VM (GH #166, CE-0001)

This ExecPlan is a living document. The sections Progress, Surprises & Discoveries, Decision Log, and
Outcomes & Retrospective must be kept up to date as work proceeds. It is maintained in accordance with
docs/PLANS.md.

Tracker: GitHub #166. Time box: ended 2026-11-12. Follow-ups: GH #172 (script the run and a comparator), GH #173
(load `tun` at boot), TD-054 (userfaultfd). Decisions D-014, D-017, D-023.

## Purpose / Big Picture

The Phase 2 stage gate in [design-v0.md](../../specs/design-v0.md) requires Firecracker's upstream integration suite
to pass against Backend A, with the pass set defined by a Linux aarch64 baseline (D-014). The suite had never been
run here (CE-0001). This time-boxed trial found out whether it can run in the System VM at all, what fails and why,
and how the pass set should be produced. No product code changed.

## Progress

- [x] (2026-11-11) Upstream source at `d83d72b` (v1.16.0), `fcuvm:v90` and the CI artefacts staged on the System VM's
      data disk (GH #164 made the 4.3 GB image fit).
- [x] (2026-11-11) Harness proved on `test_api.py` (61 passed, 6 skipped, 2 errors once `tun` was loaded).
- [x] (2026-11-12) `integration_tests/functional` (891 results) and `integration_tests/security` (87).
- [x] (2026-11-12) Rerun of 53 tests after staging the missing tool binaries.
- [x] (2026-11-12) Every failure classified; outcome and procedure recorded (D-023); follow-ups opened.

## Surprises & Discoveries

- Observation: the suite runs unmodified in the System VM. It drives the stock jailer and Firecracker under nested
  KVM and boots both CI guest kernels (5.10.253, 6.1.172) with both root filesystems.
  Evidence: `run/GH-166/results/combined-classified.tsv` (local).
- Observation: an attached `docker run` lost output after about 1 s of silence through `run/vesper/docker.sock`;
  that was a Vesper bug (socat's 0.5 s half-close timeout), fixed in GH #167. Evidence: `run/GH-166/logs/attach-idle-repro.txt`.
- Observation: `tun` is not loaded at System VM boot (GH #173); the Alpine `virt` kernel has no
  `CONFIG_USERFAULTFD` (TD-054); the Apple virtual CPU exposes no cache geometry and no DMI.
- Observation: pytest's 300 s per-test timeout did not stop `test_vsock_transport_reset_g2h`; two workers hung for
  about 10 h 45 min until the run was interrupted. A full run needs a timeout that works.
- Observation: some upstream tests ignore `--binary-dir` and expect tools and `.debug` files in
  `build/cargo_target/aarch64-unknown-linux-musl/release`; upstream's `setup-ci-artifacts.sh` cannot run on a
  virtio-fs share (mode-0000 files), so the tree was staged on a data-disk volume.

## Decision Log

- Decision: run pytest in the upstream `fcuvm` container directly (reproducing `devtool`'s `docker run`), because the
  System VM has no bash for `devtool` itself. Date/Author: 2026-11-11, trial agent.
- Decision: raise the System VM to 4 vCPUs and 8 GiB for the trial only, then restore the defaults.
  Date/Author: 2026-11-11, trial agent.
- Decision: the pass set is produced on Linux, not on Backend A (D-023). Date/Author: 2026-11-12, maintainer agent.

## Outcomes & Retrospective

| Directory | Selected | Passed | Failed | Error | Skipped | Hung |
|---|---:|---:|---:|---:|---:|---:|
| integration_tests/functional | 893 | 683 | 84 | 31 | 93 | 2 |
| integration_tests/security | 87 | 72 | 12 | 0 | 3 | 0 |
| **Total** | **980** | **755** | **96** | **31** | **96** | **2** |

All 96 skips are for x86-only, cgroup-v1 or guest-kernel reasons that apply on Linux aarch64 too. The 129 failures,
errors and hangs fall into these classes:

| Tests | Class | Kind |
|---:|---|---|
| 52 | Firecracker's own memory (outside guest memory) 5.00–5.84 MiB against the test thresholds of 5 MiB booted and 7 MiB snapshot | unexplained |
| 24 | No cache size, line size or sets in the guest: the Apple virtual CPU exposes none | environment |
| 10 | `spec_store_bypass: Vulnerable` on host and guest | environment |
| 8 | userfaultfd snapshot load: no `CONFIG_USERFAULTFD` in the Alpine kernel (TD-054) | environment |
| 8 | Expected CPU features keyed on Neoverse N1, V1 and V2; Apple M3 is "Unknown" | test assumption |
| 7 | `clippy-tracing` and `log-instrument` examples not built | harness gap |
| 6 | Rate-limiter or balloon numbers just outside tolerance | unexplained (probably timing) |
| 5 | Guest SSH timed out under load (load average 28 on 4 vCPUs) | environment / timing |
| 3 | `test_vsock_transport_reset_g2h`: one failure and two hangs | unexplained |
| 2 | `test_attach_maximum_devices` over 300 s | environment / timing |
| 2 | EXDEV hard-linking `framework/vm_config.json` into the jail | unexplained |
| 1 | No DMI or SMBIOS | environment |
| 1 | One `KeyError` in 44 `test_vmclock_notifications` cases | unexplained |

No failure is shown to be a Firecracker-on-Backend-A difference. The 56 unexplained ones need the same tests on
Linux aarch64 (CE-0003) to tell "fails upstream too" from "Backend A differs". 43 tests cannot pass on this hardware
(cache geometry, SSB, the CPU feature list, DMI) and will be permanent Backend A exception classes.

Resources: 4 vCPUs and 8 GiB at `-n 3` (at least 3.55 GiB stayed free); about 67 min for the functional run before
the hang, 99 s for security; data disk peaked at 29.5 GB. A full run needs a working per-test timeout, `tun` at
boot, the tools staged and a `/srv` wiped per run, and takes an estimated 70–80 min.

Lessons: the data disk (GH #164) was the real prerequisite; the trial would not have fit in tmpfs. The trial found
a real Vesper bug (GH #167) on the way, which a Docker-path suite should have caught; `make docker-conformance` now
covers it.

## Context and Orientation

- CE-0001 and CE-0003 in [compat-exceptions.yaml](../../specs/compat-exceptions.yaml); decisions D-014, D-017 and
  D-023 in the [decision log](../../design-docs/decision-log.md).
- The System VM and its data disk: [systemvm-bringup.md](../../process/systemvm-bringup.md).
- Upstream: `tools/devtool`, `tests/README.md` and `tests/pytest.ini` at `d83d72b`.

## Plan of Work

Done as recorded in Progress. The scripted version of the harness and the comparator are GH #172.

## Concrete Steps

All commands are in `run/GH-166/commands.md` (local, not tracked), with the harness scripts beside it.

## Validation and Acceptance

Accepted when the per-test results, the failure classes, the resource estimate, the pass-set recommendation and
the follow-ups were recorded: this plan, D-023, CE-0001, TD-054, GH #172 and GH #173. `python3 harness/check_docs.py`
passes.

## Idempotence and Recovery

The trial changed nothing tracked. The upstream image and the source and cargo volumes remain on the data disk so a
rerun needs no downloads; the `/srv` scratch volume was removed. `make systemvm-down` and a regenerated launch
config return the System VM to its defaults.

## Artifacts and Notes

Under `run/GH-166/` (local, not tracked): `summary.md`, `commands.md`, the harness scripts, `classify.py` and
`tabulate.py`, `results/combined-classified.tsv` (980 rows) and per-run directories with junit, json and logs,
`logs/` (System VM environment, resources every 15 s, the attach repro).

## Interfaces and Dependencies

None changed. A full run uses `public.ecr.aws/firecracker/fcuvm` at the version `devtool` pins and the public CI
artefact bucket `spec.ccfc.min`; neither becomes a product dependency.
