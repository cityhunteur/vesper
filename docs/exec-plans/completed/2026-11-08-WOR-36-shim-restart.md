# Shim restart resilience and reconciliation (WOR-36 / GH #25)

This ExecPlan is a living document. The sections Progress, Surprises & Discoveries, Decision Log, and
Outcomes & Retrospective must be kept up to date as work proceeds. It is maintained in accordance with
docs/PLANS.md.

## Purpose / Big Picture

GitHub #25 (Linear WOR-36, mirror #75) asks that the shim stay authoritative across a containerd restart,
that state reconciliation restore accurate task status, that in-flight operations resume or fail gracefully,
and that no microVM is orphaned when a shim dies. After this plan `make systemvm-restart-suite` proves all four
through `ctr` inside the System VM.

## Progress

- [x] (2026-11-08) Durable task record `<bundle>/vesper-task.json` (`id, phase, pid, image, exit_status,
  exited_at, shim_pid, updated_at`), written atomically on create, start, exit, kill and delete; the manager's
  `delete` (what containerd runs after a dead shim) reconciles from it: a task that had already exited keeps
  its real exit status and time, a still-running task is reported as killed, and the microVM, jail, mounts and
  drive are removed in every case. Guest `vesper-ctr shim-pid|kill-shim|wait-gone|state-file|inflight-start|
  inflight-result`; `control::run_restart_suite` (`src/control/suites/restart.rs`); `systemvmctl control
  restart-suite`; `make systemvm-restart-suite`. Suite 18/18 on an M3 Pro: a running task survives a SIGKILLed
  and respawned containerd with the same VMM pid and can be killed and deleted afterwards; a SIGKILLed shim
  leads containerd to run `delete`, the task disappears, the microVM is EXITED and the jail CLEAN; an in-flight
  `ctr run --rm` across a containerd restart exits 1 with `error reading from server: EOF` and leaves nothing;
  `vesper-task.json` shows `phase running` with the VMM pid. Shim, bundle, workload and microvm suites pass.
- [x] (2026-11-08) PR #135 merged as `ccf239d`, CI green (seven workflows), GH #25 checklist ticked, mirror #75 closed, WOR-36 Done, milestone entry; plan moved to completed.

## Surprises & Discoveries

- Observation: no reconnect code was needed in the shim. containerd 2.1 records the shim's bootstrap
  parameters in the bundle and reconnects to the live ttrpc socket on start; the shim's in-memory state is the
  source of truth while it lives, and the durable record covers the case where it does not.
- Observation: for the in-flight run the task was absent after the restart, so the interrupted `ctr run`
  had not reached a state containerd would reload; the client failed with EOF and nothing was left behind.

## Decision Log

- Decision: persist a small JSON record per transition rather than a database or a journal.
  Rationale: one task per shim, five transitions, read by exactly one consumer (`delete` after a crash).
  Date/Author: 2026-11-08, agent.
- Decision: a running task whose shim died is reported as exit 137 and torn down, not re-adopted.
  Rationale: containerd has no re-adoption path for a shim that is gone; the console and the VMM have no owner
  left to serve stdio or Wait, so a clean kill is the honest outcome.
  Date/Author: 2026-11-08, agent.

## Outcomes & Retrospective

See Progress; `run/WOR-36/systemvm/restart-report.json` carries the 18 steps. Gap: containerd's own crash
recovery of an in-flight `Create` is containerd behaviour, not Vesper's; the suite only proves the failure
is clean.

## Context and Orientation

`shim/internal/task/state_file.go`, `shim/internal/manager/manager.go` (`Stop`),
`systemvm/overlay/usr/local/bin/vesper-ctr`, `src/control/suites/restart.rs`.

## Plan of Work

Done in one slice; see Progress.

## Concrete Steps

    make shim-test && make systemvm-prepare && make systemvm-up
    make systemvm-restart-suite
    make systemvm-down

## Validation and Acceptance

`make shim-test` (state record round trip), `make systemvm-restart-suite` with `passed: true`, sibling suites
still passing, docs and structure sensors, CI.

## Idempotence and Recovery

The suite force-deletes `rs1`..`rs3` first and cleans their microVMs; containerd's respawn is supervise-daemon's
job (`vesper-services`), so a failed step leaves at worst a task that `vesper-ctr kill|delete` removes.

## Artifacts and Notes

    ok   containerd_killed_and_respawned: containerd pid 2313 SIGKILLed; containerd respawned and healthy
    ok   task_survives_restart: after restart: STATE rs1 RUNNING pid=3365 (same VMM pid)
    ok   task_gone_after_shim_crash: GONE rs2
    ok   no_orphaned_microvm: microVM EXITED rs2; jail CLEAN rs2
    ok   inflight_fails_gracefully: ctr exited 1: ctr: error reading from server: EOF

## Interfaces and Dependencies

No new dependencies. `task.ReadState(bundle)` is the only new cross-package call (manager → task).
