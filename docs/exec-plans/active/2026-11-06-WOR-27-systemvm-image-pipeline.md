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
- [x] (2026-11-06 16:30Z) Slice 1: PR #119 merged as `8bd505b`, CI green, GH #74 and WOR-27 updated, milestone
  entry `297c678`.
- [x] (2026-11-06 17:10Z) Slice 2: ADR 0001 (Swift helper, chosen by the owner); `launcher/` boots
  `launch-config.json` with NAT and a virtio console and writes `console_log_path`; lock moved to the Alpine virt
  ISO with zboot kernel unpacking and the ISO as read-only boot media; `boot-smoke` drives the launcher and records
  per-boot durations; 5/5 and 200/200 boots to the login prompt recorded under `run/WOR-27/systemvm/`.
- [x] (2026-11-06 17:40Z) Slice 2: PR #120 merged as `a638c34`, CI green (Linux image job + macOS launcher build),
  GH #74 and WOR-27 updated, milestone entry `6866681`.
- [x] (2026-11-06 18:30Z) Slice 3: owner set the threshold to 5 s and chose the console-driven probe; lock schema 3
  carries `boot.max_boot_ms` and `boot.probe`; the launcher logs in, runs `udhcpc`, pings the NAT gateway, prints
  `VESPER-NAT-OK` and powers off; boot smoke parses the launcher's ready time and enforces the threshold; 5/5 and
  200/200 probed boots recorded.
- [ ] Slice 3: PR merged, CI green, GH #74, GH #12 checklist and WOR-27 updated, milestone entry written; plan
  moved to completed.

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
- Observation: Virtualization.framework refused the netboot kernel with "Internal Virtualization error" because
  Alpine's arm64 `vmlinuz-virt` is an EFI zboot PE wrapping a gzip payload, not a raw Image; the `file` output
  (`PE32+ … EFI application`) had been misread as an EFI-stubbed raw Image in slice 1.
  Evidence: bytes 4..8 are `zimg`, bytes 24..28 `gzip`; the gunzipped payload has `ARM\x64` at offset 56 and boots.
- Observation: the raw kernel with the netboot initramfs alone drops to the initramfs emergency shell ("Mounting boot
  media: failed") because that initramfs expects boot media or network parameters. Attaching the virt ISO as a
  read-only virtio block device satisfies it offline, and the ISO (80 MB) is smaller than the netboot tarball.
  Evidence: `run/WOR-27/systemvm/single-boot-raw.txt` vs `single-boot-iso.txt` (ready in 1055 ms).
- Observation: ad-hoc signing with only `com.apple.security.virtualization` is enough to boot on macOS 27 (no
  Developer ID, no sandbox).
  Evidence: `codesign -d --entitlements -` on `build/vmlauncher/vesper-vmlauncher`; boot-smoke 200/200.
- Observation: the live ISO leaves `eth0` down; `udhcpc` alone loops on "Network is down". `ip link set eth0 up`
  first gives a lease from 192.168.64.1 within a second.
  Evidence: `run/WOR-27/systemvm/single-boot-probe.txt` and the console log (first probe attempt timed out).

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
- Decision: Swift helper process (`launcher/`), recorded as ADR 0001.
  Rationale: owner's choice; no Rust dependency, Apple API as documented, policy stays in Rust.
  Date/Author: 2026-11-06, cityhunteur (decision), agent (record).
- Decision: pin the Alpine `virt` ISO instead of the `netboot` tarball, unpack the zboot kernel in a Python
  standard-library script, and attach the ISO read-only as boot media.
  Rationale: the only offline-bootable combination found (see Surprises); keeps the lock a single source file;
  avoids a gzip crate in Rust. Lock schema bumped to 2 with `member_sha256`, `unpack`, `file`, `boot_media` and
  `boot` so every transformation is digest-pinned.
  Date/Author: 2026-11-06, agent.
- Decision: the launch config carries `ready_pattern` and `boot_media_path`, and the launcher reads both.
  Rationale: the launcher must have no policy of its own (ADR 0001).
  Date/Author: 2026-11-06, agent.
- Decision: boot-time threshold 5000 ms, pinned as `boot.max_boot_ms` in the image lock and enforced by
  `systemvmctl boot-smoke` against the launcher's `ready` time (not the wall clock, which includes the probe).
  Rationale: owner's number; measured boots are about 1.05 s, leaving headroom for slower hosts.
  Date/Author: 2026-11-06, cityhunteur (decision), agent (record).
- Decision: prove NAT from the guest with a console-driven probe pinned in the lock (`boot.probe`), not a guest
  agent.
  Rationale: owner's choice; self-contained, no new dependency, and the same path gives a clean `poweroff`. The
  vsock control channel stays with WOR-25. The success marker is split in the typed command because the console
  echoes input, and `ProbeSettings::validate` rejects markers that appear in typed lines.
  Date/Author: 2026-11-06, cityhunteur (decision), agent (record).

## Outcomes & Retrospective

Slice 1 delivered a verifiable image pipeline with no new crate dependencies. Slice 2 delivered the launcher and
real boot evidence (200/200 to the login prompt, about 1.1 s each on an M3; `run/WOR-27/systemvm/boot-report-200.json`).
Slice 3 delivered the guest probe and threshold: every boot now proves NAT from inside the guest and powers off
cleanly, under 5 s (`run/WOR-27/systemvm/boot-report-probe-200.json`). All four GH #12 checklist items are proven.
Gaps: no workload inside the guest and no control channel beyond the console script; boot evidence cannot be
produced in CI (TD-005). Lesson: verify an image format by booting it, not by `file` output; slice 1's
"EFI-stubbed Image" claim was wrong.

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

Slice 2 (done): ADR 0001; `launcher/` reads `launch-config.json`, boots with NAT, streams the console to
`console_log_path`, exits 0 on the config's `ready_pattern` and 2 on timeout; lock schema 2 (ISO, zboot unpack,
boot media, boot settings); `systemvmctl launch-config --from-lock`; `boot-smoke.sh` drives the launcher; per-boot
durations in the report; `launcher-build` CI job on macOS.

