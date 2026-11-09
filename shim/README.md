# shim/

`containerd-shim-vesper-v2` runs each containerd task in a jailed Firecracker microVM inside the System VM.
Runtime name: `io.containerd.vesper.v2`. See [ADR 0004](../docs/design-docs/adr/0004-containerd-shim-in-go.md)
and [ADR 0006](../docs/design-docs/adr/0006-guest-exec-via-runc-delegation.md).
The Go module pins containerd 2.1.5 and builds static Linux arm64 binaries with `make shim-build`.

## Interface

- `cmd/containerd-shim-vesper-v2`: registers the task plugin and starts the shim manager.
- `internal/manager`: starts one shim per task. After a shim dies, `Stop` reads `vesper-task.json`,
  reports the recorded exit or 137, removes the microVM and unmounts the bundle.
- `internal/bundle`: validates OCI bundles before mounting. `Inspect` derives a cache key from the ordered
  overlay lower-layer chain. `GuestConfig` copies the process into an OCI spec for `/newroot` inside the microVM.
  `GuestProcess` preserves argv, environment, user and cwd, removes host security labels and grants the guest's
  capabilities. Both use the microVM as the isolation boundary. The config joins `/proc/1/ns/net`, which is
  the existing guest network namespace. It creates no namespace and installs no second seccomp filter.
- `internal/backend`: builds one digest-verified ext4 cache entry per layer chain and copies a private drive
  into each bundle. Directory bundles use the direct fallback. `vesper-rootfs.json` records the strategy,
  digest, size, timing and cache identity. Configs travel separately, so they never change the cached drive.
  `Launch` boots `vesper-microvm --guest-exec`. `OpenSession` connects to its private `v.sock`, sends the
  `CONNECT <port>` preface and pumps the containerd FIFOs over three independent vsock ports.
- `internal/task`: implements init and exec `Start`, `State`, `Kill`, `Wait`, `Delete`, `ResizePty` and `CloseIO`.
  A task follows created → running → stopped → deleted. Init exit waits for running exec exit events.
  Every init transition updates `vesper-task.json`. Exec records remain in memory; a dead shim destroys
  the entire microVM. `Stats` reports the VMM's cgroup v2 metrics. `Pids` reports its System VM pid.
  `Pause`, `Resume`, `Checkpoint` and `Update` remain unsupported.
- `cmd/vesper-console`: uses the already pinned containerd `go-runc` and `console` libraries to receive
  runc's terminal master. It runs detached `runc create/start` or `exec`, reaps the guest process and
  relays its terminal through stdin/stdout. A private Unix socket applies terminal resize.
  The same binary supplies `proof-pty` for the hardware suite's `vesper-ptyrun` wrapper.

## Guest session protocol

`vesper-microvm-init` mounts proc, sysfs, cgroup2 and devpts, populates the workload's `/dev` and starts
`tini-static` with `socat` on microVM vsock port 1025. Each connection invokes the static shell script
`vesper-guest-session`. These listeners stay inside the microVM; no host socket or entitlement is added.

The shim sends `RUN|EXEC <base-port> <byte-count> <tty>` followed by the exact config bytes.
The guest writes `config.json` verbatim and replies `READY <base-port>`. The shim connects stdin,
stdout and stderr on `base-port`, `base-port+1` and `base-port+2`. A terminal merges output on stdout.
The guest returns `STARTED <guest-pid>` and later `EXIT <status>`, or `ERROR <JSON>` on runtime failure.
The shim bounds control replies. Requests validate numeric fields; configs may contain at most 1 MiB.

`SIGNAL <base-port> <signal> <all>`, `RESIZE <base-port> <rows> <cols>` and
`ABORT <base-port> 0 0` use separate control connections. Signal numbers are 0..64.
Terminal dimensions are 1..65535. Init uses port 1028; execs reserve consecutive groups of four ports.
The session key `p<base-port>` also names its guest directory and console breadcrumbs.
The task id names its VMM, jail, shim record and console log.

## Invariants

- A running process cannot start twice or be deleted. Concurrent lifecycle requests use a per-process lock.
- Cached ext4 bytes contain only rootfs content; process configuration never enters the cache key.
- runc launches argv directly. The image does not need `/bin/sh`.
- Non-terminal stdout and stderr remain separate, and stdin EOF reaches the workload.
- A signal exit maps to `128 + signal`; a lost transport returns 137 and logs the failure.
- The terminal starts at the OCI console size, or 24×80 when absent. `ResizePty` changes the existing terminal.
- Init exit destroys remaining guest processes. A failed exec start is aborted before its record can be deleted.
- The PID 1 supervisor reaps orphans. A dead shim never intentionally leaves a VMM behind.
- One task allows at most 16,125 exec sessions. Ports are not reused within that task (WOR-32 follow-up).

## Verification

    make shim-test                                  # vet and Go tests
    make shim-build                                 # static Linux arm64 shim and terminal helper
    make systemvm-up && make systemvm-exec-suite     # streams, exec, terminal resize and signals
    make systemvm-shim-suite                         # existing task lifecycle checks
    make systemvm-bundle-suite                       # layer cache and private drive checks
    make systemvm-restart-suite                      # containerd and shim crash checks
