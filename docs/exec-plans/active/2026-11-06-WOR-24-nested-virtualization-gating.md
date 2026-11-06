# Nested virtualization detection and backend gating (WOR-24 / GH #17)

This ExecPlan is a living document. The sections Progress, Surprises & Discoveries, Decision Log, and
Outcomes & Retrospective must be kept up to date as work proceeds. It is maintained in accordance with
docs/PLANS.md.

## Purpose / Big Picture

GitHub #17 (Linear WOR-24, mirror #66) asks for a host capability probe, a clear fallback policy and user-facing
diagnostics so Backend A (stock Firecracker under nested KVM in the System VM) activates only on hosts that support
nested virtualization (macOS 15 or later on an Apple M3 or later), without crashing elsewhere. After this plan
`systemvmctl backend-select` prints the selection with diagnostics, the launcher enables nested virtualization per
the lock's policy, and the guest reports `/dev/kvm`.

## Progress

- [x] (2026-11-06 20:25Z) `vesper-vmlauncher --host-capabilities` (JSON: API availability, nested support,
  macOS, chip, model); launch config `nested_virtualization` (`auto`, `require`, `off`) applied through
  `VZGenericPlatformConfiguration`, with a `platform` status line and a clean exit 3 under `require` on an
  unsupported host; `src/platform.rs` `HostCapabilities`, `NestedPolicy`, `select_backend` with diagnostics and
  tests for M3, M1, pre-macOS-15 and `off`; `systemvmctl backend-select`; lock `boot.nested_virtualization: auto`;
  probe prints `VESPER-KVM-OK|ABSENT`; agent `HEALTH.kvm`; CI exercises the probe and selection on GitHub's
  M-series runner. Verified on an M3 Pro: `auto` boots with `/dev/kvm`, `off` without, `require` succeeds.
- [ ] PR merged, CI green (including the runner's unsupported-host path), GH #17 checklist and WOR-24 updated,
  milestone entry; plan moved to completed.

## Surprises & Discoveries

- Observation: the Alpine `virt` kernel ships `CONFIG_KVM=y`, so `/dev/kvm` appears as soon as the launcher enables
  nested virtualization; no guest change was needed beyond reporting it.
  Evidence: `config-6.12.115-0-virt` from the Alpine netboot directory; `VESPER-KVM-OK` in
  `run/WOR-24/systemvm/console-nested-auto.log`.
- Observation: the first Firecracker release that contains the pinned baseline revision
  `083c4cf6…` is v1.16.0 (v1.15.x diverged); relevant for WOR-31's binary provisioning.
  Evidence: `gh api …/compare/083c4cf…...v1.16.0` → `ahead_by 285, behind_by 0`.

## Decision Log

- Decision: the authoritative probe is the launcher (`VZGenericPlatformConfiguration.isNestedVirtualizationSupported`),
  parsed by Rust; `sysctl` strings are diagnostics only.
  Rationale: Apple's API is the only truthful answer; chip names are not a contract.
  Date/Author: 2026-11-06, agent.
- Decision: `auto` falls back to Backend B (designed, not implemented) with diagnostics rather than failing; `require`
  fails with the same diagnostics; `off` never enables nested virtualization.
  Rationale: the issue's "unsupported hosts route to fallback backend" with an honest `implemented: false`.
  Date/Author: 2026-11-06, agent.

## Outcomes & Retrospective

See Progress. M1/M2 behaviour is covered by unit tests and by the CI step on GitHub's macOS runner (which lacks
nested virtualization); no M1/M2 hardware was available to boot a guest.

## Context and Orientation

`src/platform.rs` (layer 1), `launcher/`, `docs/specs/systemvm-image.lock.yaml` (`boot.nested_virtualization`),
`systemvmctl backend-select`. Terms: *Backend A* and *Backend B* as in design-v0.

## Plan of Work

Done in one slice; see Progress.

## Concrete Steps

    cd <repo root>
    make vmlauncher-build
    build/vmlauncher/vesper-vmlauncher --host-capabilities
    cargo run --bin systemvmctl -- backend-select [--policy auto|require|off]
    make systemvm-up && cargo run --bin systemvmctl -- control health   # "kvm":true on an M3+

## Validation and Acceptance

`cargo test platform`, the three backend-select runs, boots under `auto`, `off` and `require` with the console
`VESPER-KVM-*` line and `HEALTH.kvm`, CI (`launcher-build` job runs the probe and selection), docs and structure
sensors. Issue exit: the four GH #17 checklist items proven.

## Idempotence and Recovery

All probes are read-only; a wrong policy value fails validation before any VM starts.

## Artifacts and Notes

    $ cargo run -q --bin systemvmctl -- backend-select
    {"backend":"backend-a","nested_virtualization":true,"implemented":true,"diagnostics":[
      "host: Apple M3 Pro (Mac15,6), macOS 27.0.1",
      "nested virtualization supported: Backend A (stock Firecracker in the System VM) is available"]}
    $ cargo run -q --bin systemvmctl -- control health
    {"containerd_ok":true,"docker_ok":true,"docker_version":"28.3.3","kvm":true,...}

## Interfaces and Dependencies

`vesper::platform::{HostCapabilities, parse_host_capabilities, probe_host_capabilities, NestedPolicy, Backend,
BackendSelection, select_backend}`; `LaunchConfig.nested_virtualization`; lock schema 6. No new dependencies.
