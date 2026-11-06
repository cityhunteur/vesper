# Build the Phase 1 System VM image and boot pipeline (WOR-27 / GH #12)

This ExecPlan is a living document. The sections Progress, Surprises & Discoveries, Decision Log, and
Outcomes & Retrospective must be kept up to date as work proceeds. It is maintained in accordance with
docs/PLANS.md.

## Purpose / Big Picture

GitHub issue #12 (Linear WOR-27, mirror #74) asks for reproducible kernel and rootfs artifacts and a launch pipeline
for the Linux System VM, with the VM booting to a healthy state in repeated runs and the artifacts reproducible in
CI. After this plan a developer runs `make systemvm-fetch` and gets a digest-verified Alpine kernel and initramfs in
`build/systemvm/`, `make systemvm-launch-config` writes a NAT-default launch configuration for them, and a launcher
boots that configuration on Virtualization.framework so that `make systemvm-boot-smoke` proves N consecutive boots
with console output. The parity unit for slice 1 is "pinned, verified, CI-reproducible image acquisition".

## Progress

- [x] (2026-11-06 16:00Z) Slice 1: image lock, `systemvmctl lock-env` and `verify-image`, `fetch-image.sh`,
  `make systemvm-fetch`, `systemvm-image.yml`, initrd support in manifest and launch config, placeholders removed,
  docs and EGM contracts updated. Local sensors green; cold and cached fetch proven.
- [ ] Slice 1: PR merged, CI green, GH #74 and WOR-27 updated, milestone entry written.
- [ ] Slice 2: launcher decision recorded as an ADR (Swift helper under `packaging/` vs Rust bindings to
  Virtualization.framework); launcher boots `launch-config.json` and writes the console to `console_log_path`.
- [ ] Slice 3: `boot-smoke` drives the launcher; healthy-state detection on the console (Alpine login banner);
  200-iteration run recorded under `run/WOR-27/systemvm/`.
- [ ] Slice 4: NAT connectivity proof from the guest; boot-time threshold recorded; GH #12 checklist updated.

## Surprises & Discoveries

- Observation: the Alpine `netboot` flavour ships `vmlinuz-virt` as an EFI-stubbed PE32+ arm64 Image, not an ELF
  `vmlinux`. Virtualization.framework's Linux boot loader accepts this format; the earlier placeholder name
  `vmlinux` was misleading.
  Evidence: `file build/systemvm/vmlinuz-virt` prints `PE32+ executable (EFI application) Aarch64`.
- Observation: the netboot archive is about 330 MB because it bundles three kernel flavours and modloops; only two
  members are needed.
  Evidence: `tar tzf` listing; `source.size_bytes` in the lock.
- Observation: no ext4 tooling (`mke2fs`) is on a stock macOS host, so a block rootfs cannot be built locally
  without new tooling. The initramfs boot path sidesteps this for the boot-reliability milestone.
  Evidence: `command -v mke2fs` returns nothing on the development host.

## Decision Log

- Decision: pin an upstream Alpine netboot release instead of building a kernel and rootfs in-repo.
  Rationale: the issue's first deliverable is reproducibility, which a pinned URL plus SHA-256 digests gives at once;
  building images needs Linux tooling that CI has but macOS developers do not. design-v0 allows any minimal Linux
  System VM and suggests Apple Containerization's kernel as a later optimisation.
  Date/Author: 2026-11-06, agent (Claude) for cityhunteur.
- Decision: parse the lock in Rust and expose it to bash via `systemvmctl lock-env`.
  Rationale: "parse at the boundary" (ARCHITECTURE.md invariant 7); the shell never interprets YAML.
  Date/Author: 2026-11-06, agent.
- Decision: make `rootfs` optional and add `initrd` to the manifest and launch config (schema version 2).
  Rationale: the pinned image boots from an initramfs; the Phase 1 containerd slice adds a block rootfs later and the
  config already carries the field.
  Date/Author: 2026-11-06, agent.
- Decision: defer the launcher technology choice to a human.
  Rationale: both options add a runtime dependency or a packaged, signed helper, which AGENTS.md lists under
  "Ask a human first".
  Date/Author: 2026-11-06, agent.

## Outcomes & Retrospective

Slice 1 delivered a verifiable image pipeline with no new crate dependencies. Gaps: nothing boots yet (TD-005); the
image is `aarch64` only, so the CI job verifies digests on x86-64 Linux but cannot boot there.

