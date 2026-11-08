# Backend A parity and performance baseline (WOR-26 / GH #21)

This ExecPlan is a living document. The sections Progress, Surprises & Discoveries, Decision Log, and
Outcomes & Retrospective must be kept up to date as work proceeds. It is maintained in accordance with
docs/PLANS.md.

## Purpose / Big Picture

GitHub #21 (Linear WOR-26, mirror #63) asks for an automation entrypoint that exercises upstream Firecracker at
the locked revision, a report with the exact baseline ids and a compat exception summary, and performance
captures that are comparable across repeat runs. After this plan `make systemvm-parity-baseline` runs an API
conformance checklist against the stock release Firecracker inside the System VM, two performance runs, and
writes `run/systemvm/parity-baseline-report.json`; the committed capture `docs/specs/backend-a-parity-baseline.json`
is re-validated against the lock, the archive manifest and the compat registry by the `Parity Baseline` workflow,
which publishes it as a CI artifact. The gaps to the design bar (the upstream pytest suite, the release-vs-commit
binary, the missing Linux comparison) are registered as compat exceptions CE-0001 to CE-0003.

## Progress

- [x] (2026-11-06 21:30Z) Guest `vesper-fcparity`: `api ID METHOD PATH [BODY|-] [TIMEOUT]` (socat over the jailed
  API socket, FIFO-fed so the request side stays open until the response is complete), `conformance ID` (19
  checks: launch, version equals the binary's, instance Running with the right id, machine config, full VM config,
  400 faults for an invalid path, a bad body, post-boot machine-config, an unknown drive patch and a snapshot while
  running, pause, Paused state, full snapshot while paused with a 256 MiB memory file and a non-empty vmstate,
  resume, Running again, SIGTERM stop, clean jail), `perf N` (sequential launch/exit/cleanup cycles; boot p50/p95/
  max, cycle p50, create rate per minute). Agent `FCPARITY`; `control::Session::fcparity`; `run_parity_suite`
  (conformance steps, `perf_run_1`, `perf_run_2`, `perf_comparable` within 20 %); `systemvmctl control
  fcparity|parity-suite`; `scripts/systemvm/parity-baseline.sh` and `parity_baseline_report.py` (assemble and
  `--validate`, self-tested); `make systemvm-parity-baseline` and `make parity-baseline-check`; the `Parity
  Baseline` workflow; compat exceptions CE-0001..CE-0003; microVM cgroup memory cap raised to 640 MiB.
  Capture on an M3 Pro: conformance 19/19; boot p50 1510/1520 ms, p95 1520 ms, 38.8/38.6 creates per minute over
  two runs of 20, drift 0.7 %.
- [x] (2026-11-06 22:05Z) PR #131 merged as `c50effa`, CI green (eight workflows incl. Parity Baseline), GH #21 checklist items 2-4 ticked (item 1, the upstream pytest suite, stays open under CE-0001), WOR-26 in review pending human approval of CE-0001..CE-0003, milestone entry; plan moved to completed.

## Surprises & Discoveries

- Observation: `PUT /snapshot/create` while paused killed the microVM silently. The jailer cgroup capped memory
  at 256 MiB, the guest already used 256 MiB, and the snapshot's 256 MiB memory file lands in tmpfs charged to
  the same cgroup. Raising the cap to 640 MiB fixed it.
  Evidence: `EXITED p2` after the request, nothing logged after "received a Put request on /snapshot/create".
- Observation: Firecracker's HTTP server resets the connection when the client half-closes after sending the
  request, so `printf | socat` never saw a response; the request side must stay open.
  Evidence: `socat … Connection reset by peer` in the first console probe.
- Observation: an empty argument cannot travel over the line-based agent protocol; `-` stands for an empty body.

## Decision Log

- Decision: parity confidence for this milestone is an API conformance subset against the stock release
  binary, not the upstream pytest suite; the gap is CE-0001, the release-vs-commit binary CE-0002 (TD-029) and
  the missing Linux comparison CE-0003.
  Rationale: the upstream suite needs the Firecracker source, its Python harness, artifacts from S3 and a
  non-nested KVM host; none exists here yet. Registering the gaps keeps the acceptance criterion honest.
  Date/Author: 2026-11-06, agent.
- Decision: CI validates and publishes the committed capture rather than producing one.
  Rationale: GitHub runners cannot boot the System VM (TD-005); validating the capture against the current lock,
  archives and registry catches drift, and the artifact carries the exact revisions.
  Date/Author: 2026-11-06, agent.
- Decision: comparability means both runs without failures and boot p50 within 20 % of each other.
  Rationale: matches the design's 20 % tolerance; p50 is stable on this host (0.7 % drift).
  Date/Author: 2026-11-06, agent.

## Outcomes & Retrospective

See Progress; `run/WOR-26/systemvm/parity-baseline-report.json` is the capture. The compat exceptions need a
human's approval as pre-GA exceptions (they are `open` with expiry 2027-03-31).
Approved by cityhunteur on 2026-11-08 (`approved_by`/`approved_on` in the registry); WOR-26 and the Phase 2 epic WOR-14 closed.

## Context and Orientation

`systemvm/overlay/usr/local/bin/vesper-fcparity`, `src/control/suites.rs` (`run_parity_suite`),
`scripts/systemvm/parity-baseline.sh`, `scripts/systemvm/parity_baseline_report.py`,
`.github/workflows/parity-baseline.yml`, `docs/specs/compat-exceptions.yaml`,
`docs/specs/backend-a-parity-baseline.json`.

## Plan of Work

Done in one slice; see Progress.

## Concrete Steps

    cd <repo root>
    make systemvm-prepare && make systemvm-up
    make systemvm-parity-baseline PERF_CYCLES=20 PARITY_FLAGS=--publish
    make parity-baseline-check
    make systemvm-down

## Validation and Acceptance

`cargo test control`, `parity_baseline_report.py --self-test`, `make systemvm-parity-baseline` passing,
`make parity-baseline-check` passing on the committed capture, the microvm, workload and rootfs suites still
passing with the new memory cap, docs and structure sensors, CI (including the new workflow).

## Idempotence and Recovery

Every run cleans its microVM ids first (`vesper-parity`, `perf-N`); re-running overwrites the run reports;
`--publish` overwrites the committed capture only when the run passed.

## Artifacts and Notes

    ok   conformance_summary: passed=19 failed=0 firecracker_version=1.16.0
    ok   perf_run_1: n=20 boot_p50_ms=1510 boot_p95_ms=1520 boot_max_ms=1520 cycle_p50_ms=1541 create_rate_per_min=38.8 wall_ms=30960 failures=0
    ok   perf_run_2: n=20 boot_p50_ms=1520 boot_p95_ms=1520 boot_max_ms=1520 cycle_p50_ms=1550 create_rate_per_min=38.6 wall_ms=31060 failures=0
    ok   perf_comparable: boot p50 1510 ms vs 1520 ms, drift 0.7% (limit 20%)

## Interfaces and Dependencies

`vesper::control::{Session::fcparity, run_parity_suite}`; agent `FCPARITY`; guest `vesper-fcparity`;
`parity_baseline_report.py` (standard library). No new packages or crate dependencies.
