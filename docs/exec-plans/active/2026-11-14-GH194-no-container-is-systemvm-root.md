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
- [x] M1 (2026-11-14) The authorisation plugin (`shim/cmd/vesper-authz`, `shim/internal/authz`, standard library
  only): route allow-list, the create-body rules, refusal log without queries or bodies; unit tests for path
  cleaning, version prefixes, missing bodies, key case, folded letters, duplicate keys, merged objects and
  unreadable plugin messages; the route inventory below. Built by `build-shim.sh`, not installed yet
- [x] M2 (2026-11-14) Overlay: `vesper-containers` (mode from the kernel command line, the disk's record,
  `dockremap`, dockerd configuration), the `vesper-authz` service started before dockerd with a socket wait,
  `HEALTH` fields `containers`, `authz`, `legacy_store`, the `CONTAINERS` agent verb
- [x] M3 (2026-11-14) Private state from a full inventory: `umask 077` in the root tools with two reasoned
  exceptions, explicit modes for the stores, snapshot store files `root:vesperd-jail` 0440, shim files 0600, the
  daemons' logs 0600; the vsock peer check in the agent, port relay and share notifier; `vsock_loopback` blacklisted
- [x] M4 (2026-11-14) Callers and the unmapped boot: `test-share-reach`, `data-disk-suite.sh` on `vesper-data
  binds|label|test-relabel`, the upstream runner's token, own data disk and saved config outside the share,
  `docker-up.sh` comparing the launch config's mode with the System VM's
- [x] M5 (2026-11-14) `make systemvm-confine-suite`: 14 of 14 from a rebuilt overlay, after the council review's fixes
  (`run/GH-194/m5-confine-report.json`). The same overlay passes 23 other suites
  (`run/GH-194/battery/summary.txt`): sharenet, microvm, shim, workload, launchapi, vesperd, egress, bounds, exec,
  bundle, restart, rootfs, snapshot, docker-smoke, docker-conformance, ports, dns, workflow, crossarch,
  ctr-conformance, data-disk, binds. Not rerun: the upstream Firecracker suite in its new unmapped boot
- [x] M5b (2026-11-15) TD-062 fixed before merge (D-036): dockerd's default runtime is `vesper-runc`, which asks
  each bind source on a share for its owner again as the container's uid after `runc create`; the host-edit
  replay runs as container root with `CAP_FOWNER` only. `make binds-suite` 25 of 25 with no waits; plain runc and a
  root replay fail the same checks (`run/GH-194/td062/control.txt`). Confine 14 of 14 and 23 other suites pass on
  that overlay (`run/GH-194/td062/battery/summary.txt`; the snapshot suite on a rerun in its own boot config)
- [ ] M6 `legacy import-volume`; a short upstream-suite run in its unmapped boot on its own disk; deviations
  recorded in the conformance notes; council review of the code
  - [x] (2026-11-16) `vesper-data legacy status|import-volume NAME`; the confinement suite's 15th step imports a
    planted volume and checks the owners in the store and in a container (`run/GH-194/m6/`). The real old store
    lists 6 volumes; none was imported (that is the owner's choice per volume)
  - [x] (2026-11-16) TD-061's council findings: the Docker relay runs `vesper-docker-relay-conn`, which turns
    away a peer that is not the macOS host, and the Docker and port relays hold at most 256 connections (finding
    20); the upstream runner derives `vesper_data=` for its own disk (finding 19). Confine 15 of 15 (the vsock step
    now tries the port relay too, 9 refused connects), docker-smoke, docker-conformance and ports pass
  - [x] (2026-11-16) Docker deviations recorded in `design-v0.md` Phase 4 as accepted deviations (D-035); its
    BuildKit line, which still said BuildKit builds pass, now says the legacy builder passes and BuildKit is refused
  - [x] (2026-11-16) A short upstream-suite run in its unmapped boot on its own disk
    (`integration_tests/functional/test_api.py`, twice): 67 tests, 59 passed, 6 upstream platform skips and 2
    `vmm-memory-overhead` errors, every result explained; the next normal boot was mapped. The runner now trims the
    suite's disk before it restores (20 GiB to 7.1 GiB on the Mac). Evidence: `run/GH-194/m6/upstream-short-*`
  - [x] (2026-11-16) Council review of the M6 code (`2ec6281..cd7063a`; the rewritten static council: five
    reviewer roles, a Claude Opus judge, no code run). Four confirmed defects, all in `import-volume`, fixed: the
    copy is shifted in a root-only staging directory, not in the container-reachable volumes directory (a
    container could swap a path mid-shift, high); a hard-link marker that cannot be made fails the import; the
    volume is created with a one-time label and is used or removed only when it carries it; `flock` and signal
    traps replace the `mkdir` lock. The suite fixture gained the boundary ids, set-uid, outward and dangling links,
    a leading-dash name and a dead import's leftovers. Remaining coverage gaps are TD-064
  - [x] (2026-11-16) Acceptance check against GH #194: `docker exec --privileged` was the one listed flag no step
    ran. Step 3 now runs its 21 probes again through `docker exec --privileged` into the broadest container; all 21
    are refused (`run/GH-194/m6/exec-privileged-confine-report.json`, run without the two restarts)

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

- 2026-11-14 (M5): **ownership on shares flickers under remapping (TD-062).** Apple's virtio-fs reports the asking
  uid as each file's owner, and the System VM caches the answer for about a second for all uids. While container
  root was the System VM's root the two always agreed. Now a look by root (dockerd resolving a bind at container
  start, the host-edit replay) leaves uid 0 in the cache, and for that second a container sees `nobody` and gets
  `EPERM` from `chmod` and `touch`. A same-mode `chmod` by the container uid cannot refresh the cache, because the
  kernel checks the cached owner first. Measured in a one-off unmapped boot on a scratch disk.
