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
- [x] (2026-11-09, local hardware) runc prerequisite: initramfs runtime closure, cgroup2/devpts/devices,
  private mounts and digest verification. A 256 MiB microVM printed `VESPER-RUNC-HELLO` and exit 0.
  The smoke test initially reported 163020 KiB available. The pinned kernel lacks `/proc/config.gz`;
  use its matching ISO config as evidence.
- [x] (2026-11-09, local hardware) Non-terminal sessions: OCI configs remain outside the rootfs cache,
  three socat stream ports carry stdin/stdout/stderr, and runc returns workload status.
  Exec works after `/bin/sh` is removed. Existing shim lifecycle checks passed (21 steps).
- [x] (2026-11-09, local hardware) Terminal receiver: `vesper-console` reuses pinned containerd console
  libraries. Interactive input, init and exec terminal resize from 24×80 to 37×101, and exit 7 were observed.
- [x] (2026-11-09, local hardware) Signals: init TERM → 143, trapped TERM → 0 with marker and KILL → 137.
  The exec signal proof initially used `ctr -s 0`, which ctr rejects before the shim; use a readiness marker.
- [x] (2026-11-09) Observability: session `p<base-port>` links stream ports, the guest directory and console
  invocation/exit markers. Startup failures return `ERROR <JSON>` with runc diagnostics.
- [x] (2026-11-09) Council review: two independent reviewers traced protocol, resources, lifecycle and events.
  Fixed output FIFO shutdown, failed-start abort, child reaping, init-last exit events, pre-start CloseIO,
  signal-zero phase checks and a bounded abort watchdog. Hardware regressions cover orphan cleanup.
- [x] (2026-11-09, local hardware) `make ctr-conformance CTR_FLAGS=--publish`: 74/74 checks passed,
  including 19 exec checks. The committed capture contains the baseline lock revisions.
- [x] (2026-11-09, local hardware) Prerequisite captured the complete pinned kernel configuration, runc
  hello-world and exit 0, with 160304 KiB available in a 256 MiB microVM. Legacy microVM, rootfs,
  workload and control suites passed. Rust, Go, race, shell protocol, contract, matrix and docs checks passed.
- [x] (2026-11-09, local hardware) Fresh-boot services suite passed all nine checks. The earlier
  post-conformance failure was the bounded startup-log window, not failure to recover a service.
- [x] (2026-11-09) Clean-source conformance captured at `74e6544`: 74/74; all nine source CI checks green.
- [x] (2026-11-09, M3 Pro) Reliability: 50/50 boots, maximum 2846 ms against 5000 ms; control, services,
  Docker smoke and leak checks passed. No leaked launchers, proxies, sockets or pid files.
- [x] (2026-11-09) Evidence bundle collected at `.symphony/runs/WOR-32/current/`; publish it with the issue evidence update.
- [ ] PR #145 reviewed and merged; move this plan to completed after landing. GitHub #23/#72 and
  WOR-32 were already closed before this implementation; add evidence comments without advancing their state.

## Surprises & Discoveries

- Observation: host clap needs `--` before forwarded guest flags.
- Observation: runc has no `wait` command. Foreground run/exec return status; terminal sessions require
  detached execution to use `--console-socket`, so the local terminal helper also reaps their status.
- Observation: runc 1.3.4 rejects exec with an empty namespace list (`ns paths are empty`). Joining the
  existing `/proc/1/ns/net` resolves it without creating a namespace. Hardware exec then returned 7.
- Observation: runc needs a regular `/dev/console` mount target. The workload receives a separate tmpfs
  device tree and shared devpts rather than a recursive bind of the whole guest `/dev`.
- Observation: the kernel lacks `/proc/config.gz` and a configs module. The matching config is on the pinned
  ISO at `boot/config-6.12.115-0-virt`; the smoke path emits it instead.
- Observation: ctr rejects signal 0 in its own parser. The shim still handles the RPC, tested by phase checks;
  the exec signal hardware probe waits for application output before sending KILL.

- Observation: a broad conformance run exposed a pre-existing Delete/Shutdown race. Calling shutdown
  inside Delete could close ttrpc before its reply. Containerd already calls Shutdown after Delete;
  the implementation now follows that order and tests it.

- Observation: the reliability script's summary used invalid Python quoting, although the JSON report passed.
  Replace that print expression with `str.format` and validate it against the captured 50-boot report.

Known limits (review 2026-11-09):

