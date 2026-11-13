---
status: proposed
owner: cityhunteur
last_reviewed: 2026-11-13
---

# 0009. Host shares into shim microVMs: one `unfs3` server per microVM, inside that microVM's network namespace

Date: 2026-11-13. Supersedes: none. Superseded by: none.

## Context and problem

Docker bind mounts from macOS work today because containers run on runc in the System VM, where each macOS
directory is a virtio-fs mount. A task that the shim (`containerd-shim-vesper-v2`) runs as a Firecracker microVM has
no virtio-fs device, so `docker run --runtime io.containerd.vesper.v2 -v /Users/me/src:/src …` has no way to give
the task `/src` (GH #160, TD-049).

The GH #159 prototype found one path that works: NFSv3 from the userspace server `unfs3` in the System VM, over a
TAP device, mounted in the microVM with `actimeo=1,lookupcache=positive` (D-020). The kernel `nfsd` is ruled out: it
returns stale file handles for every new directory over virtio-fs. The prototype also found the exposure. `unfs3`
listened on every System VM address, and a container on the Docker bridge could connect to it. With NFSv3
`AUTH_SYS` and `no_root_squash`, that gives every container read-write access to every share.

The owner approved `unfs3` as a runtime dependency on three conditions (D-021):

1. the server binds only to microVM-facing addresses;
2. there is one export per microVM address;
3. a test proves that a Docker container cannot reach it.

This ADR decides how those conditions hold. Binding one address is not enough on its own. A container's packets to
any System VM address are delivered locally whatever address the server binds (`unfs3 -l` restricts the socket, not
the route). A design with one server in the root namespace would therefore depend on `INPUT` filter rules, which
every dockerd restart and every future rule change could break. This is the failure class ADR 0008 had to defend in
two layers.

Who can attack the server, from the threat table in `design-v0.md`:

| Actor | Must not |
|---|---|
| Hostile container (runc, Docker bridge) | connect to any share server |
| Hostile microVM workload | reach another microVM's server, or any path outside its own bind-mount sources |
| Same-user process on macOS | gains nothing new: it already reads the shared directories directly |

## Decision

We will run one `unfsd` per shim microVM that has shares, inside a network namespace that belongs to that microVM
alone. The namespace holds `lo` and one TAP device, and nothing else: no veth, no route out. The jailer joins it
with `--netns`, the pattern CE-0010 set for `vesperd` microVMs.

- **Addresses:** every namespace uses the same fixed link-local pair, `169.254.77.1/30` for the server and
  `169.254.77.2` for the guest. No allocator is needed, because no two namespaces can see each other.
- **Server:** `unfsd -d -p -t -l 169.254.77.1 -n 2049 -m 2050 -e <exports> -i <pidfile>`, which runs in the
  foreground, without portmap, over TCP only and on the server address alone.
- **Exports:** the exports file names only that task's bind-mount sources. Each line is
  `<source> 169.254.77.2(rw|ro,no_root_squash)`, `ro` when the OCI mount is read-only.
- **Server sandbox:** `unfsd` runs in a private mount namespace in which only those sources are visible: they are
  bind-mounted into an empty tmpfs root, and `unfsd` is chrooted into it. A file handle or `..` that escapes an export
  then finds nothing to serve.
- **Supervision:** `supervise-daemon` runs each server under its own name, `vesper-unfs-<id>`, and restarts it.
  `vesper-microvm launch` starts it and `vesper-microvm cleanup` stops it. The reaper stops any server whose microVM
  is gone and removes its namespace.
- **Guest:** the initramfs gains the NFS client modules (`nfs`, `nfsv3`, `sunrpc`, `lockd`, `grace`). The init brings
  up `eth0` from `ip=` on the kernel command line and mounts each share with
  `vers=3,proto=tcp,port=2049,mountport=2050,mountproto=tcp,nolock,hard,actimeo=1,lookupcache=positive`.
- **Shim:** an OCI bind mount whose source is under a virtio-fs share mount becomes one of these NFS mounts. Any
  other bind mount keeps today's behaviour.

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
  code (D-020). It stays the fallback if the NFS deviations prove too large.
- **9p over a vsock file descriptor (`CONFIG_NET_9P_FD`).** `design-v0.md` excludes 9p as a GA share transport.
- **A block image of each share.** A snapshot: writes never reach macOS and macOS edits never reach the task.
  Rejected by the prototype.

## Rationale and consequences

- **Condition 1** holds by construction: each server binds the only non-loopback address in a namespace that
  contains one microVM's TAP device.
- **Condition 2** holds literally: one server per microVM, whose exports name that microVM's address alone.
- **Condition 3** is shown by a test (M3), but it does not depend on a rule. No container's packets can reach the
  namespace.
- **Blast radius:** a hostile workload that defeats `unfs3` reaches its own bind-mount sources, as root on those
  paths. Those are the same paths runc would have given it. The chroot keeps everything else out of the server's
  view.

Costs and constraints:

- **One process per microVM with shares.** Its memory is **Unknown**: the prototype did not measure it. M3 measures
  it, and the W4 memory budget (`vesper-memadmit`) reserves that much beside the microVM's ceiling.
- **The chroot is unproven.** `unfsd` may need files such as `/etc/passwd` or `/dev/null` inside its root. M3 finds
  out, and the sandbox gains only what `unfsd` is shown to need.
- **No locks across the boundary.** `nolock` means `flock` and `fcntl` locks are local to the microVM; a lock taken
  on macOS or in another microVM is not seen. This is a deviation, recorded in the binds-suite variant.
- **`hard` mounts.** Guest I/O waits while the server restarts instead of failing, and it never corrupts silently.
  A server that stays down hangs the task's I/O until the task is stopped. Cleanup kills Firecracker before it stops
  the server, so a task stop never waits on a dead server.
- **The deviations D-020 measured stand:** `stat` can lag up to 1 s, and I/O is 3 to 11 times slower than virtio-fs.
- **Host-edit events (GH #150)** need one more hop. The System VM replays a macOS edit as an inotify event, and that
  event must then reach the guest as a local `touch` on the NFS mount. That hop runs over the microVM's vsock device,
  not the share network, and is designed in M6.
- **Future networking:** a later uplink for shim microVMs (TD-032, GH #53) joins the same namespace. It must not
  route to `169.254.77.0/30` from outside. The M2 test that only the TAP device and `lo` exist becomes a test that
  the server address is reachable from the TAP device alone.

## Implementation and migration impact

The milestones are in `docs/exec-plans/active/2026-11-13-GH160-shares-into-microvms.md`:

1. `unfs3` pinned in the package lock and in `THIRD_PARTY_NOTICES.md` (BSD-3-Clause).
2. A per-microVM namespace in `vesper-microvm`, reusing `netns_up` and `netns_down` from `vesper-vesperd`.
3. Exports, the chroot sandbox and supervision, with the isolation tests.
4. The guest client.
5. The shim mapping.
6. The event hop.
7. The binds-suite variant.

Existing tasks are unaffected: a task without share-backed bind mounts gets no namespace, no server and no network
device. Rollback is the removal of the shim mapping, after which shares again reach runc containers only.

## References

- D-020 and D-021 in `docs/design-docs/decision-log.md`; GH #159 (prototype), GH #160 (this work), GH #150 (event
  replay), GH #53 and TD-032 (microVM networking), TD-049.
- `docs/exec-plans/completed/2026-11-11-GH159-share-into-microvm-prototype.md` (evidence under `run/GH-159/`).
- ADR 0008 (egress for `vesperd` microVMs) and CE-0010 (a network namespace per `vesperd` microVM).
- `systemvm/overlay/usr/local/bin/vesper-microvm`, `vesper-vesperd` (`netns_up`, `netns_down`), `shim/`.
