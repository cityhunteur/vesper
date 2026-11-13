---
status: proposed
owner: cityhunteur
last_reviewed: 2026-11-13
---

# 0009. Host shares into shim microVMs: one `unfs3` server per microVM, inside that microVM's network namespace

Date: 2026-11-13. Supersedes: none. Superseded by: none. Revised the same day after an llm-council design review
(`docs/exec-plans/active/2026-11-13-GH160-shares-into-microvms.md`, Decision Log).

## Context and problem

Docker bind mounts from macOS work today because containers run on runc in the System VM, where each macOS
directory is a virtio-fs mount. A task that the shim (`containerd-shim-vesper-v2`) runs as a Firecracker microVM has
no virtio-fs device, so `docker run --runtime io.containerd.vesper.v2 -v /Users/me/src:/src …` has no way to give
the task `/src` (GH #160, TD-049).

The GH #159 prototype found one path that works: NFSv3 from the userspace server `unfs3` in the System VM, over a
TAP device, mounted in the microVM with `actimeo=1,lookupcache=positive` (D-020). The kernel `nfsd` is ruled out: it
returns stale file handles for every new directory over virtio-fs. The prototype also found the exposure. `unfs3`
listened on every System VM address, and a container on the Docker bridge could connect to it. With NFSv3
`AUTH_SYS` and `no_root_squash`, that gives every container read-write access to every share. The prototype ran
`unfsd` as root with no sandbox.

The owner approved `unfs3` as a runtime dependency on three conditions (D-021):

1. the server binds only to microVM-facing addresses;
2. there is one export per microVM address;
3. a test proves that a Docker container cannot reach it.

This ADR decides how those conditions hold. Binding one address is not enough on its own. A container's packets to
any System VM address are delivered locally whatever address the server binds (`unfs3 -l` restricts the socket, not
the route). A design with one server in the root namespace would therefore depend on `INPUT` filter rules, which
every dockerd restart and every future rule change could break. This is the failure class ADR 0008 had to defend in
two layers.

Who can attack the server. This table is derived from the threat model in `design-v0.md` for this design; M7
updates `design-v0.md` and `docs/SECURITY.md`, which still say shim microVMs have no network device.

| Actor | Must not |
|---|---|
| Hostile container (runc, Docker bridge) | connect to any share server |
| Hostile microVM workload | reach another microVM's server, or any path outside its own bind-mount sources, or write to a source mounted read-only |
| Same-user process on macOS | gains nothing new: it already reads the shared directories directly |

## Decision

We will run one `unfsd` per shim microVM that has shares, inside a network namespace that belongs to that microVM
alone, and inside a sandbox that does not rely on `unfs3`'s own checks.

### Network

- The namespace holds `lo` and one TAP device, and nothing else: no veth, no route out. The jailer joins it with
  `--netns`, the pattern CE-0010 set for `vesperd` microVMs.
- **Addresses:** every namespace uses the same fixed link-local pair, `169.254.77.1/30` for the server and
  `169.254.77.2` for the guest. No allocator is needed, because no two namespaces can see each other.
- **Defence in depth:** an `INPUT` rule inside the namespace accepts TCP 2049 and 2050 only from the TAP device and
  drops the rest. It does not carry condition 3; it keeps the server private if a later uplink (TD-032, GH #53) joins
  the namespace.
- **Helpers:** `netns_up` and `netns_down` move from `vesper-vesperd` into a sourced file,
  `/usr/local/lib/vesper/netns.sh`, that takes the TAP owner's uid and a state directory. The shim's namespaces live
  under `/run/vesper/microvm/netns/`, which the `vesperd` reaper never scans, with TAPs owned by the shim's jail uid
  (10000), not `vesperd-jail` (20000). The two backends may reuse ids, so they share no directory.

### Server

- `unfsd -d -p -t -l 169.254.77.1 -n 2049 -m 2050 -e <exports> -i <pidfile>`: in the foreground, without portmap,
  over TCP only and on the server address alone. The pid file and the bounded log are outside the server's root.
- **Exports:** the exports file names generated paths, `/s/0`, `/s/1` and so on, never a user path, so a source with
  spaces or shell characters never enters the file. Each line is `/s/N 169.254.77.2(rw|ro,no_root_squash)`.
- **Supervision:** `supervise-daemon` runs each server as `vesper-unfs-<id>` with `--respawn-delay 1` and a bounded
  `--respawn-max` in its period. It runs in its own memory-capped cgroup beside the microVM's; M3 sizes the cap,
  and the W4 budget (`vesper-memadmit`) reserves it with the microVM's ceiling.

### Sandbox

A chroot alone does not contain a root process: a root `unfsd` that a client exploits can leave a plain chroot. The
server therefore runs:

- in new mount, pid, ipc and uts namespaces, with `pivot_root` into an empty tmpfs and the old root unmounted;
- with its sources bind-mounted at `/s/N` with `nosuid,nodev`, and remounted `ro` at the kernel when the OCI mount is
  read-only, so read-only does not rest on the exports file;
- with `no_new_privs` and a capability bounding set of only what NFS service needs to act as each caller
  (`CAP_SETUID`, `CAP_SETGID`, `CAP_CHOWN`, `CAP_DAC_OVERRIDE`, `CAP_FOWNER`, `CAP_FSETID`); without
  `CAP_SYS_CHROOT` and `CAP_SYS_ADMIN`, the nested-chroot escape and new mounts fail.

M3 proves this with an escape test, and M3 also evaluates `all_squash` with the sharing user's uid, which would let
the server run without root. Until M3 passes, the blast-radius statement below is a goal, not a fact.

### Lifecycle

- **Record and lock:** `vesper-microvm launch` writes a per-id record under one lock before it creates the namespace,
  then starts the server, then the jailer. `launch` has an error trap that undoes what it made.
- **Stop:** the server stops when the task reaches `PhaseStopped`, and `vesper-microvm cleanup` stops it again
  idempotently.
- **Dead server:** a guest process blocked in a `hard` NFS call cannot exit, even on `SIGKILL`. So the shim's `Kill`
  of the init process with `SIGKILL`, or a `SIGTERM` whose grace period runs out, stops the Firecracker VMM
  (`vesper-microvm stop`). `Wait` then returns and `Delete` can run `Cleanup`. A task stop never waits on a server.
- **Reaper:** a new `vesper-microvm reap` pass, run by the shim's System VM service every 10 s, takes the same lock and
  removes the server, namespace and record of any id whose record is past its grace period and whose Firecracker
  is gone.
- **Sensors:** `cleanup`'s `DIRTY` check, `status`, `isolation` and `vesper-ctr leftovers` also check the server and
  supervisor pids, the namespace mount and the exports file.

### Guest

- The initramfs gains the NFS client modules and their dependencies, taken from `modules.dep` (the prototype needed
  `netfs`; `nfs_acl` and others are Unknown until M4), and `virtio_net` with its dependencies.
- A `vesper_share_net=169.254.77.2/30` kernel parameter brings up `eth0` with no gateway and no default route. It is
  separate from `vesper_net=`, which configures the egress tests' uplink.
- The share list (guest path, export, read-only flag) travels as JSON over the existing vsock guest session, not on
  the kernel command line. The init mounts each with
  `vers=3,proto=tcp,port=2049,mountport=2050,mountproto=tcp,nolock,hard,actimeo=1,lookupcache=positive`, plus `ro`
  for a read-only mount.

### Shim

An OCI bind mount whose resolved source (symlinks followed, checked against the mount table) is under a virtio-fs
share mount becomes one of these NFS mounts. A source that is a file, not a directory, is refused with an error
naming the mount, until a later change supports it. Any other bind mount keeps today's behaviour.

Nothing in the root namespace listens for NFS, so no rule there has to hold for condition 3.

## Alternatives considered

- **One `unfsd` in the root namespace, bound to a dedicated address, with `INPUT` rules by interface.** One process
  and one exports file. But isolation then rests on filter rules that a dockerd restart or a later rule change can
  remove. The exports would have to list every microVM, and every microVM's address would reach the same process,
  so a bug in `unfs3`'s export checks would expose other microVMs' shares. Rejected.
- **One `unfsd` per microVM in the root namespace, each bound to that microVM's own address.** Separate processes,
  but containers can still route to every address. Rejected for the same reason as the option above.
- **A vsock file service (a FUSE client in the microVM, a server in the System VM).** Needs no network device and no
  listener that containers can reach, and could invalidate caches exactly. It would cost two to three weeks of new
  code (D-020). It stays the fallback if M3's sandbox cannot be made to hold or the NFS deviations prove too large.
- **9p over a vsock file descriptor (`CONFIG_NET_9P_FD`).** `design-v0.md` excludes 9p as a GA share transport.
- **A block image of each share.** A snapshot: writes never reach macOS and macOS edits never reach the task.
  Rejected by the prototype.

## Rationale and consequences

- **Condition 1** holds by construction: each server binds the only non-loopback address in a namespace that
  contains one microVM's TAP device.
- **Condition 2** holds literally: one server per microVM, whose exports name that microVM's address alone.
- **Condition 3** is shown by a test (M3), but it does not depend on a rule. No container's packets can reach the
  namespace.
- **Blast radius, once M3 passes:** a hostile workload that defeats `unfs3` reaches its own bind-mount sources, as
  root on those paths, and not a read-only source for writing. Those are the paths runc would have given it. Whether
  a forged file handle can cross from one of its exports to another on the same server is an M3 test; both belong to
  the same task, so it would not widen the blast radius beyond `ro`, which the kernel enforces.

Costs and constraints:

- **One process per microVM with shares.** Its memory is **Unknown**: the prototype did not measure it. M3 measures
  it and sets the cgroup cap from it.
- **The sandbox is unproven.** `unfsd` may need files such as `/etc/passwd` or `/dev/null` in its root, or more
  capabilities. M3 finds out, and the sandbox gains only what `unfsd` is shown to need. If it needs
  `CAP_SYS_ADMIN` or `CAP_SYS_CHROOT`, the design goes back to the owner.
- **No locks across the boundary.** `nolock` means `flock` and `fcntl` locks are local to the microVM; a lock taken
  on macOS or in another microVM is not seen. This is a deviation, recorded in the binds-suite variant.
- **`hard` mounts.** Guest I/O waits while the server restarts instead of failing, and it never corrupts silently.
  A server that stays down hangs the task's I/O until the task is killed, which stops the VMM (see Lifecycle).
- **Restarts may cost `ESTALE`.** If `unfs3`'s file handles do not survive a restart, open files on the guest see
  `ESTALE`. M3 measures it and records it as a deviation if so.
- **The deviations D-020 measured stand:** `stat` can lag up to 1 s, and I/O is 3 to 11 times slower than virtio-fs.
- **Host-edit events (GH #150)** need one more hop. The System VM replays a macOS edit as an inotify event, and that
  event must then reach the guest as a local `touch` on the NFS mount. That hop runs over the microVM's vsock device,
  not the share network, and is designed in M6.
- **Future networking:** a later uplink for shim microVMs (TD-032, GH #53) joins the same namespace. The in-namespace
  `INPUT` rule keeps the server reachable from the TAP device alone, and the M2 test that only the TAP device and
  `lo` exist becomes a test of that rule.

## Implementation and migration impact

The milestones are in `docs/exec-plans/active/2026-11-13-GH160-shares-into-microvms.md`:

1. `unfs3` pinned in the package lock and in `THIRD_PARTY_NOTICES.md` (BSD-3-Clause).
2. The shared namespace helpers and a per-microVM namespace in `vesper-microvm`, with record, lock and reaper.
3. Exports, the sandbox and supervision, with the isolation and escape tests.
4. The guest client.
5. The shim mapping and the kill path.
6. The event hop.
7. The binds-suite variant, and updates to `design-v0.md`, `docs/SECURITY.md`, TD-032, TD-049 and the decision log.

Existing tasks are unaffected: a task without share-backed bind mounts gets no namespace, no server and no network
device. Rollback is the removal of the shim mapping, after which shares again reach runc containers only.

## References

- D-020 and D-021 in `docs/design-docs/decision-log.md`; GH #159 (prototype), GH #160 (this work), GH #150 (event
  replay), GH #53 and TD-032 (microVM networking), TD-049.
- `docs/exec-plans/completed/2026-11-11-GH159-share-into-microvm-prototype.md` (evidence under `run/GH-159/`).
- ADR 0008 (egress for `vesperd` microVMs) and CE-0010 (a network namespace per `vesperd` microVM).
- `systemvm/overlay/usr/local/bin/vesper-microvm`, `vesper-vesperd` (`netns_up`, `netns_down`, `reap_pass`), `shim/`.
