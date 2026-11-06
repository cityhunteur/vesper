---
status: accepted
owner: cityhunteur
last_reviewed: 2026-11-06
---

# 0001. Boot the System VM through a thin Swift Virtualization.framework helper

Date: 2026-11-06. Supersedes: none. Superseded by: none.

## Context and problem

Phase 1 (design-v0, GitHub #12, Linear WOR-27) needs a process that boots the pinned Linux System VM on
macOS with Virtualization.framework, NAT networking and a serial console, so that boot reliability can be
measured. The crate is Rust and has no bindings to Virtualization.framework. Any launcher must carry the
`com.apple.security.virtualization` entitlement, which means a signed binary. design-v0 states "Rust preferred;
Swift allowed only as FFI shim" and AGENTS.md requires a human decision for new runtime dependencies and for
anything that touches entitlements. The repository owner chose the Swift helper on 2026-11-06.

## Decision

We will boot the System VM with `vesper-vmlauncher`, a small Swift executable under `launcher/` that reads the
JSON launch config written by `systemvmctl launch-config`, builds the `VZVirtualMachineConfiguration`, streams the
console to the configured log, and exits with a status code as soon as the configured ready pattern appears.
All policy (which image, which command line, what "ready" means, how many boots, timing) stays in Rust and in
`docs/specs/systemvm-image.lock.yaml`; the helper has no defaults of its own beyond the config file path.

## Alternatives considered

- **Rust bindings (`objc2-virtualization`)**: one binary and reusable for Backend B, but a new runtime dependency
  whose coverage of Virtualization.framework was not verified, and `systemvmctl` itself would then need the
  entitlement and signing step.
- **External launcher (`vfkit`, `limactl`)**: fastest to a first boot, but a tool outside the repository with its
  own defaults, so boot evidence would not be reproducible from the repo alone.
- **Swift helper (chosen)**: no Rust dependency, the Apple API used as documented, about 250 lines, built and
  ad-hoc signed by `scripts/macos/build-vmlauncher.sh`.

## Rationale and consequences

The helper keeps the Rust crate dependency-free and keeps the boot contract (launch config in, JSON status line
and console log out) inspectable. Easier: boot reliability runs (`make systemvm-boot-smoke`) and future guest
agents that only need a booted VM. Harder: a second toolchain (Xcode command line tools, Swift 5.9+) on
macOS developer machines; Linux CI can only verify the image, and GitHub's macOS runners cannot boot
(no nested virtualisation), so boot evidence is produced on developer hardware and attached to issues. The
helper must stay thin: new behaviour goes into the launch config schema in `src/systemvm.rs` first.

## Implementation and migration impact

Added `launcher/Package.swift`, `launcher/Sources/vesper-vmlauncher/main.swift`,
`packaging/macos/vesper-vmlauncher.entitlements` (virtualization only, no sandbox so the helper can read the
image files), `scripts/macos/build-vmlauncher.sh`, `make vmlauncher-build`, a `launcher-build` job in
`systemvm-image.yml`, and `boot_media_path` plus `ready_pattern` in the launch config. Nothing migrates; the
placeholder boot-smoke defaults (`sh -c 'echo boot-ok'`) are replaced by the launcher in `scripts/systemvm/boot-smoke.sh`.
ExecPlan: [2026-11-06-WOR-27-systemvm-image-pipeline.md](../../exec-plans/completed/2026-11-06-WOR-27-systemvm-image-pipeline.md).

## References

- [design-v0.md](../../specs/design-v0.md), "Phase 1" and "Rust preferred; Swift allowed only as FFI shim".
- [systemvm-bringup.md](../../process/systemvm-bringup.md).
- [launcher/README.md](../../../launcher/README.md), [src/systemvm.rs](../../../src/systemvm.rs).
- GitHub #12, #74; Linear WOR-27; tech debt TD-005.
