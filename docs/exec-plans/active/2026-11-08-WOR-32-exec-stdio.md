# Exec, PTY, stdio streaming and exit status through runc in the microVM (WOR-32 / GH #23)

This ExecPlan is a living document. The sections Progress, Surprises & Discoveries, Decision Log, and
Outcomes & Retrospective must be kept up to date as work proceeds. It is maintained in accordance with
docs/PLANS.md.

## Purpose / Big Picture

GitHub #23 (Linear WOR-32, mirror #72) asks for exec sessions, PTY resize, separated stdin/stdout/stderr and
exit status plus signal propagation that match containerd's expectations. The Phase 3.1 shim drives the task
through the kernel command line and a one-way console log, which cannot carry stdin, a second stream, a PTY or
a signal. Per ADR 0006, the slice drives execution with runc inside the microVM as a bare supervisor rather
than a custom guest agent: the shim generates the OCI config.json host-side and sends it over vsock, each
stdio stream rides its own vsock port bridged by socat, and runc owns the spawn, chroot, signals and exit.
When this plan is done, `ctr run -t`, `ctr exec`, piped stdin with separated stderr and `ctr task kill -s
TERM` behave as with runc, proven by a `make systemvm-exec-suite` checklist.

## Progress

- [x] (2026-11-08, PR #133 `ecba8ac`, CI green) Transport plumbing: `vesper-microvm launch ID --vsock` adds
  `{"vsock":{"guest_cid":3,"uds_path":"/v.sock"}}` to the Firecracker config and `vesper_vsock=1` to the
  kernel command line; the initramfs carries the vsock modules and the init loads them (`VESPER-VSOCK-OK`);
  `status ID` reports `vsock=present|absent`. Verified on an M3 Pro.
- [x] (2026-11-09) Design reset to runc-delegation (ADR 0006), after an adversarial brainstorm (two swarms,
  six lenses). The custom Go guest agent is withdrawn. the guest-exec brainstorm note records the full analysis and the
  split verdict (`docs/design-docs/2026-11-09-wor-32-guest-exec-brainstorm.md`).
- [ ] Prerequisite: runc-enablement in the microVM. `vesper-microvm prepare` copies runc, libseccomp and the
  terminal receiver (and optionally tini-static) plus their library closures into the initramfs, digest-pinned
  like the existing busybox/module copy loop; the init mounts cgroup2 and devpts, populates /dev and makes the
  new root a private mount. Capture `zcat /proc/config.gz` as kernel evidence. Proven by a trivial `runc run`
  of a hello-world inside the microVM.
- [ ] Slice 1 (non-interactive exec, model-deliverable): the shim generates a bare host-namespaced
  config.json (no new namespaces, no second seccomp, full caps, `root.path` at the mounted ext4) in
  `shim/internal/backend`/`bundle` and sends it over the vsock start frame; the guest writes it verbatim.
  Each of stdin, stdout, stderr rides its own vsock port bridged by socat; the shim maps its FIFOs to ports
  with no framing. runc supervises; exit via `runc wait`, signals via `runc kill`. The shim session code
  replaces the console-log follower for the running path; the console log stays for boot diagnostics with a
  retained `VESPER-WORKLOAD-EXIT <n>` breadcrumb. Covers `ctr run` (non-tty), `ctr exec` (non-tty), piped
  stdin with separated stderr, exit fidelity, signals, and fixes TD-034.
- [ ] Slice 2 (interactive terminal + resize): the terminal-master receiver (open decision: runc upstream
  `recvtty` vendored, or hand-written) receives the master over `--console-socket` via `SCM_RIGHTS`, relays
  it over vsock and applies `TIOCSWINSZ` on resize; the shim implements `Exec`/`Start(execID)`/`Kill(execID)`/
  `ResizePty`/`CloseIO`/`Wait(execID)`/`State(execID)`. Verify the console-socket path and runc overhead at
  256 MiB on hardware before committing the estimate.
- [ ] Observability: one correlation id across shim session, guest container and vsock port; a structured
  error surfaced instead of a bare EOF when runc fails (against TD-007, TD-026).
- [ ] Proof: `vesper-ctr run-stdio|run-tty|exec|exec-tty|signal-test`, `control::run_exec_suite`,
  `make systemvm-exec-suite`: TTY session reports `/dev/pts/*` and the requested size; piped stdin read back
  on stdout with stderr separated; exec exit code 7 with output; `SIGTERM` trapped (exit 0 with marker);
  `SIGKILL` → 137; all of 137/143/0/7 proven through the chain.
- [ ] PR(s) merged, CI green, GH #23 checklist and WOR-32 updated, milestone entry; plan moved to completed.

## Surprises & Discoveries

- Observation: `systemvmctl control microvm` needs `--` before guest options (`-- launch ID --vsock`), as clap
  otherwise claims them.
- Observation (brainstorm): socat cannot carry the terminal master descriptor (an `SCM_RIGHTS` hand-off),
  cannot separate stdout from stderr, and has no channel for resize or signals. The per-stream-port design and
  a terminal-master receiver inside the guest are the consequences. runc and socat are pinned for the System
  VM, not the microVM, so they are new initramfs members.
- Constraint: the model is blocked from writing a socket-driven process-spawning/PTY agent. runc-delegation
  removes the spawn/chroot/signal code from model-authored source; the slice-1 guest surface (static shell
  init + socat bridges) is plausibly model-writable; the slice-2 terminal receiver is the blocked class unless
  `recvtty` is vendored. The split is deliberate so progress is not gated on the blocked piece.

## Decision Log

- Decision (2026-11-09, superseding the entries below): drive execution with runc as a bare supervisor per
  ADR 0006; withdraw the custom Go guest agent; generate config.json host-side; one vsock port per stream;
  stage as prerequisite → non-interactive exec → interactive terminal.
- Reversed (2026-11-08): a custom vsock-connected Go guest agent with a `type/length/payload` framed protocol
  and pinned vsock/pty Go libraries. Superseded by ADR 0006; the framed multiplexer and the library pins are
  dropped in favour of per-stream ports and runc.

## Outcomes & Retrospective

Transport landed. The design is reset to runc-delegation after adversarial review. Next unit is the
runc-enablement prerequisite, then non-interactive exec.

## Context and Orientation

`systemvm/overlay/usr/local/bin/vesper-microvm` (`--vsock`, init, `prepare`), `shim/internal/{backend,bundle,
task}` (ADR 0004), `systemvm/overlay/etc/init.d/vesper-port-relay` (socat idiom), `docs/specs/systemvm-packages.lock.yaml`
(runc 1.3.4-r1, socat, tini-static), Firecracker vsock semantics in `third_party/firecracker/firecracker.yaml`.
ADR 0006 is the governing decision; `docs/design-docs/2026-11-09-wor-32-guest-exec-brainstorm.md` holds the brainstorm analysis.

## Plan of Work

1. runc-enablement prerequisite (kernel evidence, initramfs members, init mounts, trivial `runc run`).
2. Slice 1: host-side config.json, per-stream socat bridges, shim session code, exit/signal mapping.
3. Slice 2: terminal-master receiver (recvtty or hand-written) and resize; exec-session methods.
4. Observability wiring; proof driver and suite; docs and landing.

## Concrete Steps

    make systemvm-up
    cargo run --bin systemvmctl -- control microvm -- launch vs1 --vsock
    cargo run --bin systemvmctl -- control microvm status vs1        # RUNNING vs1 pid=N vsock=present
    # prerequisite proof (once wired): a trivial `runc run` of a hello-world inside the microVM
    cargo run --bin systemvmctl -- control microvm cleanup vs1

## Validation and Acceptance

Per unit: the transport step above; a trivial `runc run` for the prerequisite; `make shim-test` and
`make systemvm-exec-suite` for the slices; microvm and shim suites must keep passing. 137/143/0/7 proven
through the chain. config.json generation keeps the cached ext4 byte-identical (TD-035).

## Idempotence and Recovery

`vesper-microvm cleanup ID` removes the jail including `v.sock` and the runc state.

## Artifacts and Notes

    LAUNCHED vs1 pid=2834 boot_ms=1510
    RUNNING vs1 pid=2834 vsock=present
    VESPER-VSOCK-OK
    "vsock":{"guest_cid":3,"uds_path":"/v.sock"}

## Interfaces and Dependencies

Guest kernel modules from the pinned modloop; Firecracker v1.16.0 vsock device; runc 1.3.4-r1, socat and
tini-static (pinned in the System VM lock, to be added as microVM initramfs members with their closures);
libseccomp for runc. Open decision: terminal-master receiver (runc upstream `recvtty` vendored vs
hand-written), to be settled at slice 2 with hardware verification. Pause/snapshot interaction with runc state
is deferred (ADR 0006).
