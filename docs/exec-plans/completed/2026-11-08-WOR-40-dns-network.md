# DNS and network defaults aligned with Docker Desktop expectations (WOR-40 / GH #30)

This ExecPlan is a living document. The sections Progress, Surprises & Discoveries, Decision Log, and
Outcomes & Retrospective must be kept up to date as work proceeds. It is maintained in accordance with
docs/PLANS.md.

## Purpose / Big Picture

GitHub #30 (Linear WOR-40, mirror #80) asks for Docker Desktop-like DNS and network defaults: a resolver that
returns correct records, service discovery and aliases between compose containers, external queries forwarded,
defaults documented with caveats, and deterministic behaviour across runs. After this plan `make dns-suite`
proves 12 checks from macOS with a compose stack through the ingress, and the defaults are written down.

## Progress

- [x] (2026-11-08) `scripts/systemvm/fixtures/dns-stack.yml` (web with aliases `api` and `backend.internal`,
  client, one compose network) and `scripts/systemvm/dns-suite.sh` with `make dns-suite`: default-bridge
  containers inherit the System VM's resolver (`nameserver 192.168.64.1`, the NAT gateway) and resolve and
  reach `registry-1.docker.io:443`; compose containers use dockerd's embedded DNS (`127.0.0.11`), `web`
  resolves and answers over HTTP, both aliases resolve to the same address, external names resolve through the
  embedded DNS, an unknown name is a clean NXDOMAIN, a second `compose up` yields the same resolver, address
  and subnet (`172.18.0.0/16`), and `compose down` leaves no network. 12/12 on an M3 Pro; Docker conformance
  still passes.
- [x] (2026-11-08) PR #139 merged as `7d35acc`, CI green, GH #30 checklist ticked, mirror #80 closed, WOR-40 Done, milestone entry; plan moved to completed.

## Surprises & Discoveries

- Observation: busybox `nslookup` prints the server address last, so "take the last Address line" picked the
  resolver instead of the answer; `getent hosts` is the stable way to read a resolution in the test image.
- Observation: a `: ` inside a compose `command:` string is YAML mapping syntax; the fixture uses the list form.

## Decision Log

- Decision: no Vesper-specific resolver. dockerd's defaults (host resolver for the default bridge, embedded DNS
  for user networks) already match Docker Desktop's behaviour; the slice documents and verifies them.
  Rationale: the deterministic policy point is the fixed NAT network mode of the System VM in the image lock
  and dockerd's unchanged defaults, both pinned; adding a resolver would add a moving part without a gap to fill.
  Date/Author: 2026-11-08, agent.

## Outcomes & Retrospective

See Progress; `run/WOR-40/systemvm/dns-report.json` carries the 12 steps. Caveats recorded in the process doc:
IPv6 answers appear when the host resolver returns them (containers have no IPv6 route); `host.docker.internal`
is not provided (TD-039).

## Context and Orientation

`scripts/systemvm/dns-suite.sh`, `scripts/systemvm/fixtures/dns-stack.yml`, the Docker ingress section of
`docs/process/systemvm-bringup.md`.

## Plan of Work

Done in one slice; see Progress.

## Concrete Steps

    make systemvm-up && make docker-up
    make dns-suite
    make docker-down && make systemvm-down

## Validation and Acceptance

`make dns-suite` with `passed: true`; `make docker-conformance` still passing; docs sensors; CI.

## Idempotence and Recovery

The suite runs `compose down` before and after; project `vesper-dns` is the only state it creates.

## Artifacts and Notes

    ok   default_bridge_resolver: /etc/resolv.conf: nameserver 192.168.64.1
    ok   compose_embedded_resolver: client resolv.conf: nameserver 127.0.0.11; network 172.18.0.0/16
    ok   alias_api_resolves / alias_backend_internal_resolves: -> 172.18.0.2 (same as web)
    ok   deterministic_across_runs: same resolver, web ip and subnet on the second compose up

## Interfaces and Dependencies

Host `docker compose` (v2 plugin; v5.6.0 used); no crate or guest changes.
