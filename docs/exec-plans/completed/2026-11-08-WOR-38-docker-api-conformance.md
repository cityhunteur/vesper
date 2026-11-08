# Docker Engine API conformance through vesper-proxy (WOR-38 / GH #27)

This ExecPlan is a living document. The sections Progress, Surprises & Discoveries, Decision Log, and
Outcomes & Retrospective must be kept up to date as work proceeds. It is maintained in accordance with
docs/PLANS.md.

## Purpose / Big Picture

GitHub #27 (Linear WOR-38, mirror #78) asks that Docker CLI and API behaviour through `vesper-proxy`, the
canonical host ingress, be consistent and verified, that auth and context handling pass through correctly,
that unsupported operations fail with clear errors, and that the host cannot bypass the proxy to reach raw
runtime sockets. After this plan `make docker-conformance` proves 22 checks from macOS against the System VM's
Docker daemon.

## Progress

- [x] (2026-11-08) `scripts/systemvm/docker-conformance.sh` and `make docker-conformance`: raw `GET /_ping`
  and `GET /version` through the socket; a named Docker context pointing at the ingress; image import, list,
  inspect (`linux/arm64`); attached run with exit code 7 and separated stdout/stderr; detached run, `ps`,
  non-TTY `exec` with exit code, `stop` with SIGTERM caught in the container (exit 0, logs), `kill` (137),
  `rm`; `pull alpine:3.22` through the System VM's NAT in 4 s (the CLI's anonymous registry auth header
  passes through); `docker login` against an unresolvable registry and `checkpoint create` and an unknown
  `--runtime` return dockerd's own clear errors intact; ingress boundary: socket mode 600, no TCP Docker
  listener on localhost, only the ingress and the owner-only bridge exist under `run/`, guest ports without a
  listener and a would-be containerd relay port are refused. 22/22 on an M3 Pro; `make docker-smoke` passes.
- [x] (2026-11-08) PR #136 merged as `5914811`, CI green, GH #27 checklist ticked, mirror #78 closed, WOR-38 Done, milestone entry; plan moved to completed.

## Surprises & Discoveries

- Observation: the host-built trivial image (`build/systemvm/trivial-rootfs.tar`) links only the `sh` applet,
  so `sleep` must be called as `/bin/busybox sleep`; the guest-built trivial image links every applet.
- Observation: `docker run` prints its usage hint as the last line, so error checks must grep the daemon's
  message rather than take the last line.

## Decision Log

- Decision: no L7 interception in `vesper-proxy` for this slice; dockerd's errors are passed through intact
  and the proxy stays a byte-level forwarder.
  Rationale: the checklist's "unsupported commands" are met by dockerd's own messages; Vesper-specific gaps
  (published ports not reachable from macOS, bind mounts resolving inside the guest) are the subjects of #28
  and #29, and an HTTP-aware guard would have to frame keep-alive requests and hijacked streams correctly to be
  reliable (TD-036).
  Date/Author: 2026-11-08, agent.
- Decision: the registry pull is a real network step, not skipped when offline.
  Rationale: the checklist names `pull`; a failure is honest evidence that the System VM has no egress.

## Outcomes & Retrospective

See Progress; `run/WOR-38/systemvm/docker-conformance-report.json` carries the 22 steps. Gap: TD-036.

## Context and Orientation

`scripts/systemvm/docker-conformance.sh` (host, bash), `src/proxy.rs` (unchanged), the Docker ingress
section of `docs/process/systemvm-bringup.md`.

## Plan of Work

Done in one slice; see Progress.

## Concrete Steps

    make systemvm-up && make docker-up
    make docker-conformance
    make docker-down && make systemvm-down

## Validation and Acceptance

`make docker-conformance` with `passed: true`; `make docker-smoke` still passing; docs sensors; CI.

## Idempotence and Recovery

The script removes its containers (`vc1`..`vc3`), its image and its context before and after; a failed run
leaves at most those, which the next run clears.

## Artifacts and Notes

    ok   run_exit_code_and_streams: rc=7 stdout=to-stdout stderr=to-stderr
    ok   stop_sigterm_propagates: stop in 1s, exit 0, logs: started got-term
    ok   pull_from_registry: alpine:3.22 in 4s
    ok   unsupported_runtime_error: rc=125: docker: Error response from daemon: unknown or invalid runtime name: nosuch-runtime
    ok   no_containerd_relay: no vsock relay to containerd (port 10010 refused)

## Interfaces and Dependencies

Host `docker` CLI, `curl`, `nc`; no code changes to the crate.
