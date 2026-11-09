---
status: draft
owner: cityhunteur
last_reviewed: 2026-11-09
---

# WOR-32 guest execution: can delegating to runc shrink the blocked surface?

* **Author:** cityhunteur
* **Date:** 2026-11-09
* **Status:** Brainstorm (concepts brief). Not a design of record. The architecture is an open decision; a superseding ADR and a rewritten ExecPlan must follow before any code.
* **Reviewed by:** two adversarial EGM brainstorm swarms, twelve reports across six lenses (design, codebase-details, zoom-out, comprehension, implementation-readiness, structural), each grounded in the current code.

## Overview

WOR-32 (GH #23) needs exec sessions, a pseudo-terminal with resize, separated stdin/stdout/stderr,
exit status and signal propagation that behave as containerd expects. Today the shim drives the task
through the kernel command line and a one-way console log, which cannot carry stdin, a second stream,
a terminal or a signal. The vsock transport to reach inside a microVM landed in PR #133.

The original plan (ADR 0004, the active ExecPlan) is a custom Go guest agent inside the microVM that
spawns the container process and owns its terminal, stdio and signals over vsock. A safety control blocks
the model from writing that component, because a socket-driven process spawner with terminal and signal
control is the same shape as a remote-shell implant, regardless of the legitimate use.

The idea under review is to delegate the sensitive mechanisms to **runc** (already pinned in the guest
package set), driven by a static shell init, with stdio carried over vsock, so the human-written,
classifier-sensitive surface shrinks toward zero. The brainstorm tested whether that actually holds.

## The headline finding

**runc narrows the blocked surface but does not eliminate it, and the reviewers split on whether the
narrowing is worth the cost.** This is the decision the brief hands back, not a settled recommendation.

runc genuinely removes the kernel-facing code the model cannot write: clone, namespace setup, pivot_root,
exec and the seccomp install. What runc does **not** remove, for the interactive terminal path:

* A guest process must still receive the terminal master descriptor from runc over a Unix socket
  (`--console-socket`, an `SCM_RIGHTS` hand-off), relay its bytes over vsock, and apply resize with an
  `ioctl`. That receiver is a network-driven terminal controller, the same class of code the block
  targets. runc has no resize command; whoever holds the master does it.
* Keeping stdout and stderr apart needs either three vsock connections with a convention someone writes,
  or a framed protocol. socat carries one byte pipe and cannot do either.

So for the **non-interactive** case the blocked surface may reach zero (runc plus off-the-shelf socat
bridges plus a static shell init plus host-side Go). For the **terminal** case it does not, unless an
upstream receiver is adopted, and that is contested (below). Several lenses concluded the custom agent is
simpler and more debuggable, and that the right response to the block is human authorship and review, for
which the repo already has a path (the CE-0001..CE-0003 approvals).

## Analysis

Risks and corrections first. These are agreed across both swarms unless noted.

### socat cannot carry the hard parts (unanimous)

"Off-the-shelf socat bridge carrying stdio and the runc console-socket FD" does not hold. The terminal
master is a kernel descriptor passed via `SCM_RIGHTS`; socat relays bytes, not descriptors, and a
descriptor cannot cross the VM boundary at all. socat also cannot separate stderr from stdout, and has no
side channel for resize or signals. The existing `vesper-port-relay` proves only the vsock-plus-socat
byte-pipe idiom, not an exec protocol.

### runc and socat are in the System VM, not the microVM (packaging cost)

runc 1.3.4-r1 and socat are pinned and digest-verified in the outer System VM overlay. The microVM guest
is a from-scratch busybox initramfs plus the per-task `rootfs.ext4`; neither binary is present there.
runc, `libseccomp` and a terminal receiver must be injected into the initramfs with their library
closures, digest-pinned, the way `vesper-microvm prepare` already copies busybox and kernel modules. This
is real build work and a boot-size and latency cost on a path that boots in about 1.5 s. Correction to an
earlier claim: runc is therefore not a new dependency, so the "ask a human before adding a dependency"
gate does not apply; but making runc Vesper's own workload runtime is a security-sensitive,
process-lifecycle decision that still needs an ADR and runtime evidence. `catatonit` and `crun` are not
in the package set; only `tini-static` is.

### The current model does not nest; if runc is used, use it as a bare supervisor

The shipped workload runs as `chroot /newroot /bin/sh -c "$cmd"` as guest PID 1, with no namespaces,
cgroups or pivot_root inside the guest. The microVM boundary is the container boundary. A full OCI sandbox
from runc would double-isolate a VM holding one workload, add a second failure surface, and complicate the
shim's cgroup `Stats` path. If runc is adopted, run it as a minimal host-namespaced supervisor: no new
namespaces, no second seccomp profile, full capabilities, `root.path` at the mounted ext4. This also keeps
the host-side config.json small, since there is no namespace or seccomp translation to do. The
counter-view (zoom-out, YAGNI) is that a supervisor this thin is barely more than the existing chroot, so
runc earns little and the custom agent is the smaller system overall.

### Kernel and rootfs cache

* **Kernel:** strongly evidenced, not pinned. The microVM boots the same Alpine linux-virt kernel as the
  System VM, which already runs runc with interactive terminal exec (GH #31), so namespaces, cgroup v2,
  seccomp and devpts are present. But no kernel config is pinned in the repo as evidence. Capture
  `zcat /proc/config.gz` from the guest before committing. The guest-side runtime wiring (mount cgroup2,
  devpts, populate /dev, make the new root private) is definitely absent and is new work.
* **Cache (TD-035):** untouched by structure. config.json sits beside the rootfs, never inside it, so the
  cached ext4 stays byte-identical and the layer-chain key is unaffected. Generate config.json host-side
  and send it over the vsock start frame; the guest writes the bytes verbatim and never interprets them.
  Do not bake per-task config into the shared cached image.

### A runc-enablement prerequisite exists (readiness, codebase)

Before any acceptance work, a prerequisite step must land and be proven: verify the kernel config; mount
cgroup2 and devpts and populate /dev in the init; ship runc, libseccomp and the receiver into the
initramfs; and prove a trivial `runc run` of a hello-world inside the microVM. This is a slice of its own,
not folded into the feature.

### Debuggability regresses, against the project's own ledger

The repo has no structured logging, metrics or tracing (TD-007), and TD-026 explicitly wants the
socat-per-connection pattern retired exactly when the shim reaches Phase 3, which is now. A socat-plus-runc
path makes a hung exec harder to triage at 3 a.m. than a typed agent with per-session logging: the process
tree is anonymous forks, the descriptor hand-off leaves no log line, and runc's own stderr (bad entrypoint,
cgroup or seccomp failure) can be swallowed into a closed pipe instead of surfacing as the canonical runc
error. Required mitigations if runc is chosen: keep a ttyS0 breadcrumb (runc invocation line, runc stderr
on failure, a `VESPER-WORKLOAD-EXIT <n>` marker); stamp one correlation id across shim session, guest
container and vsock port; and surface a structured error rather than a bare EOF.

### Exit and signal fidelity must be proven through the chain

The containerd contract (signal to 128+n, SIGKILL to 137, a trap exiting 0, a real exit 7) must survive
every hop. With `runc run` the code is available, but a shell wrapper, tini or socat in the path can
flatten the encoding. The design must state the exact exit path and prove 137, 143, 0 and 7.

### What runc genuinely buys

Exit status and signal delivery map cleanly onto `runc wait` and `runc kill`, better than today's log-marker
scrape. runc execs argv directly, which fixes TD-034 (the `/bin/sh` dependency for shell-less images).
These are real gains and they hold in either direction for the non-terminal path.

### The irreducible human-written core, by slice

* **Non-interactive exec (separated stdio, exit, signals):** guest side is socat per-stream bridges plus a
  static shell init that runs runc; host side is config.json generation and the shim session code. No
  compiled network-driven spawner is authored by the model. Plausibly fully model-writable, to be
  verified. Requires the runc-enablement prerequisite.
* **Interactive terminal plus resize:** needs the terminal-master receiver (`recvmsg`/`SCM_RIGHTS` plus
  `TIOCSWINSZ`), about 100 to 250 lines, the blocked class. Either adopt runc's upstream `recvtty` (but it
  is a contrib demo, not in the apk, terminal-only, so it is vendored-and-extended, not truly off the
  shelf), or hand-write it (blocked for the model). This is the piece to attempt last and discover, not
  assume.
