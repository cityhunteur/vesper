# Cross-architecture execution: linux/amd64 containers through binfmt_misc and qemu-user (WOR-55 / GH #39)

This ExecPlan is a living document. The sections Progress, Surprises & Discoveries, Decision Log, and
Outcomes & Retrospective must be kept up to date as work proceeds. It is maintained in accordance with
docs/PLANS.md.

## Purpose / Big Picture

GitHub #39 (Linear WOR-55, mirror #95) asks for `docker run --platform linux/amd64` on Apple Silicon through a
documented emulation path in the System VM, execution-mode metadata that says native or emulated, clear errors
for unsupported combinations, and acceptable performance. After this plan `make crossarch-suite` proves 9
checks from macOS and the workflow suite's amd64 step is required.

## Progress

- [x] (2026-11-08) `qemu-x86_64` 10.0.0 pinned in the packages lock (22 packages); guest `vesper-binfmt` (OpenRC,
  default runlevel) mounts `binfmt_misc` and registers `qemu-x86_64` for x86-64 ELF with flags `OCF`, so a
  container without qemu in its rootfs runs; guest `vesper-arch status|mode ARCH` (native arch, registered
  interpreters with flags and version; `MODE <arch> native|emulated:<interp>|unsupported (…supported: …)`);
  agent `ARCH`, `Session::arch`, `systemvmctl control arch`. `scripts/systemvm/crossarch-suite.sh` and
  `make crossarch-suite`: binfmt registered with `F`, modes surfaced for arm64/amd64/riscv64, `docker run
  --platform linux/amd64 busybox:1.37 uname -m` → `x86_64`, image and container inspect show `linux/amd64`
  against a `linux/arm64` host, an unsupported platform fails (`exec format error`) with `vesper-arch mode`
  naming the supported set, a 20k-iteration shell loop runs at 1.9–2.5× the native wall time including
  container start (limit 40×), and the native test image is left untouched. 9/9 on an M3 Pro. The workflow
  suite's `platform_amd64` step is now required and passes; conformance and DNS suites pass (the DNS
  determinism step now compares resolver and subnet, since service address order is not fixed).
- [x] (2026-11-08) PR #141 merged as `20c9c28`, CI green, GH #39 checklist and GH #31 item 6 ticked, mirror #95 closed, WOR-55 Done, milestone entry; plan moved to completed.

## Surprises & Discoveries

- Observation: `docker run --platform <other>` on an existing tag pulls that variant and re-tags the same
  reference, so a native image silently becomes foreign. Both suites probe foreign platforms on a separate
  image reference and verify the native image's architecture first and last.
- Observation: Docker's own message for a platform with no interpreter is just `exec format error`; the
  actionable text comes from `vesper-arch mode`.
- Observation: the DNS suite's determinism check was flaky because compose assigns service addresses in start
  order; the stable defaults are the resolver and the subnet.

## Decision Log

- Decision: register qemu through Vesper's own `vesper-binfmt` service rather than Alpine's `qemu-openrc`.
  Rationale: one line per interpreter with explicit flags (`OCF`), no dependency on another package's
  registration policy, and only installed interpreters are registered.
  Date/Author: 2026-11-08, agent.
- Decision: x86-64 is the only emulated architecture for now (TD-040).

## Outcomes & Retrospective

See Progress; `run/WOR-55/systemvm/crossarch-report.json` carries the 9 steps. Gaps: TD-040.

## Context and Orientation

`systemvm/overlay/etc/init.d/vesper-binfmt`, `systemvm/overlay/usr/local/bin/vesper-arch`,
`docs/specs/systemvm-packages.lock.yaml`, `scripts/systemvm/crossarch-suite.sh`.

## Plan of Work

Done in one slice; see Progress.

## Concrete Steps

    make systemvm-prepare && make systemvm-up && make docker-up
    make crossarch-suite
    cargo run --bin systemvmctl -- control arch status
    make docker-down && make systemvm-down

## Validation and Acceptance

`make crossarch-suite` with `passed: true`; `make workflow-suite` with the amd64 step required; conformance and
DNS suites; `cargo test`; docs and structure sensors; CI (the image workflow verifies the 22 pinned packages).

## Idempotence and Recovery

`vesper-binfmt start` skips interpreters already registered; `stop` unregisters them. The suite removes the
amd64 test image and restores the native one.

## Artifacts and Notes

    ok   binfmt_qemu_registered: INTERP qemu-x86_64 enabled flags=OCF interpreter=/usr/bin/qemu-x86_64 version=10.0.0
    ok   run_platform_amd64: docker run --platform linux/amd64 busybox:1.37 uname -m -> x86_64
    ok   unsupported_arch_mode_clear: MODE riscv64 unsupported (no binfmt_misc interpreter for riscv64; supported: aarch64 native, x86_64 emulated)
    ok   emulated_cost_within_limit: native 156 ms, emulated 302 ms, ratio 1.9x (limit 40x)

## Interfaces and Dependencies

Guest package `qemu-x86_64` (GPL-2.0, from the pinned Alpine community index); kernel `binfmt_misc`. No crate
dependencies.
