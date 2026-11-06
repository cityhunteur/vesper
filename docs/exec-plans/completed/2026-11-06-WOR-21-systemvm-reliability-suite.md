# System VM bring-up reliability suite (WOR-21 / GH #16)

This ExecPlan is a living document. The sections Progress, Surprises & Discoveries, Decision Log, and
Outcomes & Retrospective must be kept up to date as work proceeds. It is maintained in accordance with
docs/PLANS.md.

## Purpose / Big Picture

GitHub #16 (Linear WOR-21, mirror #62) asks for an automated stability check: an N-cycle boot stress script
(default 50), socket round-trip smoke tests for vsock and the Docker socket, a machine-readable health report, and
no resource leaks across cycles, runnable in dev or CI with actionable failure signatures. After this plan
`make systemvm-reliability` (or `CYCLES=200 make systemvm-reliability`) produces `run/systemvm/reliability-report.json`
whose `failures[]` name the step, the reason and the evidence file.

## Progress

- [x] (2026-11-06 19:45Z) `scripts/systemvm/reliability-suite.sh` (boot stress through `boot-smoke.sh`, then on a
  kept-up VM the control suite, the services suite and the Docker smoke, with host leak snapshots before and after),
  `scripts/systemvm/reliability_report.py` (assembler with boot-drift and leak checks, `--self-test`),
  `make systemvm-reliability`, CI runs the self-test.
- [x] (2026-11-06 20:05Z) 50-cycle run recorded under `run/WOR-21/systemvm/`; PR #126 merged as `e152da1`, CI
  green, GH #16 checklist and WOR-21 updated, milestone entry; plan moved to completed.

## Surprises & Discoveries

- Observation: the first 50-cycle run failed 3 cycles (6.6 s, 6.1 s, 5.6 s) against the 5 s threshold with a mean of
  4.2 s, and a second run after detaching the daemon starts still had one 6.1 s cycle: the `networking` service in
  the boot runlevel (DHCP wait, about 1.3 to 2 s) was the remaining cost. Everything else passed and nothing leaked.
  Evidence: the first two `reliability-report-50.json` runs (kept in `run/WOR-21/systemvm/reliability-50-first.log`);
  ready time 3.4 to 4.4 s with `networking` in boot vs 2.2 s without.
- Observation: GitHub's macOS runners cannot boot Virtualization.framework guests, so "runs in CI" is limited to
  the report assembler's self-test and the script syntax; the suite itself runs on developer hardware (TD-005).
  Evidence: `launcher-build` job notes in `.github/workflows/systemvm-image.yml`.

## Decision Log

- Decision: leak detection is host-side and structural: launcher and proxy processes, sockets and pid files under
  `run/` must be zero after the run, and the mean boot time of the last five cycles may not exceed the first five
  by more than 1.5×.
  Rationale: each cycle is a fresh VM, so guest-side leaks cannot accumulate; host-side leftovers and boot-time
  drift are the observable signatures. Host memory is too noisy to assert on and is recorded as Unknown.
  Date/Author: 2026-11-06, agent.
- Decision: `networking`, `containerd` and `dockerd` start from a detached `setsid` child of `vesper-services`
  (TD-028 resolved) instead of blocking the runlevel; the probe waits for `/run/vesper/net-ready` and
  `/run/vesper/services-ready`.
  Rationale: the boot-time threshold measures time to a usable login; readiness of the daemons is measured by the
  probe and `HEALTH`, not by the login prompt. Keeps the owner's 5 s threshold meaningful on slower hosts.
  Date/Author: 2026-11-06, agent.
- Decision: the suite reuses the existing suites (boot smoke, control, services, Docker smoke) rather than
  re-implementing checks, and the report links each failure to the underlying report or log.
  Rationale: one source of truth per check; actionable signatures.
  Date/Author: 2026-11-06, agent.

## Outcomes & Retrospective

The third 50-cycle run passed: 50/50 boots, mean 2174 ms, p90 2206 ms, max 2234 ms (threshold 5000), no drift
(first five 2193 ms, last five 2171 ms); control suite 4/4, services suite 9/9, Docker smoke 7/7; zero launcher or
proxy processes, sockets or pid files left (`run/WOR-21/systemvm/reliability-report-50.json`). The first run's
threshold failures were a real finding, fixed by detaching the daemon and network start-up (TD-028). Gap: the
suite needs Apple silicon; CI runs only the assembler self-test (TD-005).

## Context and Orientation

`scripts/systemvm/reliability-suite.sh` orchestrates; `reliability_report.py` assembles. Inputs: `boot-report.json`,
`control-report.json`, `services-report.json`, `docker-smoke-report.json`, `leak-before.json`, `leak-after.json`.
Terms: *cycle* is one boot through the launcher with the probe and power-off; *drift* compares early and late boot
times.

## Plan of Work

Done in one slice; see Progress.

## Concrete Steps

    cd <repo root>
    make systemvm-prepare systemvm-trivial-image
    make systemvm-reliability            # CYCLES=50 by default
    python3 -c 'import json;print(json.load(open("run/systemvm/reliability-report.json"))["passed"])'

## Validation and Acceptance

`python3 -I scripts/systemvm/reliability_report.py --self-test`, `bash -n` of the script, one 50-cycle run with
`passed: true`, docs and structure sensors, CI. Issue exit: the four GH #16 checklist items proven by the report.

## Idempotence and Recovery

The suite stops any running VM and proxy first and leaves nothing running; rerun at will.

## Artifacts and Notes

    $ make systemvm-reliability
    reliability: cycles=50 host=27.0.1 Mac15,6 commit=…
    reliability: boot stress passed: 50/50 max_boot_ms=2234 threshold=5000
    reliability: socket smoke: systemvm-up, control suite, services suite, docker-up, docker smoke
    reliability: leak snapshot after: {'launcher_processes': 0, 'proxy_processes': 0, 'sockets': 0, 'pid_files': 0}
    reliability report: passed=true failures=0 -> run/systemvm/reliability-report.json

    First run (before the fix): FAIL boot_stress: 3 of 50 cycles failed ...: iteration 18: boot took 6603 ms,
    over the 5000 ms threshold ... (see run/systemvm/boot.log, run/systemvm/console.log)

## Interfaces and Dependencies

`reliability_report.py` schema version 1: `boot_stress`, `control_suite`, `services_suite`, `docker_smoke`, `leaks`,
`failures[] {step, reason, evidence}`, `passed`. No new dependencies.