* **Shim-side session code** (vsock client, exec-session methods, exit mapping): host-side Go, not blocked,
  and identical whether the guest uses runc or a custom agent. Roughly 300 to 700 lines.

Honest total for the full feature: roughly 900 to 1500 lines, of which the terminal receiver (about 150 to
500 depending on scope) is the safety-relevant part that runc narrows but does not remove.

## Goals vs. Non-Goals

### In-scope goals

* A prerequisite runc-enablement step, proven by a trivial `runc run` in the microVM, if the runc direction
  is chosen.
* Non-interactive exec: separated stdin, stdout, stderr; correct exit code; correct signals; TD-034 fixed.
* Interactive terminal and resize as a later, separately-scoped step.

### Out-of-scope non-goals

* A custom multiplexed stdio protocol, if per-stream vsock ports are used instead.
* runc's full isolation config: reduced to a bare supervisor if runc is used.
* Pause and snapshot interaction with runc state: deferred, to be decided in the ADR, not solved here. runc
  adds a state directory, a cgroup freezer and an open console socket that a Firecracker memory snapshot
  cannot cleanly restore, the same failure class seen with firecracker-containerd.
* A dedicated microVM kernel (TD-033): a separate performance and networking want.

## Alternatives Weighed

* **Custom Go framed guest agent (the current ExecPlan, ADR 0004).** One binary owns spawn, terminal,
  stdio mux, resize, signals and exit. About 300 to 450 lines, the whole of it blocked for the model, but
  several lenses judge it the simplest and most debuggable option. Viable if a human authors or reviews it;
  the repo already has a human-approval path. This is a serious contender, not a straw man.
