---
status: verified
owner: cityhunteur
last_reviewed: 2026-11-06
---

# System VM Build and Boot Pipeline (Phase 1)

This project pins a real System VM boot image and verifies it end to end, generates a NAT-default launch
configuration, and captures repeated boot diagnostics. A launcher that boots the image on Virtualization.framework is
the next WOR-27 slice (TD-005).

## Pinned image

`docs/specs/systemvm-image.lock.yaml` names the upstream release (Alpine Linux 3.22.8 `netboot` flavour for
`aarch64`, kernel `6.12.115-0-virt`), the archive URL and SHA-256, the two archive members that are used
(`boot/vmlinuz-virt`, an uncompressed arm64 Image with EFI stub, and `boot/initramfs-virt`, a gzip cpio initramfs)
with their SHA-256 digests, and `expected_image_id`. The lock is parsed only in Rust
(`systemvm::load_image_lock`, `deny_unknown_fields`); the shell never interprets the YAML.

The netboot initramfs boots to an Alpine login prompt with no block rootfs, which is enough for boot-reliability
proof. A block rootfs for containerd arrives with the Phase 1 containerd slice.

## Commands

1. Fetch and verify the pinned image (first run downloads about 330 MB; later runs reuse
   `build/systemvm/cache/`):
   `make systemvm-fetch` (wraps `scripts/systemvm/fetch-image.sh [LOCK]`), which runs
   `systemvmctl lock-env`, downloads the archive with `curl --retry 3`, checks the archive digest, extracts the two
   members to `build/systemvm/vmlinuz-virt` and `build/systemvm/initramfs-virt`, and runs
   `systemvmctl verify-image`, which checks both member digests and the `image_id` and writes
   `build/systemvm/image-manifest.json`.

2. Build an image manifest from explicit inputs:
   `cargo run --bin systemvmctl -- build --kernel <path> [--initrd <path>] [--rootfs <path>] --out build/systemvm/image-manifest.json`
   At least one of `--initrd` and `--rootfs` is required.

3. Generate NAT-default launch configuration:
   `cargo run --bin systemvmctl -- launch-config --kernel <path> [--initrd <path>] [--rootfs <path>] --console-log run/systemvm/console.log --out build/systemvm/launch-config.json`
   Only `build` and `verify-image` read the files; `launch-config` checks that the paths are non-blank strings.
   `kernel_cmdline` defaults to `console=hvc0`, plus ` root=/dev/vda rw` when a rootfs is given.

4. Execute repeated boot smoke diagnostics:
   `cargo run --bin systemvmctl -- boot-smoke --iterations 5 --timeout-seconds 20 --command sh --args=-c,echo\ boot-ok --log run/systemvm/boot.log --report run/systemvm/boot-report.json`
   The defaults shown run a host shell command per iteration; nothing boots a VM yet (TD-005). Supply a real launcher
   via `--command` and comma-separated `--args`.

## Make targets

- `make systemvm-fetch`
- `make systemvm-build`
- `make systemvm-launch-config`
- `make systemvm-boot-smoke`

Each wraps a script under `scripts/systemvm/`. `build-image.sh` and `generate-launch-config.sh` default to the
fetched `build/systemvm/vmlinuz-virt` and `build/systemvm/initramfs-virt` and exit 2 with a pointer to
`make systemvm-fetch` when either is missing; positional overrides are `KERNEL INITRD [ROOTFS]`. `boot-smoke.sh`
takes `ITERATIONS TIMEOUT COMMAND ARGS`. `make` does not forward arguments, so invoke the script directly to
override. The scripts must be run from inside the crate tree; see `scripts/systemvm/README.md`.

## Reproducibility

`image-manifest.json` contains SHA-256 digests for the kernel and the initramfs or rootfs, plus a stable `image_id`
(`sha256("<kernel>:<rootfs or empty>:<initrd or empty>")` over the hex digests), `schema_version` `"2"` and the raw
input paths. Two manifests built from identical bytes at different paths share `image_id` but are not byte-identical.
The `System VM Image` workflow (`.github/workflows/systemvm-image.yml`) runs `make systemvm-fetch`, `make
systemvm-build` and `make systemvm-launch-config` on Linux, asserts the manifest `image_id` equals the lock's
`expected_image_id`, and uploads both JSON artifacts. The archive is cached by the lock file's hash.

## Bumping the pinned image

1. Read the new release's `latest-releases.yaml` under
   `https://dl-cdn.alpinelinux.org/alpine/<branch>/releases/<arch>/` and update `source.url`, `source.sha256`,
   `source.size_bytes`, `source.published`, `release`, `branch` and `kernel_version` in the lock.
2. Download and extract the two members once, compute their SHA-256 digests, and update `artifacts.*.sha256`.
3. Run `make systemvm-fetch`; it fails on `expected_image_id`. Copy the printed `image_id` into the lock and rerun.
4. Run `cargo test systemvm` (the `validates_repo_image_lock` test recomputes `expected_image_id`), then record the
   bump in the milestone log.

## Proxy bridge convention

Phase 0 keeps the proxy-side vsock bridge inspectable by resolving
`vsock://<cid>:<port>` upstreams through a host Unix socket at
`<vsock_socket_dir>/<cid>.sock` and sending a Firecracker-style
`CONNECT <port>\n` preface before relaying traffic. This is the current
host-initiated bridge skeleton for `vesper-proxy`; the broader control-channel
protocol work remains tracked separately under the Phase 1 vsock issue.

Configuration lives on `vesper-proxy`: `--upstream` (default `vsock://3:2375`), `--vsock-socket-dir` (default
`/tmp/vesper/vsock`) and `--dial-timeout-seconds` (default 5); the preface is written after the dial succeeds within
that timeout. `make dev-up` does not use the bridge: it points the proxy at a Unix upstream.
