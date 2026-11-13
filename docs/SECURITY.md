---
status: verified
owner: cityhunteur
last_reviewed: 2026-11-12
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
  `store-rm` (next section). It cannot launch or connect. Like `MICROVM crash`, the hooks need nothing beyond what
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
`seccomp=unconfined` can open the socket, but its connects to CID 1 and 3 fail because no vsock loopback transport
is loaded; a privileged container that can load kernel modules is not covered.

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
  named `*.mem` or `*.vmstate`; they stay root-owned and mode 0444, so the microVM can read but not change them, and
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
