# Published ports on localhost through vesper-proxy (WOR-34 / GH #28)

This ExecPlan is a living document. The sections Progress, Surprises & Discoveries, Decision Log, and
Outcomes & Retrospective must be kept up to date as work proceeds. It is maintained in accordance with
docs/PLANS.md.

## Purpose / Big Picture

GitHub #28 (Linear WOR-34, mirror #104) asks that `-p` published ports be reachable on localhost through the
defined ownership path, with conflict detection, clean teardown and multiple mappings per container. After this
plan `make ports-suite` proves 11 checks from macOS, and ADR 0005 records the path.

## Progress

- [x] (2026-11-08) `src/ports.rs` publisher (Docker events, reconciliation at start, loopback listeners per TCP
  binding, forwarding through the bridge `CONNECT 2400` + `PORT <n>` to the guest gateway, `ports.json` status,
  conflict and unsupported states); `vesper-proxy --publish-ports --relay-port --ports-status`, enabled by
  `make docker-up`; guest `vesper-port-relay` (supervised socat on vsock 2400) with `vesper-port-relay-conn`;
  launcher ignores SIGPIPE (a dying proxy with a live event stream killed the VM); agent HEALTH reports the
  guest `ip` (diagnostic). `scripts/systemvm/ports-suite.sh` and `make ports-suite`: 11/11 on an M3 Pro: two
  ports of one container answer on 127.0.0.1, both listed `listening`, dockerd rejects a second container on a
  published host port, a host-taken port is recorded as `conflict` with an actionable message, stop closes the
  ports and clears the status, the launcher survives a proxy restart, a restarted proxy republishes a running
  container, `rm -f` closes the port. Docker conformance and smoke, shim and bundle suites still pass; 12 Rust
  unit tests for parsing, planning, HTTP framing and conflict reporting.
- [ ] PR merged, CI green, GH #28 checklist and WOR-34 updated, milestone entry; plan moved to completed.

## Surprises & Discoveries

- Observation: forwarding to the guest's NAT address worked from `curl` but failed with `No route to host` from
  Python and from the proxy; the vsock gateway path removes the dependency on host network policy (ADR 0005).
- Observation: the first suite run killed the System VM: the launcher relayed guest bytes into the socket of a
  proxy that had just been stopped and died of SIGPIPE. Fixed in the launcher and covered by a suite step.
- Observation: the launcher's bridge sends no reply to `CONNECT`; the first gateway dial waited for one.

## Decision Log

- Decision: loopback-only listeners, TCP only, one gateway process (ADR 0005).
- Decision: the publisher runs inside `vesper-proxy` rather than as a separate daemon.
  Rationale: one process owns the ingress and the forwarding path; `make docker-up`/`docker-down` keep one pid.

## Outcomes & Retrospective

See Progress; `run/WOR-34/systemvm/ports-report.json` carries the 11 steps. Gaps: TD-037.

## Context and Orientation

`src/ports.rs`, `src/bin/vesper-proxy.rs`, `systemvm/overlay/etc/init.d/vesper-port-relay`,
`systemvm/overlay/usr/local/bin/vesper-port-relay-conn`, `launcher/Sources/vesper-vmlauncher/main.swift`,
`scripts/systemvm/ports-suite.sh`.

## Plan of Work

Done in one slice; see Progress.

## Concrete Steps

    make systemvm-up && make docker-up
    make ports-suite
    cat run/vesper/ports.json
    make docker-down && make systemvm-down

## Validation and Acceptance

`cargo test ports`, `make ports-suite` with `passed: true`, `make docker-conformance` and `make docker-smoke`
still passing, docs and structure sensors, CI.

## Idempotence and Recovery

The suite removes its containers and its port holder first and last; `make docker-down` releases every
listener; `run/vesper/ports.json` and the proxy log say which port is bound, in conflict or unsupported.

## Artifacts and Notes

    ok   localhost_port_reachable: 127.0.0.1:18080 -> port-80
    ok   host_level_conflict_reported: pp3 127.0.0.1:18090 conflict cannot listen on 127.0.0.1:18090: Address already in use …
    ok   launcher_survives_proxy_restart: vesper-vmlauncher pid 52068 still running after the proxy dropped a live event stream
    ok   republished_after_proxy_restart: 127.0.0.1:18091 -> port-80 after docker-down/docker-up (reconciliation)

## Interfaces and Dependencies

No new crate dependencies (tokio, serde_json, chrono already present). Guest: `socat` (pinned).
