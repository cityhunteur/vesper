# MicroVM-per-container workload lifecycle (WOR-30 / GH #20)

This ExecPlan is a living document. The sections Progress, Surprises & Discoveries, Decision Log, and
Outcomes & Retrospective must be kept up to date as work proceeds. It is maintained in accordance with
docs/PLANS.md.

## Purpose / Big Picture

GitHub #20 (Linear WOR-30, mirror #69) asks for a 1:1 mapping between a container-style workload and a jailed
Firecracker microVM: run, list, logs, wait, stop and remove; isolation between concurrent microVMs; several microVMs
at once without resource conflicts; stop tears the microVM down. After this plan `make systemvm-up && make
systemvm-workload-suite` runs three workloads concurrently in three microVMs, proves their isolation, stops one
without touching the others, maps a short workload's exit code and output back, and removes everything.

## Progress

- [x] (2026-11-06 21:00Z) Guest `vesper-workload` (`run NAME IMAGE -- CMD…`, `ps`, `logs`, `wait`, `stop`, `rm`,
  `isolation`): each workload gets a private copy of the image's cached rootfs under `/var/lib/vesper/workloads/`
  and its own jailed microVM `wl-NAME`; the command travels base64-encoded on the kernel command line
  (`vesper_cmd=`), the microVM init chroots into the mounted rootfs, prints `VESPER-WORKLOAD-START`, the output and
  `VESPER-WORKLOAD-EXIT <code>`, then powers off. Agent `WORKLOAD`; `control::Session::workload`;
  `run_workload_suite`; `systemvmctl control workload|workload-suite`; `make systemvm-workload-suite`. The trivial
  image now carries every busybox applet. Suite 16/16; microvm 13/13 and rootfs 11/11 still pass.
- [x] (2026-11-06 21:40Z) PR #130 merged as `2475a4f`, CI green, GH #20 checklist and WOR-30 updated, milestone entry; plan moved to completed.

## Surprises & Discoveries

- Observation: the first run had every workload exit at once with code 127: the trivial image only linked the
  `sh` applet, so `sleep` did not exist inside the microVM. The launch pid also read as `?` because the microVM
  had already powered off before `LAUNCHED` was printed.
  Evidence: `sh: sleep: not found` in `/var/log/vesper-microvm-wl-dbg.log`.

## Decision Log

- Decision: a workload's rootfs is a private copy of the cached image rootfs, not the shared file.
  Rationale: Firecracker opens the drive read-write; copying gives each microVM its own writable disk and makes
  "wb cannot see wa's /tmp/x" a real isolation check. Copy-on-write snapshots are future work (TD-032).
  Date/Author: 2026-11-06, agent.
- Decision: the workload command is passed on the kernel command line, base64-encoded, rather than through a
  vsock or a second drive.
  Rationale: the microVM has no network or vsock yet; the command line is already parsed by the init and keeps
  the slice small. Size is bounded by the kernel command-line limit (TD-032).
  Date/Author: 2026-11-06, agent.
- Decision: state is derived, not stored: RUNNING comes from the microVM manager, EXITED from the console log's
  exit marker, STOPPED from a record without either.
  Rationale: no daemon to keep in sync; the console log is the durable record.
  Date/Author: 2026-11-06, agent.

## Outcomes & Retrospective

See Progress; `run/WOR-30/systemvm/workload-report.json` carries the 16 steps. Gaps: TD-032 (copied rootfs, no
CoW; command-line bound; no network; no resource accounting beyond the per-microVM cgroup limits).

## Context and Orientation

`systemvm/overlay/usr/local/bin/vesper-workload`, `vesper-microvm` (`--cmd`), `src/control/suites.rs`
(`run_workload_suite`). A *workload* is one command in one microVM with one rootfs copy.

## Plan of Work

Done in one slice; see Progress.

## Concrete Steps

    cd <repo root>
    make systemvm-prepare && make systemvm-up
    make systemvm-workload-suite
    cargo run --bin systemvmctl -- control workload run demo vesper/trivial:guest -- sh -c "'echo hi'"
    cargo run --bin systemvmctl -- control workload wait demo
    make systemvm-down

## Validation and Acceptance

`cargo test control`, `make systemvm-workload-suite` with `passed: true`, the microvm and rootfs suites still
passing, docs and structure sensors, CI. Issue exit: the four GH #20 checklist items proven by the report.

## Idempotence and Recovery

`run` refuses an existing name; `rm` is safe to repeat (it reports `ERR unknown workload` once gone); the suite
removes its names first. The workload store lives in the guest's tmpfs.

## Artifacts and Notes

    ok   three_microvms_running: 3 running, pids {"3187", "3288", "3388"}
    ok   isolation_between_microvms: 3 distinct cgroups {/firecracker/wl-wa, wl-wc, wl-wb}; wb cannot see wa's /tmp/x
    ok   stop_wa_tears_down_its_microvm: STOPPED wa teardown=CLEAN
    ok   others_unaffected_by_stop: wa STOPPED, wb and wc still RUNNING, wl-wa CLEAN
    ok   wait_maps_exit_code_and_output: EXITED wd code=3; output hello-from-microvm; microVM torn down

## Interfaces and Dependencies

`vesper::control::{Session::workload, run_workload_suite}`; agent `WORKLOAD`; guest `vesper-workload`;
`vesper-microvm launch ID --rootfs PATH --cmd BASE64`. No new packages or crate dependencies.
