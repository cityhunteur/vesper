---
status: accepted
owner: cityhunteur
last_reviewed: 2026-11-06
---

# 0004. containerd shim v2 in Go, cross-built on the host and shipped in the System VM overlay

## Context and problem

Phase 3 of [design-v0.md](../../specs/design-v0.md) needs `containerd-shim-vesper-v2`, a containerd runtime v2
shim that turns each container task into a jailed Firecracker microVM inside the System VM. The shim runs as a
Linux process next to containerd in the Alpine guest, speaks ttrpc, and must follow containerd's shim framework
(bootstrap parameters, socket inheritance, event publishing, `delete` after a crash). The rest of Vesper is Rust
on macOS.

## Decision

- The shim is a Go module at `shim/` (`github.com/cityhunteur/vesper/shim`) built against the containerd
  release the guest runs (containerd 2.1.5 with `containerd/api` 1.9.0), using `pkg/shim` for the daemon
  skeleton, `core/mount` for bundle rootfs mounts and `runtime/task/v3` for the Task service.
- It is cross-compiled on the host (`GOOS=linux GOARCH=arm64 CGO_ENABLED=0 -trimpath`) by
  `scripts/systemvm/build-shim.sh`, and `make systemvm-overlay` ships the static binary to
  `/usr/bin/containerd-shim-vesper-v2` in the guest overlay (the OpenRC containerd PATH does not include
  `/usr/local/bin`). The runtime name is `io.containerd.vesper.v2`.
- One shim process serves one task. `Create` mounts the bundle rootfs and builds an ext4 root drive with
  `mke2fs -d`; `Start` runs `vesper-microvm launch ID --rootfs … --cmd <base64>`, with the OCI process
  (`cwd`, `env`, `args`) encoded as a shell command on the microVM kernel command line; the shim tails the
  microVM console log, copies the workload output between the `VESPER-WORKLOAD-START/EXIT` markers to the
  task's stdout FIFO and takes the exit status from the exit marker (or 137 when the VMM vanishes); `Kill` is
  `vesper-microvm stop`; `Delete` is `vesper-microvm cleanup` plus image removal and shim shutdown.

## Alternatives considered

- **Rust shim.** Keeps one language, but there is no maintained Rust implementation of containerd's shim
  framework and cross-linking a musl aarch64 binary from macOS needs an extra linker toolchain. The design
  recommends Go for ecosystem fit.
- **Build the shim inside the guest.** Avoids a host Go toolchain, but a nested-virtualization guest with two
  vCPUs is a slow and unobservable build host, and the result would not be pinned by the repository.
- **A vsock or MMDS channel for the command instead of the kernel command line.** More general, but the
  microVM has neither yet; the command line is already parsed by the init and bounded by its size limit
  (TD-032).

## Rationale and consequences

- Go is a new toolchain for contributors and CI (`actions/setup-go` in the image workflow and a `Shim Tests`
  workflow running `go vet` and `go test`); `go.mod` pins every dependency and the build is reproducible
  (`-trimpath`, stripped, no build id), so the overlay determinism check still holds.
- The shim's behaviour is testable without a guest: the state machine, command encoding and log follower are
  unit-tested; the end-to-end lifecycle is proven by `make systemvm-shim-suite` through `ctr`.
- Limits that follow from this slice: the image must contain `/bin/sh`; no terminal, stdin, exec or resize
  (P3.2); the bundle rootfs is converted on every `Create` with no cache (P3.3); the host sees only the VMM pid.

## Implementation and migration impact

`shim/` (module, `cmd/containerd-shim-vesper-v2`, `internal/{manager,backend,task}`),
`scripts/systemvm/build-shim.sh`, `scripts/systemvm/build-overlay.sh`, the guest driver `vesper-ctr`, the agent
`CTR` command, `control::run_shim_suite`, `systemvmctl control ctr|shim-suite`, `make shim-build shim-test
systemvm-shim-suite`, `.github/workflows/shim-tests.yml`. Nothing on the macOS host runs Go at runtime.

## References

- GH #22 / WOR-29; design-v0 "Phase 3 — containerd shim v2" and the repository layout (`shim/` (Go)).
- containerd runtime v2 shim API (`pkg/shim`, `api/runtime/task/v3`).