- The workload's requested in-guest security posture is overridden, not honored: the full capability set is
  always granted and no-new-privileges / LSM labels are cleared. Tracked as compat exception CE-0004 (needs
  human approval before GA); the microVM is the isolation boundary (ADR 0006).
- `SIGNAL` to an exec session targets the single exec pid and ignores the `all` flag (only an init `kill`
  uses `runc kill --all`), so a signal does not reach an exec's descendant processes.
- Killing a process in the narrow window after the guest writes its exit status but before the shim watcher
  records the exit can surface a transient "session-not-running" error rather than a no-op; containerd
  tolerates a kill that races a natural exit.

## Decision Log

- Decision (2026-11-09): implement ADR 0006 at the owner's request. Supersede the custom Go spawning agent
  with host-generated OCI configs, runc and three independent socat streams.
- Decision (2026-11-09): use the already pinned `containerd/go-runc` receiver and `containerd/console` poller
  in a Unix-socket-only terminal helper. No new dependency or vendored receiver is needed.
- Decision (2026-11-09): use tini-static as microVM PID 1. Remove `runc run --keep`; destroy a terminal init's
  runc container before draining output. These steps prevent live execs or inherited terminals from delaying init exit.
- Decision (2026-11-09): explicitly join the existing guest network namespace to avoid runc's empty-list bug.
- Reversed (2026-11-08): the custom vsock Go agent and multiplexed framing. ADR 0006 replaces that design.

## Outcomes & Retrospective

Implementation commit `74e6544` passed all nine CI checks and the clean-source 74-step conformance capture.
The 50-boot reliability report passed with zero failures or leaks. PR #145 awaits review and merge.
TD-041 records bounded exec-port allocation. TD-042 records unresolved pause/snapshot compatibility.
TD-034 is resolved for the shim; the legacy `vesper-workload --cmd` diagnostic path remains unchanged.

## Context and Orientation

`systemvm/overlay/usr/local/bin/vesper-microvm` (`--vsock`, init, `prepare`), `shim/internal/{backend,bundle,
task}` (ADR 0004), `systemvm/overlay/etc/init.d/vesper-port-relay` (socat idiom), `docs/specs/systemvm-packages.lock.yaml`
(runc 1.3.4-r1, socat, tini-static), Firecracker vsock semantics in `third_party/firecracker/firecracker.yaml`.
ADR 0006 is the governing decision; `docs/design-docs/2026-11-09-wor-32-guest-exec-brainstorm.md` holds the brainstorm analysis.

## Plan of Work

1. runc-enablement prerequisite (kernel evidence, initramfs members, init mounts, trivial `runc run`).
2. Slice 1: host-side config.json, per-stream socat bridges, shim session code, exit/signal mapping.
3. Slice 2: terminal-master receiver using pinned containerd libraries and resize; exec-session methods.
4. Observability wiring; proof driver and suite; docs and landing.

## Concrete Steps

    make shim-test shim-build
    make systemvm-overlay
    make systemvm-up
    make systemvm-exec-suite
    make ctr-conformance CTR_FLAGS=--publish
    python3 harness/check_docs.py

The runtime suite reports 19 steps, including init and exec terminals, input, separated streams,
structured errors, signal exits and cleanup of descendants. Before booting a rebuilt overlay,
stop the old System VM with `make systemvm-down`; preserve any evidence first.


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

The capture `docs/specs/ctr-conformance-baseline.json` includes 21 shim, 16 bundle, 18 restart and 19 exec checks.
Raw command logs, the full kernel config and failed-run diagnostics are preserved in `run/WOR-32/systemvm/`.
The council report is in `run/WOR-32/council/review.md` and is copied into the verification bundle.
A services probe after conformance found only one startup in the last 200 containerd log lines.
The services recovered in that run; a fresh-boot services run passed all nine checks.
`run_services_suite` reads only the last 200 log lines, so run this sensor on a fresh boot after long conformance runs.


    LAUNCHED vs1 pid=2834 boot_ms=1510
    RUNNING vs1 pid=2834 vsock=present
    VESPER-VSOCK-OK
    "vsock":{"guest_cid":3,"uds_path":"/v.sock"}

## Interfaces and Dependencies

Guest kernel modules from the pinned modloop; Firecracker v1.16.0 vsock device; runc 1.3.4-r1, socat and
tini-static (pinned in the System VM lock, to be added as microVM initramfs members with their closures);
libseccomp for runc. The terminal receiver uses the existing pinned `containerd/go-runc` and
`containerd/console` libraries. Pause/snapshot interaction with runc state
is deferred (ADR 0006).
