---
status: accepted
owner: cityhunteur
last_reviewed: 2026-11-13
---

# 0009. Host shares into shim microVMs: one `unfs3` server per microVM, inside that microVM's network namespace

Date: 2026-11-13. Supersedes: none. Superseded by: none. Revised twice the same day after two llm-council design
reviews (`docs/exec-plans/active/2026-11-13-GH160-shares-into-microvms.md`, Decision Log) and accepted by the owner
with the second review's fixes (D-032).

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
  the namespace. `vesper-microvm` installs the rule from outside (`ip netns exec`) before the server starts, so the
  server's sandbox needs no `iptables` binary.
- **Helpers:** `netns_up` and `netns_down` move from `vesper-vesperd` into a sourced file,
  `/usr/local/lib/vesper/netns.sh`, that takes the TAP owner's uid and a state directory. The shim's namespaces live
  under `/run/vesper/microvm/netns/`, which the `vesperd` reaper never scans, with TAPs owned by the shim's jail uid
  (10000), not `vesperd-jail` (20000). The two backends may reuse ids, so they share no directory.

### Server

- `unfsd -d -p -t -l 169.254.77.1 -n 2049 -m 2050 -e <exports>`: in the foreground, without portmap, over TCP only
  and on the server address alone. The server writes no pid file: it runs after `pivot_root` in its own pid
  namespace, where a pid means nothing outside. The supervisor's pid file and the server's `cgroup.procs` identify
  it. Its output goes through `vesper-boundlog` (TD-055) to a log outside the server's root.
- **Exports:** the exports file names generated paths, `/s/0`, `/s/1` and so on, never a user path, so a source with
  spaces or shell characters never enters the file. Each line is `/s/N 169.254.77.2(rw|ro,no_root_squash)`.
- **Supervision:** `supervise-daemon` runs each server as `vesper-unfs-<id>` with `--respawn-delay 1` and a bounded
  `--respawn-max` in its period. It runs in its own memory-capped cgroup beside the microVM's; M3 sizes the cap,
  and `vesper-memadmit` (TD-056) admits the microVM's ceiling plus that cap as one entry, so a launch that the two
  together would take over the budget is refused. `vesper-microvm status` reports the server's state, its respawn
  count and its cgroup's OOM kills.

### Sandbox

A chroot alone does not contain a root process: a root `unfsd` that a client exploits can leave a plain chroot. The
server therefore runs:

- in new mount, pid, ipc and uts namespaces, with `pivot_root` into an empty tmpfs and the old root unmounted;
- with its sources bind-mounted at `/s/N` with `nosuid,nodev`, and remounted `ro` at the kernel when the OCI mount is
  read-only, so read-only does not rest on the exports file;
- with every read-only source bound again, read-only, at its place inside each read-write source that contains
  it. Without this, `-v $PWD:/app -v $PWD/conf:/app/conf:ro` would leave `conf` writable through `/s/0/conf`: the
  workload is root in its guest and can mount `/s/0` itself. With it, no path the server can reach writes a source
  the task mounted read-only. M3 tests this case and confirms `unfs3`'s handles still resolve across the inner mount;
- with `no_new_privs` and a capability bounding set of only what NFS service needs to act as each caller
  (`CAP_SETUID`, `CAP_SETGID`, `CAP_CHOWN`, `CAP_DAC_OVERRIDE`, `CAP_FOWNER`, `CAP_FSETID`); without
  `CAP_SYS_CHROOT` and `CAP_SYS_ADMIN`, the nested-chroot escape and new mounts fail.

**Privilege model.** The preferred server is not root: `all_squash` with a fixed unprivileged uid and no
capabilities, because writes reach macOS as the sharing user whatever the guest uid, and a server with no
capabilities has little to escape with. It is adopted only if M3 shows it works: every virtio-fs file appears as
`0:0` in the System VM (`scripts/systemvm/binds-suite.sh`), so an unprivileged server may get `EACCES` on `0600`
files and `0700` directories, and guest `chown` would fail. If M3 shows either breaks the binds suite, the server
stays root with the capability set above. BusyBox `setpriv` cannot change uid; M1 names the tool that does, with
the dependency closure.

**Source binding.** A source is checked once and bound later, and in between another task or a macOS process could
swap a directory for a symlink to somewhere outside the share. So the sandbox setup opens each source relative to a
descriptor of the trusted share mount, refusing symlinks on the way, checks that the opened object is a directory on
that mount, and binds from the descriptor it holds (`/proc/self/fd/N`). It never opens the path a second time.

M3 proves the sandbox with an escape test. Until M3 passes, the blast-radius statement below is a goal, not a fact.

### Lifecycle

- **Record and lock:** `vesper-microvm launch` writes a per-id record (with its own pid and start time) under one
  lock before it creates the namespace, then starts the server, then the jailer. The lock covers the record and the
  namespace only, not the boot poll, and its descriptor is closed in every child. `launch` has an error trap that
  undoes what it made.
