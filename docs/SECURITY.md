---
status: verified
owner: cityhunteur
last_reviewed: 2026-11-14
---

# Vesper Security Review Map

Vesper’s security model is part of its parity contract. Treat this file as the entry point for security-sensitive work.

## Trust boundaries

1. Host macOS control plane
   - `vesperd`
   - host-visible Unix sockets
   - entitlement-bearing binaries
2. Linux System VM
   - containerd and internal runtime surfaces
   - optional stock Firecracker and jailer for Backend A
3. Guest microVM workloads
   - isolated workloads behind the control plane

## Host exposure rules

- `vesperd` is the public control-plane endpoint.
- `vesper-proxy` may expose a host-visible socket only for the intended Docker or container runtime UX.
- Raw Firecracker and raw containerd sockets stay private to the System VM.
- Shipped defaults (`src/bin/vesperd.rs`, `src/bin/vesper-proxy.rs`): `vesperd` binds `vesperd.sock` in an owner-only runtime directory (the first of `$XDG_RUNTIME_DIR/vesper`, `$TMPDIR/vesper` and `$HOME/.vesper/run`, created with mode 0700) with mode `0o600`, refuses a socket directory writable by group or others, and never removes a
  socket another process is serving (TD-050, resolved 2026-11-12); `vesper-proxy` listens on `/tmp/vesper/docker.sock` with mode `0o600` (`--listen-mode`) and dials
  `vsock://3:2375`, which is bridged through a host Unix socket `/tmp/vesper/vsock/<cid>.sock` with a `CONNECT
  <port>` preface. The `vesper-proxy` defaults are world-writable temp directories; `make dev-up`, `make systemvm-up` and `make docker-up`
  use `run/` instead, and packaged builds must pass explicit paths.
- Host-visible sockets with the System VM running (`make systemvm-up && make docker-up`), all under `run/` and
  owner-only: `run/vesper/docker.sock` (the Docker ingress, served by `vesper-proxy`) and `run/systemvm/vsock/3.sock`
  (the launcher's vsock bridge, which connects only to the vsock ports listed in the launch config (`bridge_allowed_ports`: 1024, 2375, 2400, 2401) and refuses every other port (TD-027, resolved 2026-11-12)). Inside the guest four vsock ports
  listen: 1024 (`vesper-agent`), 2375 (relay to `/var/run/docker.sock`), 2400 (published-port relay, guest
  loopback only) and 2401 (share notification sink, below). `containerd.sock` and `docker.sock`
  themselves never leave the guest, and a `CONNECT` to a guest port without a listener is refused.
  `make docker-smoke` checks the socket list and the refusal (`no_raw_host_sockets`, `raw_guest_port_denied`).

## Entitlements

- Plists: `packaging/macos/vesperd.entitlements` (`com.apple.security.app-sandbox`, `com.apple.security.virtualization`)
  and `packaging/macos/vesper-proxy.entitlements` (`com.apple.security.app-sandbox`). `make validate-entitlements`
  lints them; with `plutil` present it checks syntax only, and the required-key check runs on the Python path
  (`VALIDATE_ENTITLEMENTS_FORCE_PYTHON=1`). CI runs it in `rust-tests.yml`.
- `vesperd --entitlements-file <plist>` checks only that the literal `<key>com.apple.security.virtualization</key>`
  appears (`src/platform.rs`); the value is not inspected, and without the flag no check runs. `vesper-proxy` has no
  entitlements flag.

## Security-sensitive changes

Treat these as security-sensitive:
- entitlement changes,
- socket exposure changes,
- filesystem boundary changes,
- sandbox or process-lifecycle changes,
- proxy target parsing,
- any change that widens host ↔ System VM reachability.

## Required checks

At minimum, use:
```bash
make validate-entitlements
cargo test
python3 scripts/harness/check_structure.py --root .
python3 scripts/harness/janitor_scan.py --root .
```

`rust-tests.yml` runs `cargo test` and `make validate-entitlements` in CI for any `src/**` or `packaging/**` change.
Plus runtime evidence when the change affects real transport or VM lifecycle.

## Review rules

- Do not broaden entitlements without a concrete requirement and proof.
- Do not expose a new host-visible socket unless the design doc or execution plan says why.
- Do not accept undocumented compat drift as a silent temporary state.
- Keep boundary parsing explicit and deterministic.

## Links

- [`specs/design-v0.md`](specs/design-v0.md)
- [`process/baseline-governance.md`](process/baseline-governance.md)
- [`process/compat-exceptions.md`](process/compat-exceptions.md)
- [`RELIABILITY.md`](RELIABILITY.md)

## Published ports (GH #28, ADR 0005)

`vesper-proxy --publish-ports` (on by `make docker-up`) is the first host-visible TCP surface Vesper opens: one
listener on `127.0.0.1:<host port>` per TCP `-p` binding of a running container, closed when the container
stops or the proxy exits. Listeners are loopback only; a binding to any other host ip is recorded as
`unsupported` and not opened. Every connection travels the bridge to the guest gateway `vesper-port-relay`, which
only connects to guest loopback ports, so nothing in the guest becomes reachable that dockerd did not publish.
The live set is `run/vesper/ports.json`. Widening to non-loopback addresses is a human decision.

## Shared host directories (GH #29)

A host directory enters the System VM only when named in `VESPER_SHARES` (or `systemvmctl launch-config
--share`); nothing is shared by default. Shares are mounted in the guest at the host's absolute path and are
visible to every container that bind-mounts them; `PATH:ro` makes the share read-only at the virtio-fs level,
which no container can override. Guest writes land on the host as the sharing user. Sharing `$HOME` or wider
trees is a human decision.

The share notification sink (GH #150, `vesper-share notify` on guest vsock port 2401) lets the host name paths
that the guest then replays, so that containers get inotify events for host edits. It resolves each path with
`realpath` and acts only inside a read-write virtio-fs mount; it rejects relative paths, `..` components, paths
outside the shares and symlinks that leave a share; it skips share roots and read-only shares; it compares
mount points literally, also when they contain `*`, `?` or `[`; it never creates a file and never changes
contents. `make share-notify-selftest` checks the policy.

The replay of one path is three steps. First an atime-only set (`touch -c -a -r P P`), whose reply refreshes the
attributes the guest has cached for the file. Then an mtime-only set (`touch -c -m -r P P`), which makes the
guest kernel emit `IN_MODIFY`. Then, only if the file's size changed between the two, `touch -c -m P` (mtime
set to now, reported with reason `raced`). The effect on host timestamps is:

- mtime: the replay leaves it equal to the last host write, with two exceptions. When a host write lands between
  the two sets, the mtime becomes the time of the replay, which is never older than that write. A same-size
  overwrite inside that window (about a millisecond) is not detected and can keep the previous write's mtime.
- atime: the replay sets it to the guest's cached atime, which can move it backwards.
- ctime: every replay changes it.

The first step exists because a single-step replay moved host mtimes backwards by about a second when one file
was edited twice within the guest's attribute cache lifetime (found by the council rework of 2026-11-11;
[decision log](design-docs/decision-log.md) D-005).