* **Pure socat, no runc.** Rejected on physics: cannot pass the terminal descriptor, separate streams, or
  carry resize and signals, and still leaves spawning to be hand-written.
* **Vendor the Kata or firecracker-containerd agent.** Rejected earlier in session: firecracker-containerd
  is in maintenance mode (containerd 1.7, no releases, agent-dies-after-snapshot reports) and its host side
  mismatches the containerd 2.1 shim; Kata's agent is heavier and assumes the Kata runtime on the host.
* **runc-delegation, bare supervisor, host-side config, per-stream ports (this brief).** Removes the
  kernel-facing spawn/chroot/seccomp code. For non-interactive exec the model-authored surface may be a
  static shell init plus host-side Go, no blocked code. For the terminal path the blocked-class receiver
  remains unless `recvtty` is vendored. Costs a runc-enablement prerequisite, a redundant isolation layer
  (mitigated by the bare-supervisor spec), worse debuggability, and an unresolved snapshot interaction.

## Key Decisions & Staging for Next Steps

The one decision the user must make: **runc-delegation, or a human-authored/reviewed custom agent.** The
reviewers are genuinely split. If the goal is to maximize what the model can deliver alone, runc-delegation
lets it ship non-interactive exec with no blocked code; the terminal path still needs `recvtty` or a human.
If the goal is the simplest, most debuggable system, the custom agent is smaller overall and the block is a
process gate a human can clear.

Decisions to record in a superseding ADR either way:

1. The chosen runtime (runc-delegation or custom agent), superseding ADR 0003/0004 and the ExecPlan
   Decision Log where they commit to the custom Go agent.
2. If runc: bare host-namespaced supervisor, host-side config.json over the start frame, cached ext4 kept
   pure, one vsock port per stream, the runc-enablement prerequisite, and the terminal receiver choice.
3. The pause/snapshot deferral and the firecracker-containerd comparison, so both stop being tribal
   knowledge.
4. The observability mitigations (ttyS0 breadcrumb, correlation id, structured errors) against TD-007 and
   TD-026.

Staging if runc-delegation is chosen:

* Prerequisite: runc-enablement, proven by a trivial `runc run`.
* Slice 1: non-interactive exec (model-deliverable, to be verified).
* Slice 2: terminal and resize via `recvtty` or a human-written receiver; verify the console-socket path on
  hardware and confirm runc overhead fits 256 MiB before committing the estimate.
