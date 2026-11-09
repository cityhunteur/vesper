---
status: proposed
owner: cityhunteur
last_reviewed: 2026-11-09
---

# 0006. Guest container execution through runc, driven over vsock

Date: 2026-11-09. Supersedes: none. Superseded by: none.

## Context and problem

WOR-32 (GH #23) requires exec sessions, a pseudo-terminal with resize, separated stdin/stdout/stderr, exit
status and signal propagation that match containerd. The Phase 3 shim (ADR 0004) drives a task through the
microVM kernel command line and a one-way console log, which cannot carry stdin, a second stream, a terminal
or a signal. The vsock transport to reach inside a microVM landed in PR #133.

The active WOR-32 ExecPlan committed to a custom Go guest agent inside the microVM that spawns the container
process and owns its terminal, stdio and signals over vsock. A safety control blocks the model from writing
that component, because a socket-driven process spawner with terminal and signal control is the same shape as
a remote-shell implant. The team ran an adversarial brainstorm (two swarms, six lenses, twelve reports) to
find an execution model that minimizes and isolates the human-written, classifier-sensitive surface. The
reviewers agreed on the technical facts and split on the verdict; this ADR records the chosen direction and
the costs it carries, including the parts the brainstorm did not settle.

## Decision

We will drive container execution inside the microVM with **runc** as a bare, host-namespaced supervisor
rather than a custom spawning agent. Concretely:

- The shim generates the OCI `config.json` host-side (reusing the spec it already parses) and sends it over
  the vsock start frame; the guest writes the bytes verbatim and never interprets them. The cached rootfs
  ext4 stays pure rootfs, so the TD-035 cache key is unaffected.
- runc runs as a minimal supervisor: no new pid/mount/net/ipc/uts/user namespaces, no second seccomp profile,
  full capabilities, `root.path` at the mounted ext4. The microVM boundary remains the isolation boundary.
- Each stdio stream travels on its own vsock port, bridged by socat using the shipped `vesper-port-relay`
  idiom, so no custom multiplexer or vsock/pty Go library is needed for the non-terminal path.
- The work is staged: a runc-enablement prerequisite, then non-interactive exec, then the interactive
  terminal and resize.

The custom in-guest Go agent planned in the WOR-32 ExecPlan is withdrawn, and the ExecPlan is rewritten to
this model.

## Alternatives considered

- **A human-authored or human-reviewed custom Go agent** (the original ExecPlan, about 300 to 450 lines).
  Several lenses judged this the simplest and most debuggable option, and the repository already has a
  human-approval path (the CE-0001..CE-0003 pattern). It was not chosen because the whole agent is the blocked
  shape, so no part of the feature can be delivered by the model alone; this ADR's direction lets the
  non-interactive subset ship without blocked code. This remains the fallback if runc-delegation proves too
  costly.
- **Pure socat, no runc.** Rejected on physics: socat relays bytes, so it cannot pass the terminal master
  descriptor (an `SCM_RIGHTS` hand-off), cannot separate stderr from stdout, and has no channel for resize or
  signals. It also leaves process spawning to be hand-written.
- **Vendor the Kata or firecracker-containerd agent.** Rejected: firecracker-containerd is in maintenance
  mode (containerd 1.7, no releases, reports of the agent dying after snapshot) and mismatches the Vesper
  containerd 2.1 shim; Kata's agent is heavier and assumes the Kata runtime on the host.

## Rationale and consequences

What becomes easier:

- runc owns the kernel-facing code the model cannot write: clone, namespace setup, pivot_root, exec and the
  seccomp install.
- Exit status and signals map cleanly onto `runc wait` and `runc kill`, better than the console-log marker
  scrape, and runc execs argv directly, which fixes TD-034 (the `/bin/sh` dependency).
- For non-interactive exec the model-authored surface is a static shell init plus host-side Go, with no
  compiled network-driven spawner, so that subset is deliverable without human-authored blocked code.

What becomes harder, and the open items:

- **runc is not in the microVM today.** runc, `libseccomp` and a terminal receiver must be injected into the
  from-scratch busybox initramfs with their library closures, digest-pinned, and the guest must mount cgroup2
  and devpts and populate /dev. This is a prerequisite slice, proven by a trivial `runc run` in the microVM.
  runc is already a pinned package, so this is not a new dependency, but making it Vesper's workload runtime
  is a security-sensitive change that this ADR records.
- **The terminal path does not reach zero blocked code.** A guest process must still receive the terminal
  master over `--console-socket` via `SCM_RIGHTS`, relay its bytes over vsock, and apply resize with
  `TIOCSWINSZ`. This is the blocked class, narrowed but not removed. The receiver choice is an open decision
  for the terminal slice: adopt runc's upstream `recvtty` (a contrib demo, terminal-only, not in the package
  index, so vendored-and-extended) or hand-write it (blocked for the model). This is to be decided when the
  terminal slice is reached and verified on hardware.
