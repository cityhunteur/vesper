# Run and supervise containerd and dockerd in the System VM (WOR-33 / GH #14)

This ExecPlan is a living document. The sections Progress, Surprises & Discoveries, Decision Log, and
Outcomes & Retrospective must be kept up to date as work proceeds. It is maintained in accordance with
docs/PLANS.md.

## Purpose / Big Picture

GitHub #14 (Linear WOR-33, mirror #73) asks for containerd and optionally dockerd installed and supervised in the
System VM: services start on boot, a health check answers, a crashed daemon restarts without manual intervention,
and logs capture startup and recovery. After this plan `make systemvm-up` boots a guest where both daemons run
under `supervise-daemon`, the agent's `HEALTH` reports them, and `make systemvm-services-suite` proves crash
recovery from the host.

## Progress

- [x] (2026-11-06 19:00Z) `containerd-ctr` pinned (19 packages); overlay gains OpenRC `networking` (eth0 DHCP),
  `cgroups` in sysinit with `rc_cgroup_mode="unified"`, and `vesper-services` (needs `vesper-packages`) that
  starts `containerd` and `docker` from a clean environment; agent gains `HEALTH` fields, `KILL`, `LOGS`;
  `src/control.rs` gains `kill_service`, `logs`, `run_services_suite`; `systemvmctl control kill|logs|services-suite`;
  `make systemvm-services-suite`. Suite passed: both daemons healthy at boot, recovered in under 600 ms after
  SIGKILL, logs show the respawns. Boot smoke 5/5, max boot 4509 ms.
- [x] (2026-11-06 19:20Z) PR #124 merged as `f64ac7b`, CI green, GH #14 checklist and WOR-33 updated, milestone entry; plan moved to completed.

## Surprises & Discoveries

- Observation: `rc-service <svc> start` from inside another service's `start()` during the runlevel change only
  schedules the start and returns non-zero; with `env -i PATH=…` it starts the service immediately.
  Evidence: `vesper: service containerd failed to start` on the first boot; manual start worked.
- Observation: the live image does not run the `cgroups` service, so nothing is mounted under `/sys/fs/cgroup`;
  containerd tolerates it, dockerd exits with "Devices cgroup isn't mounted" and supervise-daemon respawns it
  every 2.5 s. `rc.conf` alone does nothing without the service in sysinit.
  Evidence: `systemvmctl control logs docker`; probe output `rc-service cgroups status` = stopped.
- Observation: with the daemons started before the login prompt, the boot time to `login:` rises from about 2.0 s
  to 3.4 to 4.5 s, under the 5 s threshold with little headroom on this host.
  Evidence: `run/WOR-33/systemvm/boot-report-5.json`.

## Decision Log

- Decision: start the daemons from a `vesper-services` oneshot that `need`s `vesper-packages`, not from runlevel
  symlinks.
  Rationale: their OpenRC scripts arrive with the packages during the runlevel, so runlevel symlinks would dangle.
  Date/Author: 2026-11-06, agent.
- Decision: unified cgroups (v2) in the guest.
  Rationale: Docker 28 requires the devices controller in v1 and the kernel's hybrid layout lacks it; v2 uses eBPF
  device filtering.
  Date/Author: 2026-11-06, agent.
- Decision: crash simulation is `SIGKILL` through the agent (`KILL`), recovery is judged by the agent's `HEALTH`
  (`ctr version`, `docker version`), and evidence is `/var/log/<svc>.log` plus `supervise-daemon` lines in
  `/var/log/messages`.
  Rationale: observable from the host without a shell in the guest.
  Date/Author: 2026-11-06, agent.

## Outcomes & Retrospective

See Progress; all four GH #14 checklist items are proven in `run/WOR-33/systemvm/services-report.json`.
Gaps: dockerd listens only on its Unix socket inside the guest (ingress is WOR-23); no container has been run yet.

## Context and Orientation

`systemvm/overlay/` (guest services), `src/control.rs` (host client), `systemvmctl control` (CLI). Terms: *supervise-daemon*
is OpenRC's supervisor that respawns a crashed daemon; *services suite* is the host-run GH #14 checklist.

## Plan of Work

Done in one slice; see Progress.

## Concrete Steps

    cd <repo root>
    make systemvm-prepare && make systemvm-up
    cargo run --bin systemvmctl -- control health      # containerd_ok and docker_ok true
    make systemvm-services-suite                       # 9 steps ok
    make systemvm-down

## Validation and Acceptance

`cargo test control systemvm` (fake-bridge suite test included), `make systemvm-up`, `make systemvm-services-suite`
with `passed: true`, `make systemvm-boot-smoke` 5/5 under the threshold, docs and structure sensors, CI.

## Idempotence and Recovery

All targets are safe to repeat; `make systemvm-down` always stops the VM; packages and the overlay rebuild from the
cache.

## Artifacts and Notes

    ok   containerd_recovers: healthy again after 579 ms (old pid 2232)
    ok   docker_recovers: healthy again after 559 ms (old pid 2269)
    ok   docker_logs_capture_startup_and_recovery: 2 startup lines in docker.log, 4 supervise-daemon lines for docker in messages

## Interfaces and Dependencies

`vesper::control::{Session::kill_service, Session::logs, run_services_suite}`; agent protocol 1 commands `KILL`,
`LOGS`; guest packages add `containerd-ctr`. No new crate dependencies.
