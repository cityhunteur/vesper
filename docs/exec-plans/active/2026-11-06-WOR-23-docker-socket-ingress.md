# Expose the Docker-compatible socket through vesper-proxy (WOR-23 / GH #15)

This ExecPlan is a living document. The sections Progress, Surprises & Discoveries, Decision Log, and
Outcomes & Retrospective must be kept up to date as work proceeds. It is maintained in accordance with
docs/PLANS.md.

## Purpose / Big Picture

GitHub #15 (Linear WOR-23, mirror #65) asks for a canonical host socket served by `vesper-proxy` that reaches the
System VM's Docker daemon over vsock, with raw containerd and Firecracker sockets private to the guest, filesystem
permissions set up at startup, and documented denial of raw access. After this plan `make systemvm-up && make
docker-up` gives `DOCKER_HOST=unix://run/vesper/docker.sock`, `docker version` and `docker info` answer from the
guest's Docker 28, and `make docker-smoke` runs a trivial container built offline from the pinned ISO, which is also
the Phase 1 exit criterion of design-v0.

## Progress

- [x] (2026-11-06 19:05Z) Guest `vesper-docker-relay` (supervised `socat VSOCK-LISTEN:2375,fork
  UNIX-CONNECT:/var/run/docker.sock`); `vesper-proxy` gains `--listen-mode` (default 0600, applied after bind),
  `--validate-upstream` (unix path or vsock bridge must exist at startup) and SIGTERM handling;
  `scripts/systemvm/docker-up.sh` / `docker-down.sh` / `build-trivial-image.sh` / `docker-smoke.sh`; Make targets.
  Smoke 7/7 through the real path: socket 0600, `docker version` (server 28.3.3 linux/arm64), `docker info`
  (overlay2, cgroup 2), 1.6 MB `docker import` through the proxy, `docker run … echo vesper-container-ok`,
  no other host sockets, guest port without a listener refused.
- [ ] PR merged, CI green, GH #15 checklist and WOR-23 updated, milestone entry; plan moved to completed.

## Surprises & Discoveries

- Observation: the macOS Docker CLI (29.x) negotiates API 1.51 with the guest's 28.3.3 server without any flag.
  Evidence: `docker version` output in `run/WOR-23/systemvm/docker-smoke.txt`.
- Observation: a busybox + musl rootfs extracted from the pinned ISO's `apks/` is enough for `docker import` and
  `docker run`, so the trivial container needs no registry or network.
  Evidence: `build/systemvm/trivial-rootfs.tar` (1 679 360 bytes), `trivial_container_runs` step.

## Decision Log

- Decision: the canonical dev socket is `run/vesper/docker.sock` (owner-only); `/var/run/vesper/docker.sock` from
  the issue stays the packaged default, Intended, because it needs root and packaging does not exist yet.
  Rationale: `run/` is the repository's runtime directory and needs no privileges.
  Date/Author: 2026-11-06, agent.
- Decision: raw-socket denial is proven by enumerating host sockets under `run/` and by a refused `CONNECT` to a
  guest port with no listener, and documented in `docs/SECURITY.md`.
  Rationale: containerd and dockerd listen only on guest Unix sockets; the only vsock listeners are the agent and
  the Docker relay.
  Date/Author: 2026-11-06, agent.
- Decision: the proxy sets its listen mode itself (TD-017 half resolved) and validates the upstream socket at
  startup instead of failing per connection.
  Rationale: the issue's "ACL/path setup with startup validation" deliverable.
  Date/Author: 2026-11-06, agent.

## Outcomes & Retrospective

See Progress; `run/WOR-23/systemvm/docker-smoke-report.json` carries the seven steps. Gaps: packaged socket path
and launchd supervision do not exist; the bridge socket exposure (TD-027) is unchanged; relay size and idle limits
remain (TD-021).

## Context and Orientation

`src/proxy.rs` and `src/bin/vesper-proxy.rs` (host ingress), `systemvm/overlay/etc/init.d/vesper-docker-relay`
(guest relay), `scripts/systemvm/docker-*.sh` and `build-trivial-image.sh`. Terms: *ingress* is the host Unix
socket Docker clients use; *relay* is the guest socat forwarding vsock 2375 to `/var/run/docker.sock`.

## Plan of Work

Done in one slice; see Progress.

## Concrete Steps

    cd <repo root>
    make systemvm-prepare && make systemvm-up && make docker-up
    export DOCKER_HOST=unix://$PWD/run/vesper/docker.sock
    docker version && docker info
    make systemvm-trivial-image && make docker-smoke
    make docker-down && make systemvm-down

## Validation and Acceptance

`cargo test proxy` (listen mode, upstream validation), `make docker-smoke` with `passed: true`, docs and structure
sensors, CI. Issue exit: the four GH #15 checklist items proven by the smoke report.

## Idempotence and Recovery

`docker-up.sh` is idempotent (pid check); `docker-down.sh` always removes the pid file and socket; the trivial image
is rebuilt deterministically from the ISO.

## Artifacts and Notes

    ok   docker_version: server 28.3.3 linux/arm64
    ok   docker_info: 28.3.3 containerd=fcd43222d6b0… driver=overlay2 cgroup=2
    ok   docker_import: sha256:e774c623… (1679360 bytes through the proxy)
    ok   trivial_container_runs: vesper-container-ok
    ok   no_raw_host_sockets: host sockets under run/: run/systemvm/vsock/3.sock run/vesper/docker.sock
    ok   raw_guest_port_denied: guest port 2376 (no listener) refused

## Interfaces and Dependencies

`vesper::proxy::ServerConfig { listen_mode, validate_upstream }` (with `Default`), `upstream_socket_path`;
`vesper-proxy --listen-mode --validate-upstream`; guest relay on vsock 2375. No new crate dependencies.
