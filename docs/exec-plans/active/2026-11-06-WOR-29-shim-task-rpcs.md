# containerd shim v2 Task lifecycle RPCs (WOR-29 / GH #22)

This ExecPlan is a living document. The sections Progress, Surprises & Discoveries, Decision Log, and
Outcomes & Retrospective must be kept up to date as work proceeds. It is maintained in accordance with
docs/PLANS.md.

## Purpose / Big Picture

GitHub #22 (Linear WOR-29, mirror #70) asks for the shim v2 Task lifecycle (Create, Start, Kill, Delete, State,
Wait) mapped deterministically to backend microVM operations, with clean teardown, so `ctr` task commands work
end to end. After this plan `ctr run --runtime io.containerd.vesper.v2` inside the System VM boots a jailed
Firecracker microVM from the container's rootfs, returns the command's output and exit code, and
`make systemvm-shim-suite` proves the GH #22 checklist.

## Progress

- [x] (2026-11-06 21:45Z) Go module `shim/` with `containerd-shim-vesper-v2` (ADR 0004): manager (start,
  delete-after-crash, info), microVM backend (`mke2fs -d` root drive, `vesper-microvm launch --rootfs --cmd`,
  stop, cleanup, status), Task service (one task per shim, state machine, console-log follower copying the
  workload output to the task stdout and reading the exit marker, events, `NotImplemented` for P3.2 RPCs);
  unit tests for the state machine, command encoding and follower. `scripts/systemvm/build-shim.sh`;
  overlay ships the static binary to `/usr/bin`; guest `vesper-ctr` driver; agent `CTR`; `run_shim_suite`;
  `systemvmctl control ctr|shim-suite`; `make shim-build shim-test systemvm-shim-suite`; `Shim Tests`
  workflow; Go in the image workflow. Suite 21/21 on an M3 Pro; workload, microvm, rootfs and parity suites
  still pass.
- [ ] PR merged, CI green, GH #22 checklist and WOR-29 updated, milestone entry; plan moved to completed.

## Surprises & Discoveries

- Observation: the first run failed every task with `sh: syntax error: unterminated quoted string`: the
  line-based agent protocol splits on whitespace, so quoted command strings reached `ctr` as separate argv
  words. The guest driver now runs the remaining words as one `sh -c` string.
- Observation: a workload that exits before `vesper-microvm launch` returns leaves `pid=` empty; the backend
  now treats that as pid 0 instead of an error and the follower reads the exit marker.
- Observation: `containerd/api@latest` (1.12) does not compile with containerd 2.1.5; the module pins the
  versions the containerd release itself requires.

## Decision Log

- Decision: Go for the shim, cross-built on the host, shipped in the overlay (ADR 0004).
- Decision: one shim per task; `Delete` shuts the shim down.
  Rationale: matches containerd's default (no grouping annotation) and keeps cleanup local.
  Date/Author: 2026-11-06, agent.
- Decision: the bundle rootfs becomes an ext4 image on every `Create` (no cache); caching and block-first
  preparation are #24 (P3.3).

## Outcomes & Retrospective

See Progress; `run/WOR-29/systemvm/shim-report.json` carries the 21 steps. Gaps: TD-034 (shell required in
the image, stdin/terminal/exec unsupported, host sees only the VMM pid, no rootfs cache).

## Context and Orientation

`shim/` (see `shim/README.md`), `systemvm/overlay/usr/local/bin/vesper-ctr`, `src/control/suites.rs`
(`run_shim_suite`). A *task* is containerd's running instance of a container; here it is one microVM.

## Plan of Work

Done in one slice; see Progress.

## Concrete Steps

    cd <repo root>
    make shim-test && make systemvm-prepare && make systemvm-up
    make systemvm-shim-suite
    cargo run --bin systemvmctl -- control ctr run demo echo hello
    make systemvm-down

## Validation and Acceptance

`make shim-test`, `cargo test control`, `make systemvm-shim-suite` with `passed: true` (e2e exit code and
output, running state with a pid, kill, delete, three invalid sequences rejected, three concurrent tasks,
no leftovers), sibling suites still passing, docs and structure sensors, CI.

## Idempotence and Recovery

The suite force-deletes its task names first; `vesper-ctr leftovers` reports anything a failed run left
(`DIRTY …`), and `vesper-microvm cleanup <id>` plus `ctr task delete -f` recover by hand.

## Artifacts and Notes

    ok   e2e_run_exit_code_and_output: ctr run --rm exited 3; output ["hello-from-task"]
    ok   state_running: STATE det RUNNING pid=3412
    ok   invalid_delete_running: rc 1: ctr: task must be stopped before deletion: running: failed precondition
    ok   three_tasks_running: ca, cb, cc RUNNING with pids {"3645", "3877", "3731"}
    ok   no_leftovers: CLEAN

## Interfaces and Dependencies

Go 1.27 toolchain (host and CI); `shim/go.mod` pins containerd 2.1.5 libraries (Apache-2.0). Guest: `ctr`,
`mke2fs`, `vesper-microvm`. `vesper::control::{Session::ctr, run_shim_suite}`; agent `CTR`.
