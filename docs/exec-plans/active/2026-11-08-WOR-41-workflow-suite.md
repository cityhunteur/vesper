# Developer workflow suite: compose, build, logs, exec, signals (WOR-41 / GH #31)

This ExecPlan is a living document. The sections Progress, Surprises & Discoveries, Decision Log, and
Outcomes & Retrospective must be kept up to date as work proceeds. It is maintained in accordance with
docs/PLANS.md.

## Purpose / Big Picture

GitHub #31 (Linear WOR-41, mirror #81) asks for realistic developer workflows automated as regression gates
with reproducible reports linked to the baseline revisions, including the `linux/amd64` execution mode. After
this plan `make workflow-suite` proves compose, build, `logs -f`, interactive `exec -it`, and signal delivery
from macOS, stamps the report with the baseline lock revisions, and records the amd64 gap until GH #39.

## Progress

- [x] (2026-11-08) `scripts/systemvm/workflow-suite.sh` with `make workflow-suite` and the build fixture
  `fixtures/wf-build/` (Dockerfile with RUN and COPY layers): compose stack up with both services running and
  the client reaching the web service; `docker build` from a host context through the ingress (BuildKit via
  the CLI, 3 layers) and the image runs; `docker logs -f` streams five lines over four seconds; `docker exec
  -it` under a pty driven by `expect` gets `/dev/pts/0` and answers a prompt loop; `kill -s HUP` and `stop`
  reach the process (trap output, exit 0) and `kill` gives 137; `--platform linux/amd64` is probed with a
  separate image and recorded as the known gap (exec format error, GH #39). The report embeds `baselinectl`
  metadata (Firecracker, containerd and Apple containerization revisions). 8/8 on an M3 Pro; DNS and Docker
  conformance suites still pass.
- [ ] PR merged, CI green, GH #31 checklist (amd64 item stays open) and WOR-41 updated, milestone entry; plan
  moved to completed.

## Surprises & Discoveries

- Observation: `docker run --platform linux/amd64 alpine:3.22` re-tags the local `alpine:3.22` reference with
  the amd64 variant, so every later alpine container failed with `exec format error`. The suite now checks the
  test image's architecture first and probes amd64 with a separate image reference.

## Decision Log

- Decision: the amd64 step records the gap instead of failing the suite (`WF_REQUIRE_AMD64=1` makes it
  required once GH #39 lands).
  Rationale: the suite is a regression gate for what is implemented; the gap is visible in the report and the
  checklist item stays unticked.
  Date/Author: 2026-11-08, agent.
- Decision: `expect` drives the interactive step rather than `script`, so the test asserts a real prompt loop.

## Outcomes & Retrospective

See Progress; `run/WOR-41/systemvm/workflow-report.json` carries the 8 steps and the baseline metadata.
Gaps: amd64 (GH #39); the suite runs only on developer hardware (TD-005).

## Context and Orientation

`scripts/systemvm/workflow-suite.sh`, `scripts/systemvm/fixtures/wf-build/`, `fixtures/dns-stack.yml`.

## Plan of Work

Done in one slice; see Progress.

## Concrete Steps

    make systemvm-up && make docker-up
    make workflow-suite
    make docker-down && make systemvm-down

## Validation and Acceptance

`make workflow-suite` with `passed: true`; DNS and conformance suites still passing; docs sensors; CI.

## Idempotence and Recovery

The suite removes its containers, image and compose project before and after; a broken native test image is
repaired by its architecture guard.

## Artifacts and Notes

    ok   docker_build_from_host_context: built sha256:d4ef7536… ; run -> built-in-system-vm payload-from-host
    ok   logs_follow_streams: 5 lines over 4s
    ok   exec_it_interactive_tty: pty inside: /dev/pts/0; prompt loop answered interactive-23
    ok   stop_signals_propagate: HUP then TERM reached the process; stop in 1s, exit 0
    ok   platform_amd64_known_gap: not available yet (GH #39): exec /bin/uname: exec format error

## Interfaces and Dependencies

Host `docker compose`, `expect`; `baselinectl` for the metadata. No crate or guest changes.