- 2026-11-14 (M5): a container without a seccomp filter can make a vsock socket without `/dev/vsock`. Its connects
  to CIDs 1 and 2 are reset by the macOS host; to the System VM's own CID they fail with "no such device".
- 2026-11-14 (M5): `make systemvm-up` fails on a refused boot, because the launcher's probe needs dockerd. The
  console line `vesper: CONTAINERS refused …` is the evidence; `HEALTH` is not reachable from a failed `up`.
- 2026-11-14 (M5): the buildx client falls back to a privileged BuildKit container with `--userns=host` when
  `/session` is refused, so the plugin's refusal names `--userns=host`. `DOCKER_BUILDKIT=0 docker build` works.
- 2026-11-14 (M5): the egress suite's last step kills a `vesperd` holder and leaves its socket file under
  `run/egress-suite/`; `docker-smoke` then fails `no_raw_host_sockets`. An ordering artefact, older than this work.
- 2026-11-16 (M6): BusyBox `cp -a` keeps hard links, so shifting owners by path shifted a file with two names
  twice and left it owned by `nobody`. The suite's planted hard link caught it; each inode is now shifted once.
- 2026-11-16 (M6): the first import staged its copy in `/var/lib/docker/100000.100000/volumes`, which container
  root can traverse. Each directory became container-owned as soon as it was shifted, before root had finished
  below it, so a running container could have swapped a subdirectory for a symbolic link and had root re-own a
  System VM file. Our own suite could not see it; the council's red-team role did.

## Decision Log

- 2026-11-16: `import-volume` gives an owner outside 0 to 65535 the container's `nobody` (System VM uid 165534)
  rather than refusing the volume or keeping the raw id. A raw id would be a System VM uid outside the mapping;
  refusing would block a volume over one stray file. The count is printed as `unmapped=N`. It copies (BusyBox
  `cp -a`) rather than moving, so the old store stays readable by a downgrade, as ADR 0010 point 8 requires.
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

- 2026-11-14 (M1): the plugin also refuses a runtime other than the default, and volume driver options or a
  driver other than `local`, on container create and volume create. With options the `local` driver makes dockerd,
  as root, perform a mount the client describes (a block device, another filesystem type), and a runtime name
  makes dockerd start a different runtime. Neither has a test that bounds it, so neither is allowed. Compose files
  that declare `driver_opts` volumes (NFS, tmpfs) stop working; that is a recorded deviation.
- 2026-11-14 (M1): a path that `path.Clean` would change is refused, not normalised, so the plugin and dockerd's
  router can never read one request two ways.

## Route inventory

