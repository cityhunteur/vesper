---
status: verified
owner: cityhunteur
last_reviewed: 2026-11-06
---

# System VM Build and Boot Pipeline (Phase 1 scaffold)

This project includes a deterministic scaffold for System VM image metadata, NAT-default launch config generation, and repeated boot diagnostics capture.

## Commands

1. Build deterministic image manifest from kernel + rootfs inputs:
   `cargo run --bin systemvmctl -- build --kernel <path> --rootfs <path> --out build/systemvm/image-manifest.json`

2. Generate NAT-default launch configuration:
   `cargo run --bin systemvmctl -- launch-config --kernel <path> --rootfs <path> --console-log run/systemvm/console.log --out build/systemvm/launch-config.json`
   Only `build` reads the kernel and rootfs files; `launch-config` checks that the paths are non-blank strings.

3. Execute repeated boot smoke diagnostics:
   `cargo run --bin systemvmctl -- boot-smoke --iterations 5 --timeout-seconds 20 --command sh --args=-c,echo\ boot-ok --log run/systemvm/boot.log --report run/systemvm/boot-report.json`
   The defaults shown run a host shell command per iteration; nothing boots a VM yet (TD-005). Supply a real launcher
   via `--command` and comma-separated `--args`.

## Make targets

- `make systemvm-build`
- `make systemvm-launch-config`
- `make systemvm-boot-smoke`

Each wraps a script under `scripts/systemvm/` with the defaults above (`assets/systemvm/vmlinux`,
`assets/systemvm/rootfs.ext4`, 5 iterations, 20 s timeout). The scripts accept positional overrides (`KERNEL ROOTFS`,
or `ITERATIONS TIMEOUT COMMAND ARGS`), but `make` does not forward them, so invoke the script directly to override.
The scripts must be run from inside the crate tree; see `scripts/systemvm/README.md`.

## Reproducibility

`image-manifest.json` contains deterministic SHA256 digests for kernel/rootfs and a stable `image_id` derived from these digests, plus `schema_version` and the raw `kernel_path`/`rootfs_path` strings. Two manifests built from identical bytes at different paths share `image_id` but are not byte-identical.

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
