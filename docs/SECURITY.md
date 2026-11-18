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

Built: the network (M2), the server (M3), the guest's mounts (M4), the shim's mapping and kill path (M5), host-edit
events (M6) and the failure suite (M7). A containerd task on the Vesper shim whose bind mount comes from a macOS share gets it as an NFS mount
from its own microVM's server. Docker containers do not use the shim and still get virtio-fs binds through runc.

- **Mapping (shim):** a bind mount whose source, with symbolic links followed in the System VM, lies under a
  virtio-fs share mount becomes one export (`--unfs-src`), and the guest mounts it at the bind's destination; a
  source that holds another share mount (JDG-1) is refused with an error naming the mount; other bind mounts keep
  today's behaviour (ignored by the microVM path). Binds from several shares are one server's sources, each opened
  beneath its own share mount and keyed by it, so the same relative path in two shares is two exports, and read-only
  nests stay within one share (GH #207, D-047). A source bound both read-write and read-only is one read-write
  export, read-only in the guest at its read-only destinations: two exports of one directory give identical handles,
  and the task holds write access to it anyway (D-049).
- **Single-file binds** (GH #208, D-050): a file source is opened with `openat2` without `O_DIRECTORY`, no symbolic
  link on the way, and must be a regular file (a FIFO or socket would hang the single-threaded server). It is
  attached on an empty file `f`, a fixed name, in `/s/N`, a small read-only tmpfs of its own (D-052), so the guest
  cannot add entries next to it, rename or remove it, or change `/s/N` (`EROFS`); a hard link out of it fails
  (`EXDEV`). A read-only file inside a read-write directory source is nested like a directory. The guest mounts `/s/N`
  at a hidden directory of its session, outside the task's root, and binds `f` onto the destination (made as an
  empty file when missing; a directory there is refused), read-only and `nosuid,nodev` as asked.
- **A file source that macOS replaces** (D-052): the supervisor looks at a file source's path once a second and,
  when another file is there, attaches it in the sandbox in place of the old one and tells the guest, which binds it
  again. This is the one place a source path is used after the server has started, and a task can change what a
  path names: with a read-write directory bind it can rename directories until the path of one of its file binds
  names a file that another bind made read-only. So where the new file is taken from depends on who can reach the
  path.
  - No read-write directory source of this task holds the path: the task cannot change what it names (another task
    or container with a read-write bind over a directory above it can, and the bind then follows; TD-074). The
    file is opened from
    the share mount by the rules of the first open (`openat2` beneath the mount, on it alone, no symbolic link, a
    regular file) and cloned with the bind's attributes.
  - A read-write directory source holds the path: the file is opened beneath that source's export inside the
    sandbox, crossing mounts as an NFS request does, and cloned there. A file under a read-only nest is cloned from
    the nest and stays read-only, whatever the file bind's own mode. The binds suite step
    `renamed_into_place_keeps_read_only` runs the renames; with the file taken from the share mount instead, the
    task wrote the read-only file (`run/D-052/notes.txt`).
  The supervisor is root in the System VM. It holds the sandbox's process directory and mount namespace from the
  start, so a process number that comes to name something else is not followed, and it acts only once the sandbox's
  first process has become the server. It enters the namespace on a thread it then discards, names only `/s/N/f`
  in the export's own read-only tmpfs, and finds everything else beneath an export with no symbolic link. One
  source is attached or nested again at most 20 times at a stretch, then once in 15 s. It writes one line to the
  microVM's vsock socket, as the event fanout does, and reads back at most 64 bytes, of which it keeps one of four
  words.
- **Read-only nests are placed again, but a window stays** (D-052, TD-074): when macOS replaces a read-only file
  that lies inside a read-write source, the kernel drops the server's read-only nest with the old name, and the
  guest's bind with it, as Linux does for any bind whose name is renamed over. The supervisor reads the sandbox's
  mounts every second and places a nest that is gone again, and the guest binds again. In between, about a second
  (up to 15 s when the source's 20 attaches at a stretch are spent), the task can write the new file through the
  outer export, and the binds suite saw one write land. A task that
  renames the new file away in that time keeps it writable, because the path then names nothing to attach. A link
  to it or a descriptor held open no longer helps: once the new file is attached the server refuses by its number
  (D-053). Only a macOS process can open the window; a task cannot rename over a nested file
  (the server refuses it). Docker on Linux drops the read-only bind for good in the same case.
- **What is bound read-only stays read-only under every name** (TD-073, D-053, unfs3 patch 0006). A read-only nest
  is a mount on one name in one export, and a task reached the object under other names: in another letter case on
  a macOS volume that ignores case (the default), by moving its directory into a second read-write bind inside the
  first, or after a server restart that followed its renames. So the server knows each read-only source by its
  device and inode number and refuses (`ROFS`) a request that changes it or anything under it: a write, a change of
  attributes, a create, a removal, a rename of it, onto it or into it, and a new hard link to it or to anything
  under it.
  - The list is `/etc/exports.protect` in the sandbox's read-only root. It names the root of each read-only source
    (`/s/N`, or `/s/N/f` for a file). The server looks at these places before every request that changes
    something, so a file that replaced a read-only file source (D-052) is known from the first such request after
    it is attached. An object once found at such a place stays protected while that server runs, also after the
    task moved it aside and another file took its place.
  - The check runs on every changing request, on the path the handle resolves to, so a descriptor the server holds
    open does not carry a write past it. unfs3 follows symbolic links inside a path, so the check resolves the
    directory part of the path through every link before it compares. For a call that acts on what a link points
    to, it follows the last name too, whether or not the target exists yet. What it cannot resolve it refuses; what
    is not there it leaves to the call, which fails on it. The nests stay in place as a second layer.
  - A list that exists and cannot be read makes the server refuse every change, and it tries again each second.
  - A symbolic link in a read-write bind that points at a protected object cannot have its own owner or times
    changed, nor be hard-linked: unfs3 applies size, mode and times through a link, so the server refuses the
    whole request. `cp -a`, `tar` and `rsync -a` of such a link report a read-only file system.
  - An object bound read-only is read-only through every export of that task, also where a read-write bind reaches
    it by a hard link. This holds for one object bound read-only under one spelling and read-write under another
    on a volume that ignores case: it is read-only through both. (The same path bound both ways is one read-write
    source, as before.)
  - **The number is the object's only while something holds the object.** The System VM gives a file another
    inode number when it has forgotten the file and looks it up again (measured after a cache drop:
    `run/TD-073/notes.txt` 8); it did not give a used number to another file in 300 tries. While a server runs,
    what it serves is held: the sources by the sandbox's mounts, and a replaced read-only file by the supervisor
    (the newest 256). An object that the record of an earlier start names, and that is no longer at a source's
    path, is held by nothing (next item).
  - **Across a restart of the server** the supervisor keeps the numbers in `unfs/ID.prot` (root only). A restart
    opens sources by path again, and the task may have renamed directories since. The new server gets the recorded
    numbers, and the supervisor serves read-only a read-write source whose path, or a directory above it, is a
    recorded object; it does the same when it attaches a file again for a read-write file source. This holds only
    while the System VM has not forgotten the object. After the first server ended, nothing holds an object that
    the task moved away from its source's path, for the rest of the task's life; once the System VM forgets it,
    its number changes and the record no longer names it (TD-076).
  - Not covered:
    - **A new launch of the task.** The record belongs to one launch. A task that renames a read-only directory's
      parent and puts a decoy at the path gets the decoy read-only at its next launch, and reaches the true
      directory through its read-write bind. A bind names a path, and the task can change what the path names.
    - The restart case once the System VM has forgotten an object that the task moved away from its path
      (TD-076). A task can add to the memory pressure that makes the System VM forget.
    - A file in a read-only directory source that has a hard link made on macOS outside that directory. The server
      knows the directory, not each file in it, so the file is writable under that outside name when a read-write
      bind covers the name. A task cannot make such a link itself: the server refuses it.
    - The window of D-052 above (until the new file is attached), and another task or a macOS process, which this
      server does not see.
    - The limits: the supervisor's record keeps the first 150 numbers seen at starts and the newest 50 of replaced
      files, the supervisor holds the newest 256 replaced files, and a server remembers 768 objects found at
      protected places. A task that makes the supervisor attach more files than that (20 at a stretch for each
      read-only file source, then one each 15 s) can push out the oldest; with three such sources the 50 are
      reached in about 20 s.
    - After a restart, a read-write bind that the user put inside a read-only one is not compared with the
      record (it is writable by design). A task that renames directories so that this bind's path lies in another
      read-only directory of the same name gets it read-write (TD-077).
    - Whether the System VM can answer with an old number for a name whose object changed a moment ago was tried
      once and not seen (`run/TD-073/notes.txt` 5); it is not ruled out.
  The binds suite steps `read_only_holds_under_every_name`, `replaced_file_stays_read_only_when_moved` and
  `reattached_under_a_recorded_object_is_read_only` run these ways. With the server of `main` 14 of the 24 changes
  of the first step and 3 of the second went through (`run/TD-073/notes.txt` 9).
- **One inode through two exports** (TD-072, D-049): unfs3 keys its handle cache on the handle's path hashes too
  (patch 0004), so a handle from one export resolves only into that export; a handle whose object was renamed into
  another directory still resolves within its own export. Before, a read through a read-only export made writes
  through a read-write export of the same file fail with `ROFS`, silently for a shell redirection.
- **The server record** (`/run/vesper/microvm/unfs/ID.args`, version 2: `squash`, then `mount` lines each followed
  by its `src` lines) is what `vesper-unfs run --args` starts from on every respawn, what the selfcheck starts from,
  and what the fanout maps host paths with, all through one Go parser (`unfs.ParseArgs`; council SEC-6). A record
  with a line that does not parse is refused whole (COR-9).
- **Guest:** `vesper-console shares mount` probes the server for 3 s, then mounts each share under the task's root,
  resolving the destination with `openat2(RESOLVE_IN_ROOT)`, so a symbolic link in the image cannot place a mount
  outside it, and mounting on the descriptor; a link whose target is missing fails the mount. Shares are synced and
  unmounted, deepest first, before the session reports `EXIT`.
- **Events (M6):** the share notification sink passes each path it replays to `vesper-unfs fanout`, a root process
  that queues it (bounded, coalescing, 10,000 paths per microVM) for each microVM whose server exports it and sends
  `N<TAB>rest` lines to the guest's vsock port 1026 through the jail's `v.sock`. The guest's replay resolves each
  path beneath the task's root with no link leaving it and sets its mtime to its own value (read-only sources get
  no event: no times can be set on a read-only mount). unfs3 keeps and reports whole seconds, so the fanout reads the
  macOS mtime in the System VM and sends it rounded up to the next second (`N:MTIME<TAB>rest`, D-043); the guest sets
  that, so the macOS mtime moves forward by less than a second, never back, and the change comes back once (TD-066). Only the System VM can reach that port; the guest is
  the task's own.
- **Replay identity:** the sink waits for the boot's container mode before choosing its uid, and refuses without it;
  it used to decide at connection time, and a launcher that connected early made the whole session replay as root
  (TD-062's fault). It logs `replay uid=` to syslog.
- **Bounds at session start:** a 3 s probe of the server, then each mount in a child process killed after 5 s, so a
  server that accepts the connection but does not answer fails the `RUN` with an error naming the export.
- **Accepted deviations (D-038):** TD-065, TD-066, TD-067's depth limit, and no host-edit events on read-only binds.
  Fixed by the patched server (D-044): an `O_CREAT` open (`>>`, `fopen(…, "a")`) of an existing file the guest had
  not looked up truncated it (TD-068).
  The host-edit events are best effort: a batch the guest's replay has not finished, and an event offered while
  its microVM's sender stops, are lost (council REL-6).
- **Kill and stop:** a `SIGKILL` to the init process gets 10 s to report `EXIT` (the guest's unmount flushes its
  writes); after that the shim stops the VMM, so a dead server cannot hold a task. The wait starts even when the
  guest refuses the signal, as it does once the workload has exited and the session is stuck in that unmount
  (council REL-1). The server stops when the task stops; `Delete` cleans up again.

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
  microVM (Vesper's patched unfs3 0.10.0 at `/usr/libexec/vesper/unfsd`, built reproducibly from
  `third_party/unfs3`, D-044), in its namespace, before the jailer. `DIR` must be a virtio-fs mount whose name holds no newline (white
  space and backslashes are matched in `/proc/mounts`'s escaped form). The server exports each source as `/s/0`, `/s/1`, ... to `169.254.77.2` only, so no source path reaches
  its exports file. It listens on `169.254.77.1` (TCP 2049 and 2050, through an IPv4-mapped IPv6 socket).
- **Sources** are read once and their attributes fetched afresh before serving, and opened again while the System
  VM still sees a directory macOS deleted and remade under the same name (up to 15 s, TD-071). They are opened by
  `vesper-unfs run` with `openat2` beneath the share mount, with no symbolic link on the way
  and no crossing to another mount, then cloned into detached mount trees (`open_tree`) with their final attributes
  (`nosuid,nodev,noexec`, plus `ro` for a read-only source). Nothing opens a directory source's path again; a file
  source's path is looked at again to attach a file that replaced it (D-052, below). A symbolic link, a
  `..` component or a mount that is not virtio-fs is refused, and the launch fails.
- **Sandbox:** new mount, pid, ipc and uts namespaces; `pivot_root` into a 1 MiB tmpfs that is then read-only, with the
  old root detached. It holds only the exports, a read-only `/usr` and `/lib` (for `unfsd` and its libraries),
  `/dev/null` (read-only: it still opens for writing, but its mode and owner, the System VM's own, cannot change;
  council SEC-1), and `/etc` with the exports file and libtirpc's tables. No `/proc`, `/sys`, `/run` or device. Each
  read-only source is bound again, read-only, inside each read-write source that holds it, at a place found beneath
  that export with no symbolic link on the way (a second layer: the server also refuses by the object's number, D-053). The server runs as root with the bounding, permitted and effective
  sets cut to `CAP_CHOWN`, `CAP_DAC_OVERRIDE`, `CAP_FOWNER`, `CAP_FSETID`, `CAP_SETGID` and `CAP_SETUID` (`0xdb`),
  no ambient capabilities, `no_new_privs`, and a seccomp filter that refuses user namespaces (`unshare` and `clone`
  with `CLONE_NEWUSER`), `setns`, and `clone3` (D-037), and the calls an exploited server could use against the System
  VM and `unfsd` never makes: the keyrings, `io_uring`, `userfaultfd`, `perf_event_open`, `bpf`, `ptrace` and other
  processes' memory, module and kexec loading, and the mount API (council SEC-3). `--unfs-squash` runs it as 65534 with no capabilities instead.
- **Supervision:** `supervise-daemon` (service `vesper-unfs-ID`, respawn after 1 s, at most 30 in 60 s), in
  `/sys/fs/cgroup/vesper-unfs/ID` with `memory.max` 32 MiB, `pids.max` 32 and `cpu.max` one CPU. A server whose
  supervisor gave up is named in HEALTH's `share_servers_down` (TD-070). The memory is admitted with the
  microVM's ceiling as one entry. The log is `/var/log/vesper-unfs-ID.log` (bounded, 256 KiB). `cleanup` stops the
  supervisor, kills what is left in the cgroup and removes it; `DIRTY` names `unfs` or `unfs-cgroup` otherwise.
- **Inspection:** `vesper-microvm unfs ID` (state, listening, memory), `unfs-isolation ID` (ids, capabilities,
  `no_new_privs`, seccomp, namespaces and every mount in the server's root), `unfs-log ID`.
- **A guest could end its own server by the timing of its connections** (TD-075, fixed by unfs3 patch 0005,
  D-054): unfs3 0.10.0 read freed memory when two descriptors were ready in one round of its main loop and one of
  them was a new connection. Only that guest's own shares were at stake, and the server came back, within its 30
  restarts a minute. The supervisor now records each end of a server that it sees in the System VM's log.
- **What remains:** unfs3 follows a symbolic link in the middle of a path, so a guest that makes a link in a
  read-write export can read what the sandbox's root holds: `/usr`, `/lib` and `/etc` (System VM binaries, the
  exports file and libtirpc tables; no secret). The mount view, not unfs3's path checks, is what confines it; the
  probe sets `/dev/null`'s mode through such a link and must be refused (SEC-2). The cgroup bounds memory, pids and
  CPU (one CPU). Firecracker's rate limiter on `share0` (400 MB/s and 20,000 frames/s each way) bounds the requests a
  guest can send, and so its server's virtio-fs I/O; block-layer `io.max` cannot, since no block device is on that path
  (TD-069, D-045). A setuid-root file made by guest root on a share is inert: Apple's virtio-fs drops
  the set-id bit and keeps the Mac user as owner (tested 2026-11-17), and the System VM mounts every share
  `nosuid,nodev` besides (SEC-4). A malformed file handle is answered with STALE; unpatched unfs3 0.10.0 aborted on
  an assertion, so a guest could stop its own shares (TD-065, fixed by D-044). `..` at an export's
  root returns a handle of the sandbox's `/s` with its attributes; every operation on it is refused.
- **A nested read-only source** is bound only on the directory that was opened as that source (same device and
  inode); a directory renamed and replaced on macOS before the bind is placed makes the server refuse to start.
- **Access is the export check, not the handle.** unfs3 resolves a handle by (device, inode) from its cache or by its
  path hashes from `/`, then decides access by the longest export that prefixes the resolved path (`exports_options`,
  a prefix match with no `/` boundary, so `/s/1` also matches `/s/10`). That is harmless while every export belongs to
  the same guest address and the longest match wins; export names must keep it so (council SEC-1, 2026-11-17).
- **Nested macOS shares.** A source is cloned without its submounts, so a share mounted inside another share's source
  is not part of that source: the guest would see and write the outer share's view of that directory, even when the
  inner share is read-only. The planner refuses a source that holds another share mount (council JDG-1, ARC-9).
- **Evidence:** `make systemvm-unfs-suite` (13 steps, `run/GH-160/m3/unfs-report.json`; the two-share step in
  `run/council-open/`).

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
  store is `/var/lib/docker/100000.100000`. `HEALTH` and `make docker-up` report the old store. Only `vesper-data legacy delete --yes` deletes it (owner
  decision D-041; the store on this host was deleted on 2026-11-17): it refuses while the old store has a container,
  while anything is mounted under it outside a remapped store, or during an import, and never touches a remapped
  store (`<digits>.<digits>`); without `--yes` it prints the plan.
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
  (imports from a scratch store under `/run/vesper/legacy-test`), `vesper-data test-delete` (deletes a scratch
  store under `/run/vesper/legacy-delete-test`). All need the agent.
- **Evidence:** `make systemvm-confine-suite` (16 steps, `run/GH-194/m5-confine-report.json`; the delete step in `run/D-041/`; the import step in
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

### MicroVM size (TD-048, D-051)

A task's memory limit and CPU quota or CPU set size its microVM, and the guest's runc sets the limit and the quota on
the workload's cgroup. Those limits inside the guest are for the workload's benefit only. The workload is root in
its guest with every capability, so it can raise them; the measurement run in `run/TD-048/measurements.txt` does exactly
that. The bounds that hold against a hostile workload are outside the
guest:

- the guest's memory and vCPUs, which Firecracker fixes at boot;
- the `memory.max` of the Firecracker process's cgroup in the System VM (the guest plus 96 MiB, plus 1 MiB for each
  128 MiB of guest above 256), which the jailer sets before Firecracker runs;
- for a task with a CPU quota, the `cpu.max` of that cgroup: the quota plus a quarter of a CPU, which is for
  Firecracker's own threads. `make systemvm-microvm-size-suite` lifts the quota inside the guest and spins on both
  vCPUs of a 1.5 CPU task; the microVM used 1.75 CPUs (council SEC-1). A task with a CPU set and no quota is
  bounded by its vCPUs alone;
- the shared memory budget (`vesper-memadmit`, TD-056), which admits that ceiling before the launch and refuses a
  microVM that does not fit.

The size comes from the task's OCI spec, so whoever may start a task on the shim chooses it, up to the System VM's
CPUs and the budget. Today that is root in the System VM and the Mac user through the control channel; dockerd
refuses the runtime. One task can take the whole budget, and later launches, `vesperd`'s too, are then refused by
name (`ERR memory`) until it is deleted; a running task is never squeezed. There is no cap for one task (council
SEC-5); one is needed before a less trusted client reaches this runtime (GH #53). The shim checks the numbers before
they reach the launcher: sums are checked before they can wrap, a CPU set is counted and refused as soon as it
passes 32, a limit above 1 TiB is refused, and a quota's period must be one the kernel takes. `vesper-microvm
launch` accepts only digits with no leading zero for `--mem-mib`, `--vcpus` and `--cpu-quota`, and the same number
feeds the ceiling, the admission and Firecracker's configuration.