Slice 3 (done): `ProbeSettings` and `max_boot_ms` in the lock (schema 3) and launch config; the launcher's probe
state machine (login, commands, success pattern, shutdown); `run_boot_check` captures stdout, reads the `ready`
time and enforces the threshold; `boot-smoke.sh` passes the lock's threshold.

Issue exit: close out on GitHub and Linear; move this plan to `docs/exec-plans/completed/`.

## Concrete Steps

    cd <repo root>
    make systemvm-fetch            # first run downloads the ISO; prints "verified alpine 3.22.8 aarch64 image ... image_id=b1965de5..."
    make systemvm-launch-config    # prints "wrote launch config: .../build/systemvm/launch-config.json"
    make vmlauncher-build          # prints "built and signed .../build/vmlauncher/vesper-vmlauncher"
    make systemvm-boot-smoke       # five JSON "ready" lines, then "boot smoke passed: successful=5 failed=0"
    cargo test systemvm            # 9 tests pass

## Validation and Acceptance

Slice 1: `cargo test systemvm`, `cargo test`, `python3 scripts/harness/check_structure.py --root .`,
`python3 harness/check_docs.py --strict`, `python3 scripts/harness/check_docs.py --root .`; `make systemvm-fetch`
twice (cold, then cached with no download); a corrupted `initramfs-virt` makes `systemvmctl verify-image` exit 1 with
a digest mismatch; the `System VM Image` workflow is green on the PR. Artifacts under `run/WOR-27/systemvm/`.

Slice 2: the above plus `make vmlauncher-build`, `make systemvm-boot-smoke` (5 and 200 iterations, zero failures),
`make validate-entitlements` on both lint paths, and the `launcher-build` CI job.

Slice 3: `cargo test systemvm` (10 tests, incl. the threshold and probe validation), one probed boot showing the
DHCP lease, `VESPER-NAT-OK` and the power-off in `run/systemvm/console.log`, `make systemvm-boot-smoke` 5/5 and
`boot-smoke.sh 200 60` 200/200 with `max_boot_ms` under 5000.

Exit for the issue: all four GH #12 checklist items proven by launcher-driven boot-smoke reports.

## Idempotence and Recovery

`make systemvm-fetch` is safe to repeat; delete `build/systemvm/` to force a cold run. The lock bump procedure is in
docs/process/systemvm-bringup.md. All slice-1 changes are plain files on branch `wor-27-systemvm-image-fetch`.

## Artifacts and Notes

    $ make systemvm-fetch
    systemvm-fetch: using cached .../build/systemvm/cache/alpine-virt-3.22.8-aarch64.iso
    unpacked .../cache/boot/vmlinuz-virt -> .../build/systemvm/Image-virt (34734080 bytes)
    verified alpine 3.22.8 aarch64 image in .../build/systemvm: image_id=b1965de508d579c75a9f55c761a9114d42e6220a81d0d101517733ff546e8abe

    $ make systemvm-boot-smoke
    {"status":"ready","elapsed_ms":1052,"ready_pattern":"login:","console_log":".../run/systemvm/console.log"}
    {"status":"probe-ok","elapsed_ms":1302,...}
    {"status":"stopped","elapsed_ms":3584,...}
    boot smoke passed: successful=5 failed=0 max_boot_ms=1053 (threshold 5000)

    $ grep -nE "udhcpc: lease|VESPER-NAT-OK|poweroff" run/systemvm/console.log
    128:udhcpc: lease of 192.168.64.3 obtained from 192.168.64.1, lease time 3600
    131:VESPER-NAT-OK
    132:localhost:~# poweroff

    $ printf x >> build/systemvm/initramfs-virt && cargo run -q --bin systemvmctl -- verify-image
    initrd build/systemvm/initramfs-virt sha256 693ffe08... does not match pinned 0f7da92c...

## Interfaces and Dependencies

`vesper::systemvm::{ImageLock, ImageSource, ImageArtifacts, ImageArtifact, UnpackMethod, BootMedia, BootSettings,
ProbeSettings, LaunchInputs, load_image_lock, validate_image_lock, image_lock_env, verify_image,
launch_config_from_lock, image_id}`;
`ImageBuildConfig { kernel_path, rootfs_path, initrd_path, boot_media_path: Option<String> }`; `LaunchConfig {
kernel_path, rootfs_path?, initrd_path?, boot_media_path?, kernel_cmdline, ready_pattern, max_boot_ms?, probe?,
network_mode, cpu_count, memory_mib, console_log_path }`; `default_launch_config(&LaunchInputs)`;
`BootCheckConfig.max_boot_ms`. Launcher contract: `launcher/README.md`.
No new crate dependencies.