Allowed (`shim/internal/authz/policy.go`, version prefix optional): `/_ping`, `/version`, `/info`, `/events`,
`/system/df`, `/auth`; containers (list, create, prune, inspect, top, logs, changes, export, stats, attach,
archive, start, stop, restart, kill, pause, unpause, wait, resize, rename, update, exec, delete); exec (start,
resize, inspect); `/commit`; images (list, search, get, create, load, prune, inspect, history, push, tag, delete);
`/distribution/…/json`; `/build` with the legacy builder and not on the host network, `/build/prune`; networks
(list, create, prune, inspect, delete, connect, disconnect); volumes (list, create, prune, inspect, delete).

Refused until a test shows each stays inside the mapping:

| Route | Why it is not listed |
|---|---|
| `/plugins/…` | a managed plugin runs outside the mapping |
| `/swarm`, `/services`, `/nodes`, `/tasks`, `/secrets`, `/configs` | service tasks are created by the daemon, not through container create |
| `/session`, `/grpc`, `/build?version=2` | BuildKit; not measured (the System VM has no `buildx`) |
| `/build?networkmode=host` | not measured under remapping |
| `/containers/{id}/checkpoints` | experimental; runs CRIU as root |
| anything else | unknown routes are refused by default |

Still to check against the running daemon in M5: that dockerd passes `RequestUri` exactly as this table parses
it, and that a chunked create reaches the plugin without a body.

- 2026-11-14 (M3): the files are closed, not only their directories, because dockerd binds a single file as root
  whatever the parent allows. `umask 077` is set per tool, and each tool sets its own, so a caller's umask never
  leaks into a tree whose modes become a guest's (`vesper-microvm prepare`, `vesper-rootfs`).
- 2026-11-14 (M3): snapshot store files change from 0444 to `root:vesperd-jail` 0440. The jail still reads them
  through its hard link and cannot change them; other uids no longer read guest memory.

- 2026-11-14 (council review of M2 to M5, `run/GH-194/council-m2-m5.md`: 2 High, 8 Medium, 10 Low). Fixed before
  merge:
  - **High 1, a container could own a directory a root tool adopts.** dockerd creates a missing bind source for
    container root, and `/var/lib/vesper/microvm` (the kernel and initramfs every microVM boots), `/srv/jailer` and
    `/run/vesper/mem` were made lazily. Now `vesper-containers prepare` makes every state directory before
    dockerd, the tools refuse one root does not own, and the plugin refuses to create a source outside a share.
  - **High 2, old files kept their modes after an upgrade.** `prepare` sets them on every boot, and refuses the
    boot if it cannot.
  - Medium and Low: `docker-up` serves an unmapped boot only to the upstream runner; the vsock step requires six
    recognised denials and the peer check has a hermetic test and fails closed under socat; images are owner-only
    while `mke2fs` writes them; the suite disk is created 0600; the guest's own probe is the positive control for
    the share server; probes that fail for another reason than a refusal fail the step; raw requests must get the
    plugin's own 403; `up.sh` says why a refused boot failed; a refused boot cannot serve; `vesper-boundlog`
    counts from the file's size; `vesper-ctr` leaves `/tmp`; jail parents are 0711; `HEALTH` no longer walks the
    old store.
  - Not fixed here: the share-ownership flicker (finding 7, TD-062; fixed in M5b after the owner chose to fix
    it first); a peer check and a child cap
    on the Docker relay (finding 20); a suite config without a data disk (finding 19). Both are in TD-061.

- 2026-11-15 (M5b): the share-ownership fault happened on every container start, not now and then: a probe saw
  `nobody` and `EPERM` from `chmod` in 4 of 4 starts, for container root and for `-u 1000`. One
  `statx(AT_STATX_FORCE_SYNC)` with the container's ids ends it. A share's root directory refuses `chmod` from
  uid 100000 even with the right owner cached, as it did from root in GH #150, so the suite binds a directory
  inside the share.
- 2026-11-14: creating the image file before `mke2fs` (to make it owner-only at once) changed the bytes `mke2fs`
  wrote, and the rootfs suite's reproducible rebuild failed. The umask is set around `mke2fs` instead.
- 2026-11-14: the vesperd suite's `firecracker_connection_limit_passes_through` failed once in the second battery
  ("connection 11 got ''") and passed on a rerun with nothing it uses changed. Not explained.

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
