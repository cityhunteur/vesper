# Prototype a host share path into a Firecracker microVM (GH #159, TD-049)

This ExecPlan is a living document. The sections Progress, Surprises & Discoveries, Decision Log, and
Outcomes & Retrospective must be kept up to date as work proceeds. It is maintained in accordance with
docs/PLANS.md.

Tracker: GitHub #159. Time box: ends 2026-11-13. Next-work row "Prototype: a share path into a microVM" in
[design-v0.md](../../specs/design-v0.md#next-work); decisions D-015 and D-017.

## Purpose / Big Picture

Docker bind mounts from macOS work today because Docker containers run on runc inside the System VM, where the
macOS directory is a virtio-fs mount. A Firecracker microVM has no virtio-fs device, so a container moved into a
microVM (GH #53) would lose its bind mounts. This prototype finds out, within a fixed time, whether some share path
into a microVM can match what `make binds-suite` proves for the Docker path, and records one of three outcomes:
full, partial with a named deviation, or the fallback in which a microVM per container is opt-in.

Nothing in the product changes. Prototype scripts live under `run/GH-159/` (not tracked); results and decisions are
recorded here and in the decision log.

## Progress

- [x] (2026-11-11) Tracking issue #159 and this plan.
- [x] (2026-11-11) Kernel and userspace facts in the System VM (Surprises).
- [x] (2026-11-11) Milestone 1 (prototype, discarded): NFS export of a virtio-fs share, mounted over loopback in
      the System VM. Kernel `nfsd` failed; the userspace server `unfs3` held.
- [x] (2026-11-11) Milestone 2 (analysis only): block image of a share. Not attached to a microVM; it fails the bar
      by construction.
- [x] (2026-11-11) Milestone 3 (prototype, discarded): `unfs3` over a TAP device into a jailed microVM, measured.
- [x] (2026-11-11) Milestone 4: cost estimate for the vsock file service.
- [x] (2026-11-11) Milestone 5: outcome recorded (partial); decision D-020; TD-049 and design-v0 Next work updated;
      follow-up GH #160.

## Surprises & Discoveries

- Observation: the System VM kernel (Alpine 3.22 `virt`, 6.12.115) builds every candidate's kernel side as a
  module: `nfsd`, `nfs`, `nfsv4`, `sunrpc`, `fuse`, `cuse`, `tun`, `virtio_net`, `vhost_vsock`, `virtio_pmem`. The
  microVM boots the same kernel, so the client side is available too once its modules are in the initramfs.
  Evidence: `boot/config-6.12.115-0-virt` from the pinned ISO; `modinfo` in the running System VM.
- Observation: no userspace for any candidate is installed. `exportfs`, `rpc.nfsd`, `rpc.mountd`, `fusermount`,
  `unfsd` are absent, and the System VM has no package index (the ISO is offline). Every server candidate needs a
  package outside the lock or new code. Adopting a package is a new runtime dependency (AGENTS.md: ask first).
  Evidence: `command -v` and `apk search` in the System VM, 2026-11-11.
- Observation: `CONFIG_NET_9P_FD=m` lets 9p run over any file descriptor, including a vsock socket, without a
  virtio-9p device. The design excludes 9p as a GA transport for shares, so it is not tried; it is recorded here in
  case that exclusion is reopened. Evidence: kernel config.
- Observation: the System VM has 1,973 MiB of memory and a tmpfs root with about 670 MiB free. A block-image
  candidate holds a copy of every share it serves in that tmpfs.
- Observation: the kernel `nfsd` can export a virtio-fs mount, but every directory created after the mount returns
  "Stale file handle" to the NFS client: 0 of 50 create-and-delete cycles succeeded, against 50 of 50 on virtio-fs
  directly. Handles also go stale after the System VM drops its inode cache. The virtio-fs file handles that `nfsd`
  hands out do not survive a fresh lookup, so the kernel server is unusable over virtio-fs.
  Evidence: `run/GH-159/m1-estale.txt`, `m1-estale-cycles.txt`.
- Observation: the userspace server `unfs3` (Alpine main, NFSv3) passes 50 of 50 cycles, before and after inode-cache
  eviction. Evidence: `run/GH-159/m1-estale-cycles.txt`.
- Observation: with NFS defaults, a file created on macOS was not visible in the microVM within 12 s, because the
  client caches negative lookups and directory attributes. With `actimeo=1,lookupcache=positive` it appears in
  25 ms. Evidence: `run/GH-159/m3-result.json` against `m3-result-actimeo1.json`.
- Observation: in the first microVM run one line written with `>` to a file that later lines appended to with `>>`
  was missing. A dedicated test (200 appends, 100 truncate-then-append pairs, 100 write-then-read files) found no
  failure under either mount option set, so the first observation is unexplained, not reproduced.
  Evidence: `run/GH-159/m3-integrity.txt`.
- Observation: `unfs3` listened on all System VM addresses, and a Docker container on the default bridge could
  connect to it (`nc -z 172.17.0.1 2049`). With NFSv3 `AUTH_SYS` and `no_root_squash` that would hand every
  container read-write access to every share. `unfs3 -l <addr>` binds one address. Evidence: `m1-ro-and-listen.txt`.
- Observation: a read-only share stays read-only through NFS (`Read-only file system`), because the virtio-fs mount
  under the export is read-only. Evidence: `m1-ro-and-listen.txt`.
- Observation: a metadata-only `touch` run inside the microVM on the NFS mount produces an inotify event there, so
  the host-edit replay of GH #150 can be extended by one hop into each microVM. Evidence: `m3-result-actimeo1.json`
  (`replay_touch_events_in_microvm`).

## Decision Log

- Decision: outcome partial; the follow-up is GH #160 and `unfs3` waits for the owner's confirmation.
  Rationale: see Outcomes. Date/Author: 2026-11-11, maintainer agent (D-020).
- Decision: the block image was judged by analysis and not attached to a microVM.
  Rationale: its failure on write-back and visibility follows from what it is; attaching it would only confirm
  that. Date/Author: 2026-11-11, maintainer agent.
- Decision: Milestone 1 tests the NFS path inside the System VM over loopback before any microVM network exists.
  Rationale: whether the kernel `nfsd` can export a virtio-fs mount, and how coherent the result is, decides the
  candidate. The microVM network is needed for GH #53 anyway (TD-032) and is a known quantity.
  Date/Author: 2026-11-11, maintainer agent.
- Decision: userspace needed only for an experiment (for example `nfs-utils`) is fetched onto the running System VM
  and discarded on reboot. It is not added to the package lock.
  Rationale: the lock and image are the product; a throwaway install measures without adopting a dependency.
  Date/Author: 2026-11-11, maintainer agent.

## Outcomes & Retrospective

Outcome: **partial** (decision D-020). A host share reaches a microVM through NFSv3 from the userspace server
`unfs3` in the System VM, over a TAP device, with these deviations from the Docker path:

| Behaviour (binds-suite) | Docker path (virtio-fs in the System VM) | microVM over unfs3 (`actimeo=1,lookupcache=positive`) |
|---|---|---|
| Read a macOS file | yes | yes |
| Write reaches macOS, owned by the sharing user | yes (501:20) | yes (501:20); `chmod` round-trips |
| macOS create seen by the next open | immediate | 25 ms round trip |
| macOS append seen by the next read | immediate | 49 ms round trip |
| macOS append seen by `stat` alone | immediate | 0.7 s (attribute cache, up to 1 s) |
| Read-only share enforced | yes | yes |
| macOS edits as inotify events | `IN_MODIFY` (GH #150) | none until the replay is extended one hop into the microVM (shown feasible, not built) |
| 64 MiB write with fsync | 69 ms | 229 ms |
| 64 MiB read, cold | 19 ms | 209 ms |
| Create 500 small files | 180 ms | 580 ms |
| `stat` 500 files | 120 ms | 89 ms |

The Docker-path numbers were taken in a System VM shell on the virtio-fs mount, not in a container; the microVM
numbers inside a 1 vCPU, 256 MiB microVM. One developer machine (M3 Pro), single runs.

What it needs before adoption (GH #160): `unfs3` as a new runtime dependency (owner's confirmation, AGENTS.md); a
network device per microVM (TD-032, already a prerequisite of GH #53); `unfs3` bound to the microVM-facing
addresses only, with one export per microVM address, so containers on the Docker bridge cannot reach it; the NFS
client modules in the microVM initramfs; the host-edit replay extended into the microVM.

Not taken:

- Kernel `nfsd`: stale handles over virtio-fs (Surprises).
- Block image (analysis only): an ext4 image of the share built with `mke2fs -d` takes 50 ms for 2,000 small files,
  but it is a snapshot. Container writes do not reach macOS and macOS edits do not reach the container, so it fails
  the write-back, visibility and event behaviours by construction. It was not attached to a microVM. Possible use:
  read-only inputs that never change while the container runs.
- vsock file service (estimate): a FUSE client in the microVM and a server in the System VM over Firecracker's
  vsock. It needs no network device and no listener that containers can reach, and the server could invalidate the
  client's caches on every macOS edit. Without a FUSE library it is about 3,000 to 5,000 lines of Go (the FUSE
  protocol, about 20 operations, the server and the vsock transport) plus tests, two to three weeks; with a library
  it is a new build dependency. It is the candidate to revisit if the NFS deviations prove too large in use.
- 9p over vsock (`trans=fd`): possible in the kernel but excluded by the design.

Lessons: test the server's file-handle stability first, because it ruled out the obvious server in minutes. Default
NFS caching is wrong for development trees; the options decide whether the candidate looks broken or usable.

## Context and Orientation

- The share path today: [systemvm-bringup.md](../../process/systemvm-bringup.md) (virtio-fs from macOS, mounted at
  the same absolute path by `systemvm/overlay/usr/local/bin/vesper-share`), [SECURITY.md](../../SECURITY.md).
- MicroVMs: `systemvm/overlay/usr/local/bin/vesper-microvm` launches stock Firecracker under the jailer with one
  root drive and an optional vsock device, from a config file, with 1 vCPU and 256 MiB (TD-048) and no network
  device (TD-032). The initramfs is built by `prepare` from the System VM's own kernel modules.
- The bar: `scripts/systemvm/binds-suite.sh` (20 steps) for the Docker path.
- Trackers: TD-049 (this gap), TD-032 (microVM network), TD-033 (kernel), TD-025 (tmpfs storage), GH #53.

## Plan of Work

Milestone 1: on the running System VM, fetch `nfs-utils` and its dependencies for Alpine 3.22 aarch64 from the
Alpine mirror on macOS, copy them in, install them, export the watched share with the kernel `nfsd` (NFSv4,
`fsid=` set explicitly), and mount it on a second path in the System VM. Run the binds-suite behaviours against the
mount: read, write-back to macOS, a macOS edit seen on the next read and within what time, ownership and chmod,
`ro` export, inotify on the mount for macOS and local edits, and throughput and small-file latency against the
virtio-fs mount.

Milestone 2: build an ext4 image of the share with `mke2fs -d`, attach it to a microVM as `/dev/vdb` through a
scratch copy of `vesper-microvm`, and record the same behaviours (most are expected to fail: it is a snapshot).

Milestone 3 (only if Milestone 1 holds): a TAP device in the System VM, `virtio_net` and the NFS client modules in
a scratch initramfs, a static address, and the same mount inside a microVM; the same measurements.

Milestone 4: estimate the vsock file service (FUSE client in the microVM, server in the System VM) in code size,
new code versus new dependencies, and what it would give that NFS does not.

Milestone 5: compare against the bar, record the outcome, update TD-049 and design-v0 Next work, and open the
follow-up issues.

## Concrete Steps

Recorded per milestone under `run/GH-159/` with the commands and transcripts.

## Validation and Acceptance

The prototype is accepted when the outcome is recorded with evidence for each candidate tried and an estimate for
each candidate not tried, and the follow-up is tracked. No product sensor changes; `python3 harness/check_docs.py`
passes for the documents this plan touches.

## Idempotence and Recovery

Every change the prototype made to the System VM (installed packages, the NFS server, the TAP device, mounts) lives
in its tmpfs and is gone after `make systemvm-down`. The scripts under `run/GH-159/` can be rerun on a fresh System
VM: each microVM run uses a fresh jail directory, and the integrity test writes to a fresh directory in the share.

## Artifacts and Notes

Under `run/GH-159/` (local, not tracked): `m1-*` for the loopback NFS measurements and the stale-handle evidence,
`m2-block-image.txt`, `m3-result*.json` and `m3-integrity.txt` for the microVM runs, the launcher
`launch-nfs.sh`, the in-microVM scripts `m3.sh` and `m3b.sh`, the drivers `m1-measure.py` and `m3-drive.py`, and
`apk/` with the package indexes and the throwaway packages.

## Interfaces and Dependencies

None changed. The candidate adopted later (GH #160) needs `unfs3` (Alpine main) in the System VM package lock, the
NFS client modules (`sunrpc`, `grace`, `lockd`, `netfs`, `nfs`, `nfsv3`) and `virtio_net` with its `failover`
modules in the microVM initramfs, and a network device per microVM.