In a boot with remapped containers (ADR 0010) the sink does not run as root. Before its first lookup it changes to
container root, uid and gid 100000 from `dockremap` in `/etc/subuid` and `/etc/subgid`, with `setpriv`, no
supplementary groups, `no_new_privs`, and `CAP_FOWNER` as its only capability, which it needs to set the times of
a file it does not own. The share reports each file as owned by whoever asks, so the owner the replay leaves in
the System VM's attribute cache is the one a container expects (TD-062). In an unmapped boot it stays root.

Two sequences in the sink are not atomic. The path check and the touch: a container that can write to a share
could swap in a symlink between them, and the worst outcome is that one guest file outside the share has its
atime and mtime rewritten; its contents do not change. The read of the mtime and the write that sets it: this is
the window behind the two mtime exceptions above.

The sink's `socat` listener runs at most four handlers at a time (`max-children=4`); the launcher holds one
connection. The port is reachable from the host only through the launcher bridge, whose allow-list includes it (TD-027); whoever reaches it
can make any path inside a read-write share emit `IN_MODIFY` and nothing more.

The host half is the launcher's share watcher: it reads FSEvents for shares marked `watch` in
the launch config (read-write shares unless `PATH:nowatch`), sends path names only, never file contents, never
writes to a share itself, opens no host socket and needs no entitlement beyond the launcher's existing one. It
never blocks shutdown: its writes to the sink are non-blocking and the exit report reads counters published
under a lock, so SIGTERM and a guest shutdown do not wait for a sink that has stopped reading
([RELIABILITY.md](RELIABILITY.md#share-watcher-gh-150)).

## System VM data disk (GH #164)

The System VM keeps its state on a raw disk image on macOS, by default `run/systemvm/data.img`
(`VESPER_DATA_DISK`). `scripts/systemvm/generate-launch-config.sh` creates it sparse with mode 0600, so only the
owner can read it: it holds container images, containerd and dockerd state, named volumes and microVM root
drives, any of which can contain secrets. The launcher attaches it as a writable virtio block device; it is a
file, not a socket, needs no entitlement beyond the launcher's existing one, and makes nothing new visible on the
host. Another user who can read the file can read every container's data, so do not widen its mode or put it on
a shared volume. The guest formats only a disk in which `blkid` finds nothing, and refuses any other filesystem or label
(`systemvm/overlay/usr/local/bin/vesper-data`), so an image with a filesystem named by mistake in `VESPER_DATA_DISK`
is not erased; a file with no recognisable filesystem or partition table is formatted.
Deleting the image with the System VM stopped erases all container state; nothing else holds a copy.

## `vesperd` microVMs in the System VM (ADR 0007, slice 3)

Three agent verbs on vsock port 1024 serve `vesperd` microVMs; no new port, socket, share or entitlement is added,
and the bridge allow-list is unchanged.

- `LAUNCHAPI <id> [ceiling] [stage=SRC:DEST|tap=NAME|tapaddr=NAME:ADDR/PREFIX|net=none|net=nat]...` starts one stock Firecracker under the jailer with no configuration file: uid and gid
  20000 (the shim's microVMs run as 10000), cgroup `/sys/fs/cgroup/vesperd/<id>` (`--parent-cgroup vesperd`,
  `pids.max=256`, `memory.max` from `ceiling`, default 640 MiB, at least 128 MiB and at most the System VM's
  memory), chroot `/var/lib/vesper/state/vesperd/jailer/firecracker/<id>/root` on the data disk (slice 5; refused
  with `ERR data disk not in use` without it), the prepared kernel and initramfs as
  `/Image` and `/initrd.gz`. After `READY` the connection is the lease: its end stops the microVM and removes the
  jail, cgroup and record; Firecracker's exit closes it.
- `FCAPI <id>` relays bytes to that microVM's API socket. The relay socat chroots into the jail root before it
  connects (`chroot-early`), so a symlink that a compromised Firecracker plants in its jail, which it owns, cannot
  lead the root-run socat to a socket outside the jail. It adds nothing `FCPARITY api` could not already do.
- `VESPERD <args…>` inspects and faults these microVMs for the suite: `status`, `isolation`, `files`, `memory`,
  `list`, `log`, `reap`, `kill-holder`, `kill-fc` and the test hooks `test-forge-start`, `test-orphan`, `test-age`,
  `test-plant`, `test-reserve` and `test-container-vsock`; since slice 5 it also carries `stage`, `store-list` and
  `store-rm` (next section), and since W4 `logsize`, `mem-budget` and three hooks that loosen or age the new bounds:
  `test-mem-reserve` (overrides the 256 MiB memory reserve), `test-store-quota` (overrides the store quota) and
  `test-age-log` (ages a console log for the reaper). Their overrides live on the System VM's tmpfs and clear at
  reboot. It cannot launch or connect. Like `MICROVM crash`, the hooks need nothing beyond what
  the agent already is: root in the System VM.

Every id is checked against the jailer's rule (1 to 64 letters, digits and hyphens) before it is joined into a path:
by `LAUNCHAPI`, `FCAPI`, `VESPERD` and, since slice 3, `FCPARITY`, which earlier joined an unchecked id. The two
owners avoid collisions by naming, not isolation: `vesper-microvm` finds its processes with `pgrep -u 10000`, so the
shim never acts on a `vesperd` microVM with the same id, and the `vesperd` tools match uid 20000.

Root reads only one file inside a jail where Firecracker has run: the jailer's `firecracker.pid`, once, when the API
socket appears and before guest code runs, and only if it is a regular file and names a uid 20000 Firecracker
with that id. Later lookups use the root-only record `/run/vesper/vesperd/leases/<id>`, which stores each pid with
its start time, so a reused pid is never signalled. A reaper (`vesper-vesperd-reaper`, every 10 s, and before each
`LAUNCHAPI`) stops a microVM whose lease shell is gone, and a jail without a record after 60 s; one `flock`
serialises reaping, jail creation and teardown.

A default Docker container cannot reach the agent from inside the System VM: Docker's default seccomp profile denies
`socket(AF_VSOCK)` (`make systemvm-launchapi-suite`, step `default_container_cannot_open_vsock`). A container with
`seccomp=unconfined` can open the socket, but its connects fail because no vsock loopback transport is loaded, the
module is blacklisted, and no container can load a module (ADR 0010). The agent also refuses a peer that is not the
macOS host.

## Network interfaces for `vesperd` microVMs (CE-0010)

`vesperd --tap NAME` (at most 8; 1 to 15 letters, digits, `_` and `-`, never `lo`, checked on both sides) adds a
`tap=NAME` token to `LAUNCHAPI`. No port, socket, share, package or entitlement is added. Every lease, with or
without taps, gets its own network namespace, bound at `/run/vesper/vesperd/netns/<id>` (root-only directory, tmpfs)
and joined by the jailer (`--netns`) before it drops to uid 20000, so Firecracker no longer shares the System VM's
network namespace. In it, root brings up `lo` and creates each tap with busybox `tunctl -u vesperd-jail`; the tap is
persistent and owned by uid 20000, which is what lets the jailed Firecracker open it without `CAP_NET_ADMIN`. A name
not given cannot be created by Firecracker and fails with Firecracker's own `EPERM` error.

With `--network none` (the default) the namespace has no route out: no veth, bridge, NAT or DNS. A guest reaches its
own taps, and the far side of a tap is an interface nobody else reads, so a guest cannot reach the System VM, other
microVMs, containers, the macOS host or the internet through it. `--network nat` is the one opt-in uplink (next
section; the owner asked for the most secure option on 2026-11-13, D-028). Teardown, the reaper and `status` (`DIRTY <id> netns`) cover the namespace; `VESPERD net <id>` lists its
interfaces and `isolation` reports `netns_ok`.

## Egress for `vesperd` microVMs (ADR 0008)

`vesperd --network nat` (opt-in; `none` is the default) gives a microVM egress; [ADR 0008](design-docs/adr/0008-vesperd-microvm-egress.md)
is the normative policy and this section the summary. No new port, socket, share, package or entitlement is added.

- **Path.** One veth pair per lease, `vpd<N>` in the System VM and `up.<N>` in the lease's namespace, a /30 from
  `198.18.0.0/16`; the namespace masquerades out of `up.<N>` and the System VM out of `eth0`. Tap gateways
  (`--tap NAME:ADDR/PREFIX`) are RFC 1918, prefix length 16 to 30, clear of `eth0`'s subnet, `docker0`, the pool and
  the resolver, checked by `vesperd` and again by the guest helper.
- **Allowed.** Public unicast IPv4 over TCP, UDP and ICMP echo, and replies; DNS to the tap gateway, DNATed to the
  System VM's IPv4 resolver (a `nat` launch fails without one). At most 1024 tracked flows per lease, in both layers.
- **Denied.** RFC 6890's special-purpose IPv4 table (loopback, RFC 1918, CGNAT, link-local and metadata, the
  benchmarking pool, documentation ranges, multicast, reserved, broadcast) and `eth0`'s subnet: so the System VM,
  the Mac's vmnet gateway (other than DNS), containers, other leases and the LAN. `INVALID` packets, other IP
  protocols and other ICMP types are dropped. Nothing can open a connection to a lease, and nothing in a lease's
  namespace (Firecracker included) can open one either: its `INPUT` and `OUTPUT` accept only `lo` and replies.
- **Two layers.** The lease's namespace (`iptables` and `ip6tables` policies `DROP`; gone with the namespace) and
  static root rules: `raw PREROUTING` drops sources outside the pool, `mangle PREROUTING -i vpd+` runs the policy
  before routing (so it also covers the System VM's own addresses), `mangle POSTROUTING -o vpd+` drops anything
  that is not a reply, and `ip6tables` drops all IPv6 on `vpd+`. Docker owns `FORWARD`; Vesper only adds accepts to
  `DOCKER-USER`, so a Docker restart can cut `nat` egress but cannot open a path. `mangle INPUT` and `FORWARD` also
  drop anything from `vpd+` that is for the System VM itself or not bound for `eth0`. `make systemvm-egress-suite`
  shows each root table holding alone: `mangle` with the namespace filter opened after dockerd was killed and
  respawned under a running microVM, `raw` with the namespace's masquerade and reverse-path check removed too.
- **Limits.** The 1024-flow limit comes before every accept, DNS included, and the egress suite floods it in each layer; tap names may not start with `veth` or
  `vpd`; every lease's namespace, `none` included, has IPv6, redirects and loose reverse-path filtering off.
- **Accepted risks** (TD-058): a VPN on the Mac that routes a public prefix is reachable, because address filtering
  cannot see macOS routes; guests' DNS queries, including names that resolve to private addresses, reach the macOS
  resolver (the addresses themselves stay blocked); DNS can carry data out (tunnelling), as any egress can.
- **Inspection.** `VESPERD net <id>` prints the policy, the veth addresses and both layers' drop counters;
  `isolation` adds `ipv6_off`, `egress` and `egress_ok`; `list`, `status` and the reaper cover `veth` (the record
  `/run/vesper/vesperd/net/<id>` outlives the lease until the link is gone).

## Share network for the shim's microVMs (ADR 0009, GH #160)

Built so far: the network (M2), the server (M3), the guest's mounts (M4) and the shim's mapping and kill path (M5). A containerd task on the Vesper shim whose bind mount comes from a macOS share gets it as an NFS mount
from its own microVM's server. Docker containers do not use the shim and still get virtio-fs binds through runc.

- **Mapping (shim):** a bind mount whose source, with symbolic links followed in the System VM, lies under a
  virtio-fs share mount becomes one export (`--unfs-src`), and the guest mounts it at the bind's destination; a
  source that is a file, or binds from two different shares, are refused with an error naming the mount; other bind
  mounts keep today's behaviour (ignored by the microVM path).
- **Guest:** `vesper-console shares mount` probes the server for 3 s, then mounts each share under the task's root,
  resolving the destination with `openat2(RESOLVE_IN_ROOT)`, so a symbolic link in the image cannot place a mount
  outside it, and mounting on the descriptor; a link whose target is missing fails the mount. Shares are synced and
  unmounted, deepest first, before the session reports `EXIT`.
- **Events (M6):** the share notification sink passes each path it replays to `vesper-unfs fanout`, a root process
  that queues it (bounded, coalescing, 10,000 paths per microVM) for each microVM whose server exports it and sends
  `N<TAB>rest` lines to the guest's vsock port 1026 through the jail's `v.sock`. The guest's replay resolves each
  path beneath the task's root with no link leaving it and sets its mtime to its own value (read-only sources get
  no event: no times can be set on a read-only mount). unfs3 keeps whole seconds, so a macOS mtime's fraction is
  lost once per edit and that change comes back once (TD-066). Only the System VM can reach that port; the guest is
  the task's own.
- **Replay identity:** the sink waits for the boot's container mode before choosing its uid, and refuses without it;
  it used to decide at connection time, and a launcher that connected early made the whole session replay as root
  (TD-062's fault). It logs `replay uid=` to syslog.
- **Bounds at session start:** a 3 s probe of the server, then each mount in a child process killed after 5 s, so a
  server that accepts the connection but does not answer fails the `RUN` with an error naming the export.
- **Accepted deviations (D-038):** TD-065, TD-066, TD-067, and no host-edit events on read-only binds.
- **Kill and stop:** a `SIGKILL` to the init process gets 10 s to report `EXIT` (the guest's unmount flushes its
  writes); after that the shim stops the VMM, so a dead server cannot hold a task. The server stops when the task
  stops; `Delete` cleans up again.

- `vesper-microvm launch ID --share-net` gives that microVM a network namespace of its own, bound at
  `/run/vesper/microvm/netns/<id>` (root-only, tmpfs) and joined by the jailer (`--netns`). It holds `lo` and one tap,
  `share0`, owned by uid 10000 (user `vesper-shim-jail`), with `169.254.77.1/30` on the System VM side and
  `169.254.77.2` for the guest. There is no veth, no route out and no forwarding. Every namespace uses the same
  address pair, because none can see another.
- **What can reach `169.254.77.1` in a namespace:** that microVM's guest, on TCP 2049 and 2050 only (an `INPUT` rule
  in the namespace; both policies are `DROP`). Nothing listens in the root namespace, so a container, on the Docker
  bridge or on the host network, privileged or not, has no route to it; nor has another microVM. The rule is defence
  in depth for a later uplink (TD-032), not what isolation rests on.
- **Lifecycle:** `launch` writes `records/<id>` (its pid, start time and the time) under a lock before it makes the
  namespace, and undoes the launch on `INT`, `TERM` or `HUP`. `cleanup` removes the namespace and the record.
  `vesper-microvm-reaper` (every 10 s, under the same lock) runs `cleanup` for a record older than 30 s whose launch,
  shim and Firecracker are all gone, and removes a namespace that has no record. Every launch holds that lock from
  its already-running check to its namespace, and `cleanup` takes it too, so a reaper pass cannot give back a new
  launch's memory admission or remove its jail. `vesper-ctr leftovers` counts
  namespaces and records. These directories are the shim's own: `vesperd`'s namespaces and reaper use
  `/run/vesper/vesperd`, and the helpers they share (`/usr/local/lib/vesper/netns.sh`) take the directory and the
  tap's owner as parameters.
- **Who counts as the owner of a microVM:** the reaper and `cleanup` find a Firecracker by uid 10000, its command
  line and its cgroup (`/firecracker/<id>`), and a shim by its binary's device and inode and an exact `-id` argument.
  A container process with a look-alike command line, or one run as uid 10000, passes for neither, so it cannot
  keep an orphan's namespace alive.
- **A container started with any flags Docker accepts** cannot enter this namespace, reach the address or touch a
  process in it, because no container is root on the System VM
  ([Container confinement](#container-confinement-adr-0010-gh-194); D-034). `make systemvm-confine-suite` starts a
  container with every capability, no seccomp filter and the System VM bound at `/host`, and shows that `setns` into
  the namespace, a connect to `169.254.77.1:2049` and a read of the launch record all fail. The one exception is the
  upstream suite's unmapped boot, described in that section.
- **Guest:** `vesper_share_net=169.254.77.2/30` makes the init configure `eth0` with that address and no gateway.
- **Test hooks**, for `make systemvm-sharenet-suite` only: `launch --share-probe`, `test-share-listen`,
  `test-share-reach`, `test-age`, `test-abort-launch`, `test-drop-record` and `test-impostor`. They need root in the System VM, like every `vesper-microvm`
  command.
- **Evidence:** `make systemvm-sharenet-suite` (12 steps, `run/GH-160/m2-sharenet-suite.json`).

### The share server (GH #160 M3)

- `vesper-microvm launch ID --share-net --unfs-mount DIR --unfs-src REL:ro|rw ...` starts one `unfsd` for that
  microVM, in its namespace, before the jailer. `DIR` must be a virtio-fs mount named without white space or a
  backslash. The server exports each source as `/s/0`, `/s/1`, ... to `169.254.77.2` only, so no source path reaches
  its exports file. It listens on `169.254.77.1` (TCP 2049 and 2050, through an IPv4-mapped IPv6 socket).
- **Sources** are opened by `vesper-unfs run` with `openat2` beneath the share mount, with no symbolic link on the way
  and no crossing to another mount, then cloned into detached mount trees (`open_tree`) with their final attributes
  (`nosuid,nodev,noexec`, plus `ro` for a read-only source). Nothing opens a source path again. A symbolic link, a
  `..` component or a mount that is not virtio-fs is refused, and the launch fails.
- **Sandbox:** new mount, pid, ipc and uts namespaces; `pivot_root` into a 1 MiB tmpfs that is then read-only, with the
  old root detached. It holds only the exports, a read-only `/usr` and `/lib` (for `unfsd` and its libraries),
  `/dev/null`, and `/etc` with the exports file and libtirpc's tables. No `/proc`, `/sys`, `/run` or device. Each
  read-only source is bound again, read-only, inside each read-write source that holds it, at a place found beneath
  that export with no symbolic link on the way. The server runs as root with the bounding, permitted and effective
  sets cut to `CAP_CHOWN`, `CAP_DAC_OVERRIDE`, `CAP_FOWNER`, `CAP_FSETID`, `CAP_SETGID` and `CAP_SETUID` (`0xdb`),
  no ambient capabilities, `no_new_privs`, and a seccomp filter that refuses user namespaces (`unshare` and `clone`
  with `CLONE_NEWUSER`), `setns`, and `clone3` (D-037). `--unfs-squash` runs it as 65534 with no capabilities instead.
- **Supervision:** `supervise-daemon` (service `vesper-unfs-ID`, respawn after 1 s, at most 30 in 60 s), in
  `/sys/fs/cgroup/vesper-unfs/ID` with `memory.max` 32 MiB and `pids.max` 32. The memory is admitted with the
  microVM's ceiling as one entry. The log is `/var/log/vesper-unfs-ID.log` (bounded, 256 KiB). `cleanup` stops the
  supervisor, kills what is left in the cgroup and removes it; `DIRTY` names `unfs` or `unfs-cgroup` otherwise.
- **Inspection:** `vesper-microvm unfs ID` (state, listening, memory), `unfs-isolation ID` (ids, capabilities,
  `no_new_privs`, seccomp, namespaces and every mount in the server's root), `unfs-log ID`.
- **What remains:** some malformed file handles, of the right length or shortened, make `unfsd` 0.10.0 abort on an
  assertion; the guest is the only client, so it can stop only its own shares, and after 30 aborts in 60 s they stay
  down (TD-065). `..` at an export's
  root returns a handle of the sandbox's `/s` with its attributes; every operation on it is refused.
- **A nested read-only source** is bound only on the directory that was opened as that source (same device and
  inode); a directory renamed and replaced on macOS before the bind is placed makes the server refuse to start.
- **Evidence:** `make systemvm-unfs-suite` (12 steps, `run/GH-160/m3/unfs-report.json`).

## Container confinement (ADR 0010, GH #194)

No container is root on the System VM, whatever flags started it. This is what keeps a container out of a microVM's
share namespace (ADR 0009, D-034) and away from Vesper's own state.

- **Remapping:** dockerd runs with `userns-remap`. Container root is uid 100000 on the System VM
  (`vesper-containers prepare`, called by `vesper-services` before dockerd starts). Capabilities a container is
  given hold only inside its own user namespace.
- **Refused by dockerd under remapping:** `--privileged`, `--pid=host`, `--network=host`. `--ipc=host` fails to
  start.
- **The authorisation plugin** (`vesper-authz`, `shim/internal/authz`) closes the switch that turns remapping off,
  and it is default-deny:
  - A request whose method and cleaned, version-stripped path are not on its list is refused. Swarm, managed
    plugins, checkpoints and BuildKit (`/session`, `/grpc`, `/build?version=2`) are not on the list.
  - A container create is refused unless dockerd passed its body (dockerd withholds a body of 1 MiB or more) and
    the body has no `UsernsMode`, no runtime but `vesper-runc` (the default) or `runc`, and no volume driver options. Volume create follows
    the same rule.
  - A bind whose source does not exist is refused unless the source lies inside a shared macOS directory. dockerd
    creates a missing source and gives it to container root; on the System VM's own filesystems that would hand a
    container a directory a root tool later adopts.
  - dockerd refuses every request while the plugin does not answer. The plugin runs under `supervise-daemon`,
    starts before dockerd, and logs each refusal (method, path, reason; no query or body) to the bounded
    `/var/log/vesper-authz.log`.
- **Owner-only state.** dockerd can bind any single file into a container whatever its parent directory allows, so
  the files themselves are closed, not only their directories:
  - console logs (`vesper-boundlog`, also after rotation), launch records, leases, the memory ledger;
  - workload records and images, the rootfs store and the shim's rootfs cache, bundle metadata;
  - everything in a shim jail; files Firecracker writes (it runs with umask 077);
  - snapshot store files, now `root:vesperd-jail` mode 0440 instead of world-readable 0444;
  - the logs of dockerd and containerd.
  `/var/log/messages` stays world-readable; it holds reaper lines with task ids.
- **Before the first container** (`vesper-containers prepare`, every boot; a failure refuses the boot): every
  state directory a root tool would otherwise make lazily exists, root-owned and 0700, and files that a build
  before this change left on the data disk get their modes (images and records 0600, snapshots
  `root:vesperd-jail` 0440). `vesper-microvm prepare`, `vesper-memadmit` and `vesper-ctr` also refuse a state
  directory that root does not own.
- **vsock:** the agent, the Docker relay, the port relay and the share notifier refuse a peer that is not the macOS
  host (CID 2), or a socat handler with no peer address (`tests/vsock_peer.rs`), and
  `vsock_loopback` is blacklisted. A container without a seccomp filter can still make a vsock socket. Its connects
  to CIDs 1 and 2 go to the macOS host, which has no listener and resets them; a connect to the System VM's own CID
  fails with "no such device". The Docker and port relays each hold at most 256 connections (`max-children`);
  past that socat waits for one to end (council finding 20).
- **Data disk:** a disk records the container mode of the first dockerd that used it
  (`/var/lib/vesper/state/containers-mode`). A boot in the other mode does not start dockerd.
- **The one unmapped boot.** `vesper.containers=unmapped` on the kernel command line starts dockerd without
  remapping and without the plugin. Only the launch config on macOS carries that line. The upstream Firecracker
  suite sets it in its temporary config, with a data disk of its own (`run/systemvm/upstream-data.img`), and keeps
  the saved normal config outside every shared directory. **In that boot a container can be root on the System VM
  and ADR 0009's host-root row does not hold.** `make docker-up` prints the mode and refuses to serve Docker when
  the System VM's mode differs from the launch config's, and refuses an unmapped boot altogether unless the
  upstream runner started it (`VESPER_ALLOW_UNMAPPED=1`), so a run that was killed cannot leave Docker served
  unmapped. A refused boot writes a dockerd configuration that names a plugin that does not exist.
- **What is not guaranteed:** isolation between containers. They share one mapping and one kernel: a container
  given `/` reads other containers' volumes and every macOS share (virtio-fs does not check uids), and a kernel
  exploit is out of scope here. A microVM per container (GH #53) answers both.
- **Ownership on shares (TD-062, resolved).** The share reports every file as owned by whoever asks, and the System
  VM caches the answer for about a second for all uids. Container root is not the System VM's root, so a look by
  root left a container seeing the file as owned by `nobody`, unable to `chmod` or `touch` it, for up to a second.
  Two lookups by root did this, and neither does now:
  - dockerd and runc look up each bind source while they create a container. dockerd's default runtime is
    `vesper-runc` (`shim/cmd/vesper-runc`): runc, and after `runc create`, before the process starts, a
    `statx(AT_STATX_FORCE_SYNC)` of each bind source on a share with the container process's uid and gid as the
    thread's filesystem ids. The step never fails a create and gives up after 2 s; its last result is in
    `/run/vesper/runc-refresh`. Every other runc command is runc itself.
  - The host-edit replay runs as container root (see the share notification sink above).

  What remains: a container running as another user (`-u 1000`) sees a file the replay touched as owned by its
  own root for up to a second; two containers with different users on one bind see each other's uid for up to a
  second after the other looked; `docker exec -u` gets no refresh; `--runtime runc` gets none; and no uid can
  change the attributes of a share's root directory (TD-038).
- **Docker data from before this change** stays in `/var/lib/docker` on the data disk, unused. dockerd's mapped
  store is `/var/lib/docker/100000.100000`. `HEALTH` and `make docker-up` report the old store; nothing deletes it.
  `vesper-data legacy import-volume NAME` (agent verb `DATA`, from macOS only) copies one old volume into a new
  volume, each owner shifted by 100000; an owner outside the 65536-id range becomes the container's `nobody`, so
  no imported file is owned by a System VM uid outside the mapping. It only reads the old store. The copy is made
  and shifted in `/var/lib/docker/100000.100000/.vesper-import`, a root-only (0700) directory that container root
  cannot enter, so a container cannot swap a path in the copy while root changes owners in it; the finished copy
  is then renamed into the volume. The volume is created with a one-time label `vesper.import`, and the import
  uses (or, on failure, removes) only a volume that carries it, never one another client made meanwhile. A
  hard-link marker it cannot make, or a `find` that fails, fails the import. What remains: an entry that was a
  hard link, in the old volume, to a root-only file elsewhere on the data disk is copied and becomes the
  container's (TD-064).
- **Test hooks:** `vesper-containers test-authz stop|start`, `vesper-data test-relabel`, `vesper-data test-import`
  (imports from a scratch store under `/run/vesper/legacy-test`). All need the agent.
- **Evidence:** `make systemvm-confine-suite` (15 steps, `run/GH-194/m5-confine-report.json`; the import step in
  `run/GH-194/m6/`); `cd shim && go test
  ./internal/authz ./internal/runcwrap`; 23 other System VM and Docker suites from the same overlay
  (`run/GH-194/td062/battery/summary.txt`). `make binds-suite` checks ownership in a container's first command
  and in an inotify handler on a replayed host edit, with no wait; with `--runtime runc`, or with the replay as
  root, the same checks fail (`run/GH-194/td062/control.txt`).

## Staging and the snapshot store (ADR 0007, slice 5)

Files reach a `vesperd` jail only through `vesperd` itself: `vesperd --stage SRC:DEST` (tokens on `LAUNCHAPI`,
staged into the fresh jail before the jailer starts) and `vesperd stage --id ID SRC DEST` (the agent's `VESPERD
stage`, while the microVM runs). No new port, socket, share, package or entitlement is added. `SRC` is
`share:/PATH` (inside a virtio-fs share of the launch configuration, which the System VM mounts at its macOS path),
`store:NAME` or `jail:/PATH` (the microVM's own jail); `DEST` is a jail path or `store:NAME`. The store,
`/var/lib/vesper/state/vesperd/store` (root, mode 0700), is shared by all of the macOS user's `vesperd` microVMs
and is one trust domain; `vesperd store list|rm` list and remove its files.

A compromised microVM controls the files in its jail (Firecracker owns the jail root), so `vesper-vesperd`:

- never opens, as root, a path inside a jail that Firecracker may have run in. Jail-side checks, reads and writes run
  as the `vesperd-jail` user (uid and gid 20000, no login shell, created at boot by `vesper-vesperd setup-user`)
  through busybox `su -s /bin/sh vesperd-jail -c …`; root opens only share files and the store files it created,
  and pipes bytes to or from that process. A planted root-only file (`test-plant`, `/planted-root`, mode 0600) is
  refused with `permission denied for uid 20000` (`make systemvm-snapshot-suite`, step
  `jail_side_read_runs_as_uid_20000`);
- refuses a symlink in any component of `SRC` or `DEST` below the share mount or the jail root, and any `SRC` that is
  not a regular file; paths are absolute, of letters, digits and `._+,@=-`, with no `.` or `..` component, so they
  stay one word in the agent's line and in `su`'s command line (steps `refuses_symlinked_src`,
  `refuses_symlinked_dest`, `refuses_src_outside_roots`);
- writes a new file only (`set -C`, so `O_EXCL`, which also refuses a symlink as the last component) and never
  overwrites, in the jail or the store; a store copy is written to `.partial.<pid>` and renamed;
- hard-links into a jail only from `--stage`, into a fresh jail before Firecracker exists there, and only store files
  named `*.mem` or `*.vmstate`; they stay root-owned, with the jail's group and mode 0440, so the microVM can read but not change them and no other uid can read them (ADR 0010), and
  one store copy serves several restores. Everything else is copied, and out of a jail files are only copied;
- checks a free-space reserve before every copy (the System VM's `MemTotal`, so the data disk keeps room for one more
  full memory file of the largest guest), under one `flock` with the copy itself (step `reserve_refuses_breach`).

Residual risks: a check and the following open are separate steps, so a symlink that appears between them is
followed, but only with uid 20000's rights (other `vesperd` jails, which share that uid, and world-readable files).
Whether Firecracker's seccomp filter lets a compromised Firecracker create symlinks at all is not checked. Share paths are read as root after the
symlink check, so a same-user macOS process that swaps a share file for a symlink at the right moment could make
root read a System VM file into a jail; that process already controls the bridge. Every `vesperd` microVM shares
uid 20000 and the store (ADR 0007 §6, CE-0004).

## `vesperd --backend systemvm` on macOS (ADR 0007, slice 4)

`vesperd --backend systemvm --id <id>` adds one host-visible socket per microVM and no new port, share or
entitlement. It reaches the System VM only through the existing bridge (`run/systemvm/vsock/3.sock`) and vsock port
1024, which the allow-list already lists.

- The socket follows slice 1's rules: owner-only directory and mode 0600, `<socket>.lock` held for the life of the
  process, and refusal to take a socket another process serves. The lock is taken before `LAUNCHAPI`, so a refused
  second `vesperd` launches nothing. The socket is bound only after `READY` and removed when the lease ends.
- Any process that can open the socket controls a real microVM. The socket mode is the only access control, as
  for Firecracker's own API socket.
- `vesperd` parses only the agent's greeting, `OK`, `READY` and `ERR`, each against a fixed form, in lines of at
  most 1 KiB read with a 5 s timeout (30 s for `READY`). Relayed bytes are not parsed. Each relay read fills at
  most 8 KiB, so a compromised System VM that sends without end costs memory for one buffer per connection, not
  more. At most 16 connections are relayed at once; a connection beyond that is closed with no bytes.
- An idle timeout (120 s with no request waiting) and a per-request ceiling (120 s without a reply byte) bound how
  long a stalled peer holds a connection and its agent handler in the System VM. Compat exception CE-0007 records
  what a client sees.
- The microVM lives as long as the lease: SIGTERM or SIGINT closes the lease and waits for the agent's teardown,
  SIGKILL closes it through the kernel, and the agent's close (Firecracker exited, the System VM went away) makes
  `vesperd` remove its socket and exit non-zero (`make systemvm-vesperd-suite`).
- Before `LAUNCHAPI`, `vesperd` asks the agent for `DATA status` and refuses to start unless the data disk is
  `formatted` or `mounted` (slice 5). `vesperd stage` and `vesperd store` check every form on macOS before they
  connect (exit 2) and send one `VESPERD` line; the guest checks every argument again.

## Private microVM sessions (ADR 0006)

The shim connects to each jailed Firecracker `v.sock` inside the System VM.
MicroVM vsock port 1025 accepts bounded session control requests; separate ports carry stdin, stdout and stderr.
The guest writes configs from the trusted shim verbatim and invokes the pinned runc binary.
Workloads share the microVM's namespaces and receive full guest capabilities. The microVM is their isolation boundary.
The terminal receiver uses only guest-local Unix sockets and the existing pinned containerd libraries.
No new host-visible socket, share or entitlement is required.
Pause, snapshot and checkpoint parity must account for runc state before those operations can be claimed.
