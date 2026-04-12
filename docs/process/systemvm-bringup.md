# System VM Build and Boot Pipeline (Phase 1 scaffold)

This project includes a deterministic scaffold for System VM image metadata, NAT-default launch config generation, and repeated boot diagnostics capture.

## Commands

1. Build deterministic image manifest from kernel + rootfs inputs:
   `cargo run --bin systemvmctl -- build --kernel <path> --rootfs <path> --out build/systemvm/image-manifest.json`

2. Generate NAT-default launch configuration:
   `cargo run --bin systemvmctl -- launch-config --kernel <path> --rootfs <path> --out build/systemvm/launch-config.json`

3. Execute repeated boot smoke diagnostics:
   `cargo run --bin systemvmctl -- boot-smoke --iterations 5 --log run/systemvm/boot.log --report run/systemvm/boot-report.json`

## Make targets

- `make systemvm-build`
- `make systemvm-launch-config`
- `make systemvm-boot-smoke`

## Reproducibility

`image-manifest.json` contains deterministic SHA256 digests for kernel/rootfs and a stable `image_id` derived from these digests.

## Proxy bridge convention

Phase 0 keeps the proxy-side vsock bridge inspectable by resolving
`vsock://<cid>:<port>` upstreams through a host Unix socket at
`<vsock_socket_dir>/<cid>.sock` and sending a Firecracker-style
`CONNECT <port>\n` preface before relaying traffic. This is the current
host-initiated bridge skeleton for `vesper-proxy`; the broader control-channel
protocol work remains tracked separately under the Phase 1 vsock issue.
