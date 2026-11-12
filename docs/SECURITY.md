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

- `LAUNCHAPI <id> [ceiling]` starts one stock Firecracker under the jailer with no configuration file: uid and gid
  20000 (the shim's microVMs run as 10000), cgroup `/sys/fs/cgroup/vesperd/<id>` (`--parent-cgroup vesperd`,
  `pids.max=256`, `memory.max` from `ceiling`, default 640 MiB, at least 128 MiB and at most the System VM's
  memory), chroot `/srv/vesperd/firecracker/<id>/root` (tmpfs until slice 5), the prepared kernel and initramfs as
  `/Image` and `/initrd.gz`. After `READY` the connection is the lease: its end stops the microVM and removes the
  jail, cgroup and record; Firecracker's exit closes it.
- `FCAPI <id>` relays bytes to that microVM's API socket. The relay socat chroots into the jail root before it
  connects (`chroot-early`), so a symlink that a compromised Firecracker plants in its jail, which it owns, cannot
  lead the root-run socat to a socket outside the jail. It adds nothing `FCPARITY api` could not already do.
- `VESPERD <args…>` inspects and faults these microVMs for the suite: `status`, `isolation`, `list`, `log`, `reap`,
  `kill-holder`, `kill-fc` and the test hooks `test-forge-start`, `test-orphan`, `test-age` and
  `test-container-vsock`. It cannot launch or connect. Like `MICROVM crash`, the hooks need nothing beyond what
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

## Private microVM sessions (ADR 0006)

The shim connects to each jailed Firecracker `v.sock` inside the System VM.
MicroVM vsock port 1025 accepts bounded session control requests; separate ports carry stdin, stdout and stderr.
The guest writes configs from the trusted shim verbatim and invokes the pinned runc binary.
Workloads share the microVM's namespaces and receive full guest capabilities. The microVM is their isolation boundary.
The terminal receiver uses only guest-local Unix sockets and the existing pinned containerd libraries.
No new host-visible socket, share or entitlement is required.
Pause, snapshot and checkpoint parity must account for runc state before those operations can be claimed.