## Context and Orientation

`src/systemvm.rs` holds the library (manifest, launch config, boot smoke, image lock). `src/bin/systemvmctl.rs` is
the CLI. `scripts/systemvm/` holds the Make-driven wrappers. `docs/specs/systemvm-image.lock.yaml` is the pin.
Terms: *image lock* is the YAML pin; *manifest* is `build/systemvm/image-manifest.json`; *launch config* is
`build/systemvm/launch-config.json`; *launcher* is the not-yet-written process that boots a launch config.
Guides: design-v0 "Phase 1", docs/process/systemvm-bringup.md, scripts/systemvm/README.md, src/README.md,
src/bin/README.md, docs/RELIABILITY.md "System VM work".

## Plan of Work

Slice 1 (done): add `ImageLock` types and `load_image_lock`, `validate_image_lock`, `image_lock_env`, `verify_image`
to `src/systemvm.rs`; make `rootfs_path` optional and add `initrd_path` to `ImageBuildConfig`, `ImageManifest` and
`LaunchConfig`; add `kernel_cmdline`; add `lock-env` and `verify-image` subcommands; write `fetch-image.sh`; point
`build-image.sh` and `generate-launch-config.sh` at the fetched files; add the Make target and the CI workflow;
delete `assets/systemvm/`.

Slice 2: record the launcher ADR; implement the launcher so that it reads `launch-config.json`, boots with NAT
networking, writes the serial console to `console_log_path`, exits 0 when a configurable console pattern appears and
non-zero on timeout.

Slice 3: teach `boot-smoke` to pass the launch config and pattern to the launcher by default; record a long run.

Slice 4: NAT proof (guest `wget` of a known host) and boot-time measurement in the report.

## Concrete Steps

    cd <repo root>
    make systemvm-fetch            # first run downloads; prints "verified alpine 3.22.8 aarch64 image ... image_id=40bd8242..."
    make systemvm-build            # prints "wrote image manifest: .../build/systemvm/image-manifest.json (image_id=40bd8242...)"
    make systemvm-launch-config    # prints "wrote launch config: .../build/systemvm/launch-config.json"
    cargo test systemvm            # 8 tests pass

## Validation and Acceptance

Slice 1: `cargo test systemvm`, `cargo test`, `python3 scripts/harness/check_structure.py --root .`,
`python3 harness/check_docs.py --strict`, `python3 scripts/harness/check_docs.py --root .`; `make systemvm-fetch`
twice (cold, then cached with no download); a corrupted `initramfs-virt` makes `systemvmctl verify-image` exit 1 with
a digest mismatch; the `System VM Image` workflow is green on the PR. Artifacts under `run/WOR-27/systemvm/`.

Exit for the issue: GH #12 checklist items proven by a launcher-driven boot-smoke report.

## Idempotence and Recovery

`make systemvm-fetch` is safe to repeat; delete `build/systemvm/` to force a cold run. The lock bump procedure is in
docs/process/systemvm-bringup.md. All slice-1 changes are plain files on branch `wor-27-systemvm-image-fetch`.

## Artifacts and Notes

    $ make systemvm-fetch
    systemvm-fetch: using cached .../build/systemvm/cache/alpine-netboot-3.22.8-aarch64.tar.gz
    verified alpine 3.22.8 aarch64 image in .../build/systemvm: image_id=40bd82424e8154232b1bc61923e6a9cca9b665e901ff161150e8c90e1423cca6

    $ printf x >> build/systemvm/initramfs-virt && cargo run -q --bin systemvmctl -- verify-image
    initrd build/systemvm/initramfs-virt sha256 693ffe08... does not match pinned 0f7da92c...

## Interfaces and Dependencies

`vesper::systemvm::{ImageLock, ImageSource, ImageArtifacts, ImageArtifact, load_image_lock, validate_image_lock,
image_lock_env, verify_image, image_id}`; `ImageBuildConfig { kernel_path, rootfs_path: Option<String>, initrd_path:
Option<String> }`; `LaunchConfig { kernel_path, rootfs_path: Option, initrd_path: Option, kernel_cmdline,
network_mode, cpu_count, memory_mib, console_log_path }`; `default_launch_config(kernel, rootfs: Option<&str>,
initrd: Option<&str>, console_log)`. No new crate dependencies. Slice 2 will add the launcher interface.
