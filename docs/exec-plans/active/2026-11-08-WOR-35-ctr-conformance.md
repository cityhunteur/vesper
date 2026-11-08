# ctr conformance suite, CI target and operator runbook (WOR-35 / GH #26)

This ExecPlan is a living document. The sections Progress, Surprises & Discoveries, Decision Log, and
Outcomes & Retrospective must be kept up to date as work proceeds. It is maintained in accordance with
docs/PLANS.md.

## Purpose / Big Picture

GitHub #26 (Linear WOR-35, mirror #79) asks for command-level conformance tests of the runtime, a CI
integration target, and operator documentation covering failure modes and recovery. After this plan
`make ctr-conformance` runs the shim, bundle and restart suites (55 steps) and assembles a report stamped with
the baseline revisions, the `ctr Conformance` workflow validates and publishes the committed capture, and
`docs/process/shim-operations.md` is the runbook.

## Progress

- [x] (2026-11-08) Shim `Stats` reads the microVM's cgroup v2 files (`memory.*`, `cpu.stat`, `pids.*`) into
  containerd's metrics message, so `ctr task metrics` works (Linux build; a stub keeps host tests building);
  `vesper-ctr metrics NAME`; shim suite step `task_metrics` (21 steps). `scripts/systemvm/ctr-conformance.sh`
  + `ctr_conformance_report.py` (assemble with lock revisions, `baselinectl` metadata, per-suite steps, the
  checklist mapping and open gaps; `--validate`; `--self-test`); `make ctr-conformance` (`CTR_FLAGS=--publish`
  writes `docs/specs/ctr-conformance-baseline.json`) and `make ctr-conformance-check`; `.github/workflows/
  ctr-conformance.yml`; runbook `docs/process/shim-operations.md`. Capture on an M3 Pro: 55/55 steps.
- [ ] PR merged, CI green, GH #26 checklist (`ctr exec` item stays open) and WOR-35 updated, milestone entry;
  plan moved to completed.

## Surprises & Discoveries

- Observation: the `cgroups/v3` manager package does not compile for the cross build in this module set
  (`BPF_*` constants, `pids.Limit` type), so Stats reads the cgroup files directly; only the protobuf stats
  package is used.
- Observation: `ctr task metrics --format json` is pretty-printed; the guest parser strips whitespace first.

## Decision Log

- Decision: conformance equals the three existing suites plus metrics, assembled into one report, rather than
  a new test framework.
  Rationale: the suites already drive `ctr` end to end; the report adds the baseline link and the checklist
  mapping the issue asks for.
  Date/Author: 2026-11-08, agent.
- Decision: CI validates and publishes the committed capture (as for the parity baseline) because runners
  cannot boot the System VM (TD-005).

## Outcomes & Retrospective

See Progress; `run/WOR-35/systemvm/ctr-conformance-report.json` is the capture. Open: `ctr exec` (WOR-32).

## Context and Orientation

`shim/internal/task/stats.go`, `scripts/systemvm/ctr-conformance.sh`, `scripts/systemvm/ctr_conformance_report.py`,
`docs/process/shim-operations.md`, `.github/workflows/ctr-conformance.yml`.

## Plan of Work

Done in one slice; see Progress.

## Concrete Steps

    make systemvm-up
    make ctr-conformance CTR_FLAGS=--publish
    make ctr-conformance-check
    make systemvm-down

## Validation and Acceptance

`make shim-test`, `make ctr-conformance` passing, `make ctr-conformance-check` valid, docs sensors, CI including
the new workflow.

## Idempotence and Recovery

Each suite cleans its own task names; `--publish` only copies a passing report.

## Artifacts and Notes

    ok   task_metrics: ctr task metrics -> METRICS det memory_bytes=74211328 cpu_usec=1363884 pids=3
    ctr conformance passed (55 steps, run/systemvm/ctr-conformance-report.json)
    ctr conformance report valid

## Interfaces and Dependencies

`github.com/containerd/cgroups/v3/cgroup2/stats` (protobuf only, already a dependency). No new packages.
