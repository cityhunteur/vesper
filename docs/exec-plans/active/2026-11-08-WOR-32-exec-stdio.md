# Exec, PTY, stdio streaming and exit status through a microVM guest agent (WOR-32 / GH #23)

This ExecPlan is a living document. The sections Progress, Surprises & Discoveries, Decision Log, and
Outcomes & Retrospective must be kept up to date as work proceeds. It is maintained in accordance with
docs/PLANS.md.

## Purpose / Big Picture

GitHub #23 (Linear WOR-32, mirror #72) asks for exec sessions, PTY resize, separated stdin/stdout/stderr and
exit status plus signal propagation that match containerd's expectations. The Phase 3.1 shim drives the task
through the kernel command line and a console log, which cannot carry stdin, a second stream, a PTY or a
signal. The slice therefore needs a guest agent inside the microVM reached over a Firecracker vsock device.
When this plan is done, `ctr run -t`, `ctr exec`, piped stdin with separated stderr and `ctr task kill -s TERM`
behave as with runc, proven by a `make systemvm-exec-suite` checklist.

## Progress

- [x] (2026-11-08) Transport plumbing: `vesper-microvm launch ID --vsock` adds `{"vsock":{"guest_cid":3,
  "uds_path":"/v.sock"}}` to the Firecracker config and `vesper_vsock=1` to the kernel command line; the
  initramfs now carries `vsock`, `vmw_vsock_virtio_transport_common` and `vmw_vsock_virtio_transport` from the
  modloop and the init loads them (`VESPER-VSOCK-OK`); `status ID` reports `vsock=present|absent` for the
  host-side socket `/srv/jailer/firecracker/ID/root/v.sock`. Verified on an M3 Pro: modules loaded, socket
  present, `GET /vm/config` shows the device; microvm and shim suites still pass.
- [ ] Guest agent `vesper-guest-agent` (Go, in `shim/`, static linux/arm64, shipped in the initramfs by
  `vesper-microvm prepare`): listens on vsock port 1025; one connection per process session; frames of
  `type(1) length(4) payload`; types: start (JSON: args, env, cwd, tty, rows, cols, root), started (pid),
  stdin, stdout, stderr, stdin-close, resize (rows, cols), signal (number), exit (status, signal), error
  (text). TTY sessions run on a pty and emit a single output stream; non-TTY sessions keep stdout and stderr
  apart. The init mounts proc, sysfs, devtmpfs (rbind) and devpts under the rootfs and execs the agent when
  `vesper_agent=1`.
- [ ] Shim: `Start` connects through `/v.sock` (`CONNECT 1025` preface) instead of the console follower, pumps
  the task FIFOs, maps the exit frame (signal → 128 + n); `Exec`/`Start(execID)`/`Kill(execID)`/`ResizePty`/
  `CloseIO`/`Wait(execID)`/`State(execID)`/`Delete(execID)` as exec sessions; `Kill` forwards the signal.
- [ ] Proof: guest `vesper-ptyrun` helper (pty wrapper so `ctr run -t` and `ctr exec -t` can run without a
  terminal), `vesper-ctr run-stdio|run-tty|exec|exec-tty|signal-test`, `control::run_exec_suite`,
  `make systemvm-exec-suite`: TTY session reports a `/dev/pts/*` tty and the requested size; piped stdin read
  back on stdout with stderr separated; exec exit code 7 with output; `SIGTERM` caught by a trap in the guest
  (exit 0 with marker), `SIGKILL` → 137.
- [ ] PR merged, CI green, GH #23 checklist and WOR-32 updated, milestone entry; plan moved to completed.

## Surprises & Discoveries

- Observation: `systemvmctl control microvm` needs `--` before guest options (`-- launch ID --vsock`), as clap
  otherwise claims them.

## Decision Log

- Decision: a vsock-connected guest agent rather than extending the console-log protocol.
  Rationale: stdin, a second output stream, PTY resize and signals need a bidirectional channel; the guest
  kernel ships the virtio vsock transport and Firecracker's host-side Unix socket uses the same `CONNECT
  <port>` preface as the launcher bridge (ADR 0003), so the shim can reuse that convention.
  Date/Author: 2026-11-08, agent.
- Decision: the agent is Go in the existing `shim/` module, cross-built with the shim (ADR 0004).

## Outcomes & Retrospective

Transport landed; the agent, shim sessions and proof are open. The agent is the next unit.

## Context and Orientation

`systemvm/overlay/usr/local/bin/vesper-microvm` (`--vsock`, init), `shim/` (ADR 0004), Firecracker vsock
semantics in `third_party/firecracker/firecracker.yaml` (`Vsock`).

## Plan of Work

1. Agent and initramfs wiring; 2. shim sessions; 3. guest proof driver and suite; 4. docs and landing.

## Concrete Steps

    make systemvm-up
    cargo run --bin systemvmctl -- control microvm -- launch vs1 --vsock
    cargo run --bin systemvmctl -- control microvm status vs1        # RUNNING vs1 pid=N vsock=present
    cargo run --bin systemvmctl -- control microvm cleanup vs1

## Validation and Acceptance

Per unit: the step above for the transport; `make shim-test` and `make systemvm-exec-suite` for the rest;
microvm and shim suites must keep passing.

## Idempotence and Recovery

`vesper-microvm cleanup ID` removes the jail including `v.sock`.

## Artifacts and Notes

    LAUNCHED vs1 pid=2834 boot_ms=1510
    RUNNING vs1 pid=2834 vsock=present
    VESPER-VSOCK-OK
    "vsock":{"guest_cid":3,"uds_path":"/v.sock"}

## Interfaces and Dependencies

Guest kernel modules from the pinned modloop; Firecracker v1.16.0 vsock device; Go libraries for vsock and
pty (to be pinned in `shim/go.mod` with the agent).
