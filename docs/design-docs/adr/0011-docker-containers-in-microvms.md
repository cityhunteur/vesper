---
status: accepted
owner: cityhunteur
last_reviewed: 2026-11-22
---

# 0011. Docker containers in microVMs: an opt-in `vesper` runtime that takes over dockerd's network namespace

Date: 2026-11-22. Supersedes: none. Superseded by: none. Amends [ADR 0009](0009-shares-into-microvms-over-nfs.md)
(where Firecracker runs and how it reaches the share server) and [ADR 0010](0010-no-container-is-systemvm-root.md)
(the runtime allow-list). Follows the B1 spike (`run/B1/findings.md`, a working paper) and the owner's decisions of
2026-11-22 (D-066). Revised the same day after EGM (design, codebase, readiness, scope), a council (security,
reliability) and a pre-review of the threat table by a reviewer of another model family
(`run/egm/ADR-0011-review.md`); a judge confirmed the revision, and it is accepted under the owner's decision of
2026-11-22 (D-066, D-068).

## Context and problem

design-v0 requires that each Docker container runs in its own Firecracker microVM (hard requirement, line 122) and
names it "Next work" step 2 (GH #53). Today `docker run` from macOS gives a runc container in the System VM:
dockerd's default runtime is `vesper-runc` (`systemvm/overlay/usr/local/bin/vesper-containers:121`), and the
authorisation plugin refuses every other runtime (`shim/internal/authz/policy.go:179-180`). Only `ctr --runtime
io.containerd.vesper.v2` reaches the shim. The owner chose an opt-in runtime first, with runc staying the default
(the D-015 fallback as an interim step, D-066); GH #53 stays open until microVMs are the default.

The shim was built for `ctr`. The spike captured what dockerd sends (`run/B1/spec.txt`) and found six gaps:

1. **The network.** The spec's `network` namespace entry has no path and there are no hooks: runc makes the
   namespace, then dockerd moves the veth into the namespace of the pid that `Create` reported and binds it at
   `/var/run/docker/netns/<id>`. The shim reports its own pid (`shim/internal/task/service.go:167-169`), in the
   System VM's root namespace: **driven by dockerd as it is, the shim would put every container's veth in the
   System VM's root namespace.** Only the plugin's runtime refusal prevents that today.
2. **DNS.** On a user-defined network dockerd's resolver listens in the container's namespace on `127.0.0.11`
   (DNAT on `OUTPUT` to a random port), which a guest kernel cannot reach.
3. **The root filesystem.** dockerd mounts the rootfs itself and passes `root.path`
   (`/var/lib/docker/100000.100000/overlay2/<id>/merged`); the shim reads only `<bundle>/rootfs` and `r.Rootfs`
   (`service.go:102-107`), so `bundle.Validate` fails (`shim/internal/bundle/bundle.go:102-106`), and with no
   mounts there is no cache key (`bundle.go:117-120`).
4. **Mounts.** The shim turns only share binds into NFS mounts (`service.go:112-115`) and gives the guest no
   mounts (`shim/internal/bundle/guest.go:63`). dockerd adds `resolv.conf`, `hosts` and `hostname` binds from
   `/var/lib/docker/100000.100000/containers/<id>/`, `/dev/shm` with a size, and volume binds.
5. **Ownership.** The rootfs is owned by 100000 and up; `mke2fs -d` copies owners as they are
   (`shim/internal/backend/backend.go:165`).
6. **Memory and disk.** Each task copies its image into the bundle under `/run` (`backend.go:115`) and again into
   its jail on tmpfs (`vesper-microvm:750`), booking the second copy (`vesper-microvm:686-700`); a 2 GiB System VM
   holds very few containers of ordinary image size.

## Decision

We will register the shim as dockerd's runtime `vesper` and make it take over the network namespace dockerd
prepares, so that `docker run --runtime vesper` runs the container in a jailed Firecracker microVM that owns the
container's address. The trust model is unchanged: whoever holds the Docker socket has dockerd's authority over
every container (the socket is owner-only on the Mac); the boundaries this ADR defends are the guest kernel against
the System VM, and runc containers against `vesper` containers.

### 1. Selection and the plugin

- `daemon.json` (mapped boots only; an unmapped boot writes none, `vesper-containers:105-109`) gains
  `"runtimes": {"vesper": {"runtimeType": "io.containerd.vesper.v2"}}`; `default-runtime` stays `vesper-runc`.
- The plugin allows `HostConfig.Runtime` equal to `vesper`, exactly, beside `vesper-runc` and `runc`.
- For a create with runtime `vesper` it refuses: `Privileged`; `Devices` and `DeviceRequests`; `NetworkMode` `host`
  or `container:*`; `PidMode`, `IpcMode` or `UTSMode` other than empty or `private`; `ReadonlyRootfs`; `Binds` or
  `Mounts` whose source is not inside a shared macOS directory (named volumes included); `VolumesFrom`; more than
  one entry in `NetworkingConfig.EndpointsConfig`; and an unreadable body. Each reason names the option and
  "(ADR 0011)".
- Until slice 3 the plugin also refuses, for runtime `vesper`, a `NetworkMode` other than empty, `default`,
  `bridge` or `none`, and until slice 4 every bind and mount whose source is a shared macOS directory; the shim
  refuses the same at `Start` (a namespace with a second interface, a share bind), so a link the slice does not yet
  make is never half made.
- For any create whose `NetworkMode`, `PidMode` or `IpcMode` is `container:<x>`, the plugin resolves `<x>` as
  dockerd does (full id, then exact name, then unique id prefix) against dockerd's store
  (`/var/lib/docker/<remap>/containers/*/config.v2.json` and `hostconfig.json`) and refuses when `<x>` is a
  `vesper` container, is ambiguous, or cannot be read. A rename that races the lookup is a recorded residual.
- Later (slice 5) the same resolver refuses, for `vesper` containers, the routes whose result would be silently
  wrong because dockerd's copy of the rootfs never sees the guest's writes: `…/archive` (`docker cp`),
  `…/export`, `…/changes` (`docker diff`), `POST /commit` (container in the query), and
  `POST /networks/<n>/connect` and `disconnect` (container in the body). Until then they are listed deviations.

### 2. The network namespace and the holder

- At `Create` the shim writes a record, then starts a holder: a re-exec of the shim binary in its `hold` mode,
  started in new network, pid, ipc, uts and cgroup namespaces (not a mount namespace, which would copy and pin every
  other task's namespace bind), with `PR_SET_PDEATHSIG` set to SIGKILL, that only waits. The shim binds the holder's network namespace at `/run/vesper/microvm/ctrns/<id>` (a new directory;
  `/run/vesper/microvm/netns/` stays ADR 0009's share namespaces) and runs `vesper-microvm ctrns-up <id>`, which
  sets IPv6 off, `ip_forward` 0, strict reverse-path filtering, no redirects, `lo` up, and `iptables` `INPUT` and
  `FORWARD` policy `DROP` with `lo` accepted. Only then does `Create` report the holder's pid.
- dockerd wires its veth into that namespace. A runc container cannot change it: it is not in it, and a process
  that joined it would hold no capability over it, since its owner is the System VM's initial user namespace. A
  `container:` join to a `vesper` container is refused (section 1); a join that races the plugin lands in the
  holder's own pid and ipc namespaces, not the System VM's.
- The holder's pid is the task's for its whole life, also after `Start` (reported by `Create`, `Connect`, `Start`
  and `State`): containerd hands dockerd the pid through `Connect`, and a join that races the plugin's check (a rename
  between create and start) lands in the holder's empty pid and ipc namespaces, not in Firecracker's, which are the
  System VM's. A network join that wins that race is in the container's namespace and can send from `eth0`, past
  the tap's allow-list (a recorded residual, TD-086). If the holder exits, the shim stops the task, so its pid is
  never reused for a join. `docker stats` reads Firecracker's cgroup, through the pid the shim keeps for it.
- The kernel sends `PR_SET_PDEATHSIG` when the forking thread exits, not the process, so the holder is forked from a
  goroutine locked to its thread for the shim's life (`runtime.LockOSThread`). dockerd uses the pid only to reach
  `/proc/<pid>/ns/net`, so the holder's own pid namespace is invisible to it; the jailer is given the bound file.
- `ctrns-links` moves the veth peer by the holder's pid; if the holder is gone it fails with that reason.

### 3. Links, set up at `Start`

`Start` reads the namespace. With no interface but `lo` (a `ctr` task, or `--network none`) there is no data or DNS
link, as today. With exactly one other interface, `eth0`, with one IPv4 address and no global IPv6 address, the
shim reads its address, prefix, MAC, MTU and routes and sets up the links below. Anything else (two interfaces, an
IPv6 address) fails the start with a message naming it.

| Link | In the container's namespace | Guest | Carries |
| --- | --- | --- | --- |
| data | tap `vd0`, mirred both ways with `eth0` (ingress `matchall`, redirect); an allow-list on `vd0` ingress | `eth0` with `eth0`'s address, prefix, MAC, MTU and routes | what a runc container's `eth0` carries |
| DNS (only when dockerd's `resolv.conf` names `127.0.0.11`) | tap `vn0` with `169.254.78.1/30`; `INPUT` accepts DNS to that address on `vn0` (the conntrack rule below); DNS to it is passed to dockerd's resolver (below) | `eth1` with `169.254.78.2/30`; `127.0.0.11/32` on the guest's `lo` and a forwarder in the guest from there to `169.254.78.1:53`, so the container's `resolv.conf` is dockerd's, unchanged | DNS only |
| share (only with share binds) | tap `vs0`, mirred both ways with the end `vh1` of a veth pair whose other end `vh0` is in the share namespace | the next `ethN` with `169.254.77.2/30` | NFS to the task's own server |

- **Allow-list on `vd0`** (`tc` flower, `cls_flower` is built in): pass frames whose source MAC is `eth0`'s and that
  are IPv4 from the container's address, or ARP whose sender MAC and sender address are the container's; drop
  everything else, including VLAN-tagged frames, other ethertypes, IPv6 and link-local multicast.
- **DNS to dockerd's resolver**: first choice, a DNAT rule in the namespace's `PREROUTING` on `vn0` from
  `169.254.78.1:53` to dockerd's resolver ports (read from dockerd's own `DOCKER_OUTPUT` rules), with
  `route_localnet` on `vn0`; no process. `INPUT` filters after DNAT, so its accept rule matches the original
  destination: `-i vn0 -m conntrack --ctstate DNAT --ctorigdst 169.254.78.1 --ctorigdstport 53`, for UDP and TCP;
  the answers leave from `127.0.0.11`, which `route_localnet` lets out of `vn0`. To be shown in slice 3 with the
  `DROP` policies in place; if it fails, a forwarder (socat with `max-children`) as an unprivileged user in a cgroup
  with `pids.max` and `memory.max`, like the share server's.
- **Rate limits**: `vn0` gets a Firecracker rate limiter (as the share tap has, `vesper-microvm:726-729`); `vd0`
  gets none, as Docker sets none.
- **The share namespace** keeps ADR 0009's server, sandbox, `INPUT` rules and rate limiter. It now holds `lo`, `vh0`
  with `169.254.77.1/30` and the server; Firecracker no longer runs in it. The veth pair is made inside the share
  namespace, where BusyBox's default peer name cannot collide, then the peer is renamed and moved with `ip link set`
  (no new tool). No container packet reaches the server: `vh1` has no address and its traffic is redirected
  (spike: closed from the namespace's stack, open from the guest). What the guest itself does with its exports is
  the guest's (ADR 0009 holds for direct reach only).
- All links are made by `vesper-microvm ctrns-links <id> …` (shell, beside `vnetns_up`), so network plumbing keeps
  one owner.

### 4. The spec fields

| Field (from `run/B1/spec.txt`) | Handling |
| --- | --- |
| `process`: `args`, `env`, `cwd`, `user` (`uid`, `gid`, `additionalGids`), `terminal`, `rlimits` | honoured in the guest |
| `process.capabilities`, `linux.seccomp`, AppArmor, SELinux, `noNewPrivileges`, `oomScoreAdj` | ignored in the guest (CE-0004: the microVM is the boundary) |
| `hostname` | honoured |
| `root.path` | the image source (section 5); `root.readonly` refused (plugin) |
| mounts `proc`, `sysfs`, `cgroup`, `devpts`, `mqueue`, `/dev` tmpfs | made by the guest's runc as runc makes them |
| mount `/dev/shm` (tmpfs with a size), user `--tmpfs` | made in the guest with the size and options |
| binds of `resolv.conf`, `hosts`, `hostname` | the shim opens exactly `/var/lib/docker/<remap>/containers/<task id>/{resolv.conf,hosts,hostname}` with `openat2` and `RESOLVE_NO_SYMLINKS`, at most 64 KiB each, and writes them in the guest before the workload; later changes by dockerd do not reach the guest |
| binds from shared macOS directories | NFS from the task's server (ADR 0009) |
| binds under `/var/lib/docker/<remap>/volumes/` (anonymous volumes from an image's `VOLUME`; named ones are refused by the plugin) | the image's own directory in the container's drive (data not shared, deleted with the container) |
| `--init` (a bind of `docker-init`) | the guest's `tini` |
| any other bind, `linux.devices` entries | refused |
| `linux.namespaces` | `network` is the shim's; the others are made in the guest; any entry with a `path` is refused |
| `linux.uidMappings`, `gidMappings` | used to shift ownership (section 5); the guest runs no user namespace |
| `linux.maskedPaths`, `readonlyPaths` | honoured in the guest |
| `linux.sysctl` (dockerd always sets `net.ipv4.ip_unprivileged_port_start`) | honoured in the guest, whose kernel is its own |
| `linux.resources`: memory, CPU | honoured (TD-048) |
| `linux.resources`: `blockIO`, `devices` rules, `pids` | ignored |
| `linux.cgroupsPath`, `annotations` | ignored, except the shim's own annotation |
| `hooks` | refused if any |

The table is kept in the shim's README with a test for each row.

### 5. Root drive, ownership, storage

- The shim's jails move from the tmpfs root to `/var/lib/vesper/state/microvm/jailer/` on the data disk, apart from
  `vesperd`'s jails, and `vesper-memadmit` (`jail_of`) and `cleanup_locked` look there. Without a data disk the
  `ctr` path keeps its jails in tmpfs, booked as now, and a `vesper` container is refused at `Create` ("the vesper
  runtime needs the System VM's data disk"), because a drive that persists between starts would be unbooked
  memory.
- Each `vesper` container has one drive, `/var/lib/vesper/state/microvm/drives/<container id>.ext4`, built at its
  first `Create` from `root.path` and reused by later starts of the same container, so `docker stop` and
  `docker start` keep its writes. A sweep (in `vesper-data mount` and the reaper) removes drives whose container is
  gone from dockerd's store. The drive is hard-linked into the jail (one bind mount, so no cross-mount link); the
  bundle copy (`backend.go:115`) goes. `launch`'s `chown -R` of the jail (`vesper-microvm:764`) then also gives the
  drive to the jail user through the link; that is right for a drive that belongs to one container, and is why a
  shared base (slice 6) must be linked after the `chown` or bound read-only instead.
- Ownership: `mke2fs -d` runs in a user namespace with the spec's own mapping (`0 100000 65536`), so the image's
  owners appear as the container's own (0 for 100000). It runs as host uid 100000 and writes by path (mke2fs resolves
  its device path, so a descriptor does not serve): under a random name in `drives/.build`, which that uid owns only
  while it builds, below `microvm/` and `drives/`, which can be passed through but not listed. The shim checks that
  the published file is the one the build wrote, then gives it and `.build` back to root. A remapped runc container
  reaches the directory only through a bind the Docker client made, and cannot learn the name. dockerd refuses to
  extract owners outside the mapping, so such an owner fails the build. Shown in slice 2.
- Size and admission: a drive is the image's size plus a fixed writable allowance (default 4 GiB, a shim option);
  it is admitted only when the data disk keeps a free-space reserve (as `vesperd`'s store does); `vesper-data trim`
  runs after removals. Drives are sparse, so drives admitted one by one can later together pass the reserve: a
  recorded residual, bounded by the allowance.
- A shared read-only base image with a writable layer (TD-035) is deferred until slice 4 has measured start time
  and density.

### 6. Lifecycle

- **Two cleanup scopes** in `vesper-microvm`: *launch* (Firecracker, jail, links, tc, the forwarder if any, the
  veth pair) and *task* (the holder, the namespace binding, the record). A failed `Start` undoes the launch scope
  only; `Delete`, a failed `Create` and `manager.Stop` after a shim crash undo both. The holder is found from its
  record, checked by its start time, not by parentage; `ctrns-up` checks its namespace inode.
- **Sweeps.** At boot `vesper-data mount` removes the data-disk jails and build files of the last boot; the reaper
  removes a data-disk jail with no Firecracker, shim or namespace, and the drive of a container dockerd's active store
  no longer has (never when no store can be read).
- The reaper and `launch` learn the `ctrns` directory: a namespace with a live record is never a leftover.
- `Pause`, `Resume`, `Checkpoint` and `Update` stay refused with their messages until slice 5 (`docker pause`
  fails clearly; `docker commit` is refused anyway). Slice 5 adds Pause and Resume by Firecracker's vCPU pause,
  with a paused state and defined `Kill` and `Exec` while paused.
- dockerd's restart stops its containers, as stock Docker does without `live-restore`. A containerd restart while
  a task runs, and a task exit while containerd is down, are tested in slice 4, not assumed.

## Alternatives considered

| Option | Why not |
| --- | --- |
| Firecracker in the share namespace with a tap in dockerd's namespace | A process joins one network namespace, and Firecracker opens taps by name in its own. The share server beside dockerd's veth makes ADR 0009's condition 3 rest on `INPUT` rules, which ADR 0009 rejected |
| A tap on the Docker bridge, the guest on the bridge directly | The guest would not be in the container's namespace: port publishing, DNS and `network connect` would not see it, and a guest kernel on a shared segment needs the same filters with less control (ADR 0008, alternative 3) |
| `macvtap` on the veth | Needs a char device in the jail; mirred is shown to work |
| DNS over vsock | Needs a relay per task in the shim; DNAT needs no process |
| Rewrite the guest's `resolv.conf` to `169.254.78.1` | `dns-suite` checks that the container sees `127.0.0.11` (`scripts/systemvm/dns-suite.sh:43`); the guest-side address keeps dockerd's file |
| Plugin lookups for every route from the start | The create-time `container:` check guards a boundary and comes first; the honesty refusals can follow |
| A shared base drive and overlayfs now | The hard link crosses bind mounts, `chown -R` on the jail would take the shared inode, and nothing has measured the need yet |
| An idmapped mount of the rootfs | An idmapped mount of overlayfs is unproven on 6.12; `mke2fs` in a user namespace needs no kernel feature beyond user namespaces |
| Netlink calls from the shim for the veth pair | A second owner of network plumbing; BusyBox `ip` can make, rename and move the pair |
| microVMs as dockerd's default now | The owner chose opt-in first (D-066) |

## Rationale and consequences

- A `vesper` container keeps Docker's network model: its address, MAC, published ports, peers by address and by
  name, outbound access. It reaches what a runc container reaches (the System VM's listeners on the bridge gateway,
  the Mac through the vmnet gateway, the LAN, VPNs); ADR 0008's egress policy is for `vesperd` microVMs only.
- ADR 0010's guarantee for these containers moves from "a remapped uid on the System VM's kernel" to "a guest kernel
  in a jailed Firecracker in a namespace the container cannot change", provided sections 1 to 4 hold.
- New costs per container: a holder, two or three taps, possibly a veth pair, a drive on the data disk, a guest NIC
  per link; `iproute2-tc` in the System VM; a store reader in the plugin.
- Deviations, recorded in design-v0 and the tracker: `docker cp`, `diff`, `export`, `commit` and `network connect`
  until slice 5 (then refused); named volumes and `--read-only` refused; anonymous volumes are not shared; changes
  dockerd makes to `hosts` or `resolv.conf` after start do not reach the guest; IPv6 networks refused; `docker
  pause` refused until slice 5; `docker stats` reports the microVM's cgroup; `docker top` and `docker inspect` show the
  namespace holder's pid; `Update` refused; `linux/amd64` images do not run in a `vesper` container (the guest has no qemu-user);
  `crossarch-suite` runs on runc only.
- SECURITY.md's threat table gains the rows the pre-review found missing: the Docker socket holder's daemon-wide
  authority, the `vesper` container as its own row apart from runc, and the guest's NIC.

## Implementation and migration impact

Its own ExecPlan (B3 of the 2026-11-22 programme). Each slice is a PR with a council review and the suites below,
run on the System VM:

1. **Package lock**: `iproute2-tc` and its closure, added without moving other pins; THIRD_PARTY_NOTICES.md.
2. **The default bridge**: `daemon.json`; the plugin's allow and every create refusal of section 1, including the
   `container:` resolver; the holder, `ctrns-up`, the record and the two cleanup scopes; the root drive with
   ownership shift, jails and drives on the data disk, the sweep; the data link with its allow-list; `resolv.conf`,
   `hosts`, `hostname`; the spec table of section 4. New suite `make systemvm-docker-microvm-suite`: `docker run
   --runtime vesper` on the default bridge reaches a peer by address and the internet, serves a published port, keeps
   its writes across stop and start, sees its own owners; the share server is closed from the namespace's stack;
   each spoofing probe fails; each refused option is refused with its message; nothing is left after `docker rm`.
3. **User-defined networks**: the DNS link; `dns-suite` and the compose steps with the runtime forced to `vesper`.
4. **Shares**: the share link and binds for `vesper` containers and for `ctr` tasks; `binds-suite`,
   `systemvm-microvm-shares-suite`, `systemvm-microvm-binds-suite`, `systemvm-sharenet-suite`,
   `systemvm-confine-suite`. Then the Phase 4 suites with the runtime forced to `vesper` (a per-suite variable,
   `VESPER_DOCKER_RUNTIME`), each failing step mapped to a deviation; p50 and p95 of `docker run` and `docker stop`;
   how many containers of ordinary image size fit the default System VM; containerd restart; an exit while
   containerd is down.
5. **Honesty and pause**: the resolver refusals for `cp`, `diff`, `export`, `commit`, `connect`; Pause and Resume.
6. **Only if slice 4's numbers call for it**: a shared base drive (TD-035).

`ctr` users of `io.containerd.vesper.v2` keep working: with no interface in the namespace there is no data link.

## References

- design-v0: hard requirement (line 122), Next work step 2, D-015, D-066; its threat table.
- ADR 0004 (the shim), ADR 0006 (exec through runc in the guest), ADR 0008 (egress for `vesperd`), ADR 0009
  (shares), ADR 0010 (remapping and the plugin).
- `run/B1/findings.md` and `run/B1/spec.txt` (spike), `run/egm/ADR-0011-review.md` (reviews).
- Code: `shim/internal/task/service.go`, `shim/internal/bundle/`, `shim/internal/backend/backend.go`,
  `shim/internal/authz/policy.go`, `shim/internal/manager/manager.go`,
  `systemvm/overlay/usr/local/bin/vesper-containers`, `vesper-microvm`, `vesper-data`, `vesper-memadmit`,
  `systemvm/overlay/usr/local/lib/vesper/netns.sh`.
- GH #53, TD-032, TD-035, TD-042, TD-051 (this ADR's review is the threat-table pre-review).
