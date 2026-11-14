# GH #194: no container is root on the System VM

The owner put a container that is given the System VM's root inside ADR 0009's threat model (D-034) and accepted
[ADR 0010](../../design-docs/adr/0010-no-container-is-systemvm-root.md) after a council review (D-035). This plan
builds it. GH #160 closes only after this plan does.

## Purpose / Big Picture

After this change, `docker run` with any flags Docker accepts starts a container whose root is an unprivileged uid
on the System VM. It cannot enter a microVM's share namespace, reach a share server, read root-owned Vesper state
or reach the Docker, containerd or agent endpoints. `--privileged`, `--pid=host`, `--network=host`, `--ipc=host`
and `--userns=host` fail with a clear message. A user sees the change as those refusals, and as old images and
volumes that must be pulled or imported once.

## Progress

- [x] M0 (2026-11-14) Design: ADR 0010, two spikes on the live System VM, one llm-council design review (2 High,
  9 Medium, 4 Low, all carried into the ADR), accepted by the owner (D-035)
- [ ] M1 The authorisation plugin (`vesper-authz`, Go, standard library only): route allow-list, the create-body
  rules, refusal log; unit tests for path cleaning, version prefixes, missing and oversized bodies, key case and
  duplicate keys; the route inventory in this plan
- [ ] M2 Overlay: `dockremap` and subordinate ranges, dockerd configuration, the plugin's OpenRC service ordered
  before dockerd with an activation wait, the data disk's mode record, `HEALTH` fields (`containers`, plugin
  state, legacy store)
- [ ] M3 Private state: an inventory of what Vesper writes under `/run/vesper`, `/var/lib/vesper` and `/var/log`,
  owner-only creation including rotation, and the vsock peer check (CID 2 only) with `vsock_loopback` blocked
- [ ] M4 Callers and the unmapped boot: `test-share-reach`, `data-disk-suite.sh` on new `vesper-data` verbs, the
  `vesper.containers=unmapped` token with its own data disk, the upstream runner's saved config moved out of the
  shared directory, the launcher printing the mode
- [ ] M5 `make systemvm-confine-suite`, and the ADR 0009 step in the share-network suite
- [ ] M6 `vesper-data legacy status` and `legacy import-volume`; every Docker suite rerun; deviations recorded;
  `design-v0.md`, `docs/SECURITY.md`, the conformance notes and TD-061 updated; council review of the code

## Surprises & Discoveries

- 2026-11-14: dockerd refuses `--privileged` under remapping; it does not confine it to the user namespace. The
  owner was told the opposite when choosing the mechanism, was corrected before accepting, and accepted.
- 2026-11-14: the first spike's script ran from a container's mount namespace, so the dockerd it restarted could
  not see its own overlay mounts and every container failed with an empty root. Restart daemons from the System
  VM's own namespaces (`nsenter -t 1 -m -u -i -n -p`, with `--cgroupns=host` so the script can leave the
  container's cgroup).
- 2026-11-14: the System VM's virtio-fs mounts do not check the caller's uid, so shares work unchanged for a
  remapped container.
- 2026-11-14: the System VM has no `buildx`; its `docker build` is the legacy builder, which runs remapped.
  BuildKit from a macOS client is untested.

## Decision Log

- 2026-11-14: the plugin is default-deny over routes, and refuses a create whose body it cannot read. dockerd
  hands a plugin the body only under 1 MiB, so "refuse a visible `host`" can be bypassed with padding (council
  finding 1).
- 2026-11-14: the unmapped boot is a kernel command-line token plus a data disk of its own, not a launch-config
  field. A persisted field could be written from inside the suite's boot, and a shared disk would start old
  restart-policy containers as root (council findings 4 and 5).
- 2026-11-14: no general root shell on the agent. Each need gets a named, validated verb (council finding 8).
- 2026-11-14: the guarantee is "no container is root on the System VM", not isolation between containers. One
  mapping is shared, so a container given `/` reads other containers' volumes (council finding 7).
- 2026-11-14: rootless dockerd is the recorded fallback if the allow-list proves unmaintainable. It needs no
  plugin but brings new dependencies and a user-mode network path.

## Outcomes & Retrospective

Not yet.

## Context and Orientation

- dockerd and containerd start from `systemvm/overlay/etc/init.d/vesper-services`; packages are pinned in
  `docs/specs/systemvm-packages.lock.yaml`. macOS reaches dockerd through `vesper-proxy` and the vsock relay
  `etc/init.d/vesper-docker-relay`.
- The agent (`usr/local/bin/vesper-agent`, vsock port 1024) is one process per connection under `socat`.
- The data disk is managed by `usr/local/bin/vesper-data` (GH #164); Docker's store is on it.
- The Go code lives in `shim/`; its binary is cross-built for the System VM by the overlay build.
- Callers of a privileged container: `vesper-microvm` (`test_share_reach`), `scripts/systemvm/data-disk-suite.sh`,
  `scripts/systemvm/upstream-suite/run.sh` (lines 245 and 285).

## Plan of Work

Milestones run in the order above. M1 has no runtime effect and is reviewed on unit tests alone. M2 to M4 land
together or behind the mode record, because remapping without M4 breaks the share-network suite. M5 is the
evidence. M6 is migration and the sweep of existing suites.

## Concrete Steps

- Unit: `cd shim && go test ./...`; `cargo test`.
- Runtime, each from a rebuilt overlay: `make systemvm-down systemvm-overlay systemvm-up docker-up`, then
  `make systemvm-confine-suite` and every existing suite.
- Before each push: `python3 harness/check_docs.py`, `python3 scripts/harness/check_structure.py --root .`.

## Validation and Acceptance

GH #194's criteria, each shown by a step of the confinement suite unless noted:

- Every container is remapped: `/proc/self/uid_map` in a default container.
- Each of `--privileged`, `--pid=host`, `--network=host`, `--ipc=host` and `--userns=host` is refused.
- With `--cap-add ALL`, seccomp unconfined and `-v /:/host`: no read of a root-only file, no `setns` into a share
  namespace, no connect to a share server, no open of the disk, `/dev/kvm` or `/dev/vsock`, no connect to the
  Docker or containerd socket, no load of a named module.
- A create padded past 1 MiB, a chunked create, a create with `usernsmode` in another case and one with duplicate
  keys are all refused when they ask for the host namespace. Unit tests cover the same cases.
- Routes outside the allow-list are refused, with and without a version prefix and with a doubled slash.
- With the plugin stopped, Docker requests fail and `HEALTH` says why; it recovers when the plugin returns.
- A single-file bind of a console log, a workload record and a lease is unreadable in a container.
- An unconfined container cannot reach the agent over vsock.
- A normal boot after an upstream-suite run reports remapped mode; a mapped disk refuses an unmapped boot.
- The data-disk suite and the share-network suite pass without a privileged container.
- Plugin latency per Docker call is measured and recorded.

## Idempotence and Recovery

The mode record makes a wrong-mode boot refuse to start dockerd instead of mixing stores. The old store is left in
place; removing it needs the owner. To undo the change on a development machine, boot an older overlay: it sees
the old store and none of the writes made since.