- **Debuggability regresses** against TD-007 (no structured logging) and TD-026 (which wants the socat
  pattern retired at this phase). Required mitigations: keep a ttyS0 breadcrumb (the runc invocation line,
  runc stderr on failure, a `VESPER-WORKLOAD-EXIT <n>` marker); stamp one correlation id across shim session,
  guest container and vsock port; and surface a structured error rather than a bare EOF when runc fails.
- **Pause and snapshot parity is unresolved.** runc adds a state directory, a cgroup freezer and an open
  console socket that a Firecracker memory snapshot cannot cleanly restore, the same failure class seen with
  firecracker-containerd. This interaction is deferred and must be decided before pause/snapshot parity is
  claimed with runc in the path.
- **Overhead at 256 MiB is unverified** (TD-033); confirm runc fits the per-microVM budget before committing
  the terminal slice estimate.

## Implementation and migration impact

In order:

1. **runc-enablement prerequisite.** `vesper-microvm prepare` copies runc, libseccomp and the receiver (and
   optionally tini-static) plus their closures into the initramfs, digest-pinned; the init mounts cgroup2 and
   devpts, populates /dev and makes the new root private. Capture `zcat /proc/config.gz` as kernel evidence.
   Prove a trivial `runc run` of a hello-world.
2. **Non-interactive exec (slice 1).** Host-side config.json generation in `shim/internal/backend` and
   `shim/internal/bundle`; per-stream socat bridges; the shim session code (vsock client over the existing
   `CONNECT <port>` preface, exit mapping) replacing the console-log follower in `shim/internal/task` for the
   running path; the console log stays for boot diagnostics. This is model-deliverable, to be verified.
3. **Interactive terminal and resize (slice 2).** The terminal-master receiver (`recvtty` or hand-written)
   and `ResizePty`, plus the `Exec`/`Start(execID)`/`Kill(execID)`/`Wait(execID)`/`State(execID)` methods.

The WOR-32 ExecPlan is rewritten to this staged design and its Decision Log entry for the custom agent is
reversed. ADR 0004 (shim in Go) stands; this ADR revises only the per-task execution and stdio path it
described as P3.2 limits.

## References

- GH #23 / WOR-32; [2026-11-09 guest-exec brainstorm](../2026-11-09-wor-32-guest-exec-brainstorm.md) (two swarms, six lenses).
- ADR 0003 (control channel, socat agent), ADR 0004 (shim in Go), ADR 0005 (ports via vsock gateway).
- design-v0 "Phase 3"; TD-005, TD-007, TD-026, TD-033, TD-034, TD-035.
- Code: `systemvm/overlay/usr/local/bin/vesper-microvm`, `shim/internal/{backend,bundle,task}`,
  `systemvm/overlay/etc/init.d/vesper-port-relay`, `docs/specs/systemvm-packages.lock.yaml`.
