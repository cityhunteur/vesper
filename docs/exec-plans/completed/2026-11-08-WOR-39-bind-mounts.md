# Bind mounts over virtio-fs with documented ownership and watch semantics (WOR-39 / GH #29)

This ExecPlan is a living document. The sections Progress, Surprises & Discoveries, Decision Log, and
Outcomes & Retrospective must be kept up to date as work proceeds. It is maintained in accordance with
docs/PLANS.md.

## Purpose / Big Picture

GitHub #29 (Linear WOR-39, mirror #77) asks for host bind mounts with practical permission and
change-notification semantics: host edits appear promptly in workloads, permission behaviour is predictable
and documented, read-only mounts are enforced, and virtio-fs is the required path (design-v0). After this plan
a host directory named in `VESPER_SHARES` is shared into the System VM and mounted there at the same absolute
path, so `docker run -v /host/path:/x` from macOS works unchanged; `make binds-suite` records 10 checks.

## Progress

- [x] (2026-11-08) Launcher: `shared_directories[] {tag, host_path, read_only}` in the launch config become
  `VZVirtioFileSystemDeviceConfiguration` devices (tag validated, path must be a directory). Rust:
  `SharedDirectory`, `LaunchConfig.shared_directories`, `shared_directories_from_specs` (`PATH[:ro]` → tags
  `share0..`), `systemvmctl launch-config --share` (repeatable), `VESPER_SHARES` for the generator,
  `make systemvm-up-shares` for the two test shares. Guest `vesper-share mount|list|umount|probe` (loads
  `virtiofs`, mounts the tag at the host path, `ro` honoured), agent `SHARE`, `Session::share`, `systemvmctl
  control share`; `up.sh` mounts every configured share after the probe. `scripts/systemvm/binds-suite.sh` and
  `make binds-suite`: 10/10 on an M3 Pro: shares mounted at the host paths (rw and ro), host file readable in
  a container, host `501:20` appears as `0:0` in the guest, a container write appears on the host owned by the
  sharing user with `chmod` kept and `chown` ignored, read-only enforced at share level and with `:ro`, a host
  edit visible on the next read, inotify events for guest writes, no inotify events for host edits (recorded as
  a documented limitation, TD-038), and an unshared `-v` path staying guest-local (path policy). Docker
  conformance, ports and shim suites still pass with shares configured.
- [x] (2026-11-08) PR #138 merged as `f83dc1d`, CI green, GH #29 checklist items 1, 2 and 4 ticked (item 3 open under TD-038), mirror #77 closed, WOR-39 Done, milestone entry; plan moved to completed.

## Surprises & Discoveries

- Observation: Virtualization.framework's virtio-fs presents every host file as owned by the guest's uid 0 and
  writes everything on the host as the sharing user; `chmod` round-trips, `chown` is accepted and ignored. This
  is the Docker Desktop behaviour developers know, and it is now documented rather than discovered.
- Observation: inotify fires for writes made inside the guest but never for host-side edits; contents and mtime
  are current on the next access. Dev servers relying on inotify need polling (TD-038).

## Decision Log

- Decision: the guest mounts each share at the host's absolute path, chosen by the host at `systemvm-up`
  through the agent, rather than a fixed `/mnt/...` tree.
  Rationale: Docker bind mounts then need no path rewriting in the proxy; the path policy stays explicit
  (only `VESPER_SHARES` entries are host paths, everything else is guest-local).
  Date/Author: 2026-11-08, agent.
- Decision: no default share. Sharing `$HOME` is a human decision because it widens what the guest can read.
- Decision: the inotify checklist item is left open with a documented limitation instead of a workaround.
  Rationale: a polling shim or a host-side watcher pushing notifications is a design of its own.

## Outcomes & Retrospective

See Progress; `run/WOR-39/systemvm/binds-report.json` carries the 10 steps. Gaps: TD-038 (host-edit
notifications), ownership fixed to the sharing user.

## Context and Orientation

`launcher/Sources/vesper-vmlauncher/main.swift` (shares), `src/systemvm.rs` (`SharedDirectory`),
`systemvm/overlay/usr/local/bin/vesper-share`, `scripts/systemvm/up.sh`, `scripts/systemvm/binds-suite.sh`.

## Plan of Work

Done in one slice; see Progress.

## Concrete Steps

    make systemvm-up-shares && make docker-up
    make binds-suite
    make docker-down && make systemvm-down && make systemvm-launch-config

## Validation and Acceptance

`cargo test systemvm`, `make binds-suite` with `passed: true`, sibling suites still passing with shares
configured, docs and structure sensors, CI.

## Idempotence and Recovery

`vesper-share mount` is idempotent; `make systemvm-launch-config` (no `VESPER_SHARES`) restores a config without
shares; the suite resets its test files.

## Artifacts and Notes

    ok   ownership_mapped_to_root_in_guest: host 501:20 appears as 0:0 in the container
    ok   container_write_owned_by_host_user: chmod 640 kept, chown 1000:1000 ignored; host sees 501:20 640
    ok   readonly_share_enforced / docker_ro_flag_enforced: Read-only file system
    ok   inotify_host_edits_not_propagated: host edits produce no inotify events in the guest (TD-038)

## Interfaces and Dependencies

Virtualization.framework virtio-fs (macOS 12+); guest `virtiofs` module from the pinned kernel. No new
packages or crate dependencies.