- **Stop:** the server stops when the task reaches `PhaseStopped`, and `vesper-microvm cleanup` stops it again
  idempotently.
- **Kill:** `Kill` forwards the signal to the guest, as today. The process exits, its files close and NFS flushes
  them; the guest unmounts its shares before it reports `EXIT`. A `SIGKILL` to the init process also starts a
  bounded wait (5 s; M5 settles the number) for that `EXIT`, outside the task's operation lock. Only if it runs out,
  which is the case of a process stuck in a `hard` NFS call to a dead server, does the shim stop the Firecracker
  VMM (`vesper-microvm stop`). `Wait` then returns and `Delete` can run `Cleanup`. So `docker kill` and `docker rm -f`
  keep the writes the guest had buffered whenever the server is alive, and a task stop never waits on a dead
  server for more than the bound. The shim adds no timer to `SIGTERM`: dockerd sends its own `SIGKILL` when a
  stop's grace period ends.
- **Reaper:** a new `vesper-microvm reap` pass, run every 10 s by a new OpenRC service that M2 adds
  (`vesper-microvm-reaper`), takes the same lock and runs `cleanup` for any id whose record is past its grace period
  and whose `launch` and Firecracker are both gone, so the jail and cgroup go with the server and namespace.
- **Sensors:** `cleanup`'s `DIRTY` check, `status`, `isolation` and `vesper-ctr leftovers` also check the server and
  supervisor pids, the namespace mount and the exports file.

### Guest

- The initramfs gains the NFS client modules and their dependencies, taken from `modules.dep` (the prototype needed
  `netfs`; `nfs_acl` and others are Unknown until M4), and `virtio_net` with its dependencies.
- A `vesper_share_net=169.254.77.2/30` kernel parameter brings up `eth0` with no gateway and no default route. It is
  separate from `vesper_net=`, which configures the egress tests' uplink.
- The share list (guest path, export, read-only flag) travels as JSON inside the session's `RUN` request, not on the
  kernel command line. The init has handed over to the session by then and has no JSON parser, so a `vesper-console`
  subcommand does the mounting: it decodes the list in Go and calls `mount(2)` itself, so no guest path reaches a
  shell. It mounts under the task's root before the workload starts, shallowest destination first, with
  `addr=169.254.77.1,vers=3,proto=tcp,port=2049,mountport=2050,mountproto=tcp,nolock,hard,actimeo=1,lookupcache=positive`,
  plus `ro` for a read-only mount.
- Before the first `hard` mount it probes the server's port with a 3 s timeout and fails the `RUN` with an error
  naming the share if there is no answer, so a stalled server cannot hang session start until its 30 s deadline.

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
- **`hard` mounts.** Guest I/O waits while the server restarts instead of failing with an I/O error the workload
  may ignore. A server that stays down hangs the task's I/O until the task is killed; the kill then stops the VMM
  after its bounded wait (see Lifecycle), and writes the guest had buffered for that server are lost.
- **An unexplained lost write.** In the GH #159 prototype a line written with `>` and then appended to with `>>` once
  went missing, and nobody explained it. Reproducing or ruling out that case is an exit gate for M3.
- **Path-based file handles.** `unfs3` derives handles from paths, so very deep trees and a rename across
  directories under an open file may fail. M7 tests both and records each failure as a deviation.
- **Restarts may cost `ESTALE`.** If `unfs3`'s file handles do not survive a restart, open files on the guest see
  `ESTALE`. M3 measures it and records it as a deviation if so.
- **The deviations D-020 measured stand:** `stat` can lag up to 1 s, and I/O is 3 to 11 times slower than virtio-fs.
- **Host-edit events (GH #150)** need one more hop. The System VM replays a macOS edit as an inotify event, and that
  event must then reach the guest as a local `touch` on the NFS mount. That hop runs over the microVM's vsock device,
  not the share network, and is designed in M6, with a bounded, coalescing queue per microVM and a check that the
  guest's `touch` does not echo back to macOS as a new edit.
- **Future networking:** a later uplink for shim microVMs (TD-032, GH #53) joins the same namespace. The in-namespace
  `INPUT` rule keeps the server reachable from the TAP device alone, and the M2 test that only the TAP device and
  `lo` exist becomes a test of that rule.

## Implementation and migration impact

The milestones are in `docs/exec-plans/active/2026-11-13-GH160-shares-into-microvms.md`:

1. `unfs3` pinned in the package lock and in `THIRD_PARTY_NOTICES.md` (BSD-3-Clause), with its whole dependency
   closure (for example `libtirpc`), each licence, and the tool that drops the server's uid. Anything in that
   closure beyond `unfs3` is a new runtime dependency and goes to the owner in M1's PR.
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
