---
status: proposed
owner: cityhunteur
last_reviewed: 2026-11-11
---

# 0007. Connect `vesperd` to Backend A as a relay to one jailed Firecracker per socket

Date: 2026-11-11. Supersedes: none. Superseded by: none.

## Context and problem

`vesperd` answers the 38 pinned routes from an in-memory state machine (`AppState`, [src/api.rs](../../../src/api.rs)).
It drives no microVM (TD-045). Its validation opens kernel, initrd, drive and pmem paths on macOS, and its snapshot
routes are stubs. By default it binds `/tmp/vesperd.sock` with mode 0660 (TD-050). Each error it returns is an
`ErrorEnvelope` (invariant 3 in [AGENTS.md](../../../AGENTS.md)).

The design requires `vesperd` to drive stock Firecracker in the System VM for Backend A
([design-v0.md](../../specs/design-v0.md), Backend ownership contract, Phase 2). D-008 and D-011 make this step 1
of Next work.

Today:

- `vesper-microvm` boots every microVM from `--config-file` as uid 10000 under `/srv/jailer`, and finds it with
  `pgrep -f "firecracker --id ID "`. The shim calls it with the containerd task id as the jail id (D-013).
- The launcher's bridge (`handleBridgeClient` in [main.swift](../../../launcher/Sources/vesper-vmlauncher/main.swift))
  connects a `CONNECT <port>` client to any System VM vsock port (TD-027) and passes a half-close on. Firecracker's
  API is a Unix socket inside each jail.
- The agent on port 1024 runs one shell per connection, under `socat … fork`. Its `MICROVM` command buffers all
  output until the command exits. Its `FCPARITY api ID` command connects to a jail's API socket and does not check
  `ID`.
- Jails are in tmpfs; `/var/lib/vesper/state` is bind-mounted onto the data disk (GH #164).
- `proxy::run_server` ([src/proxy.rs](../../../src/proxy.rs)) already relays a 0600 socket through the bridge.

GH #156 lists eight questions. D-017 asks for a pass-through relay to be weighed. D-018 makes TD-027 a gate.

## Decision

We will make `vesperd --backend systemvm` a byte relay to one jailed Firecracker v1.16.0. There is one `vesperd`
process and one owner-only socket on macOS per microVM. `vesperd` starts that microVM unconfigured, and the microVM
ends when `vesperd` ends.

Every HTTP response on that socket comes from Firecracker, unchanged. When `vesperd` itself fails, it refuses or
closes the connection and sends no body, as an unreachable Firecracker would.

`--backend model` keeps the in-memory state machine and the contract tests. It stays the default until the relay
is proven.

Items marked **Owner** need the owner's confirmation before they are built.

### 1. Path and artifact namespace

Every path in a request is a path inside the jail, as Firecracker sees it, and `vesperd` never rewrites one. The
jailer runs `pivot_root` into the jail and then `chdir("/")` (`src/jailer/src/chroot.rs`). After that, the paths a
request names resolve inside the jail: kernel, initrd, drives, pmem, snapshot, memory file, logger, metrics and
vsock `uds_path`. The jailer documentation makes the operator link or copy resources into the jail. Network
interfaces are out of scope, because microVMs have no network device yet (TD-032).

Clients stage files through `vesperd` itself, and no new socket is needed for it:

- `vesperd --stage SRC:DEST` stages a file at startup. The flag can be repeated.
- `vesperd stage --id ID SRC DEST` stages a file while the microVM runs.

Both send the agent's `MICROVM stage`. `SRC` is one of:

- a macOS path inside a share named in the launch configuration (the System VM mounts each share at its macOS
  path);
- `store:<name>`;
- `jail:<path>` in this microVM's own jail.

`DEST` is a jail path or `store:<name>`. `LAUNCHAPI` stages the prepared kernel and initramfs as `/Image` and
`/initrd.gz`.

The store is shared by every `vesperd` microVM of the macOS user, which is one trust domain. `vesperd store
list|rm` lists and removes its files. Each copy into the store first checks a free-space reserve on the data disk.
A tech-debt row for store growth is opened on acceptance. **Owner**: the staging interface and the shared store
(item 2).

A compromised microVM controls the files in its jail, so `stage` follows these rules:

- Root never opens a path inside a jail once Firecracker has run there. Reads from and writes into such a jail run as
  uid 20000. Root opens only store-side files that it created, and pipes bytes to or from the uid 20000 process.
- Hard links into a jail are made only by `--stage`, before `LAUNCHAPI` starts Firecracker in that fresh jail. Only
  the store's snapshot memory and vmstate files are linked, and they stay owned by root and read-only. Every other
  file is copied.
- Out of a jail that Firecracker has run in, files are only ever copied, never linked.
- As defence in depth, `SRC` must be under the fixed roots. `stage` also refuses a symlink in any component of
  `SRC` or `DEST`, and any file that is not regular. `DEST` must stay under the jail root or the store. Because
  copies into a jail are written as uid 20000, no `chown` is needed.
- The System VM has no `setpriv` that can change uids. Busybox's `setpriv` supports only `--nnp` and the
  capability options, and util-linux is not in the package lock. So `stage` uses busybox `su -s /bin/sh vesperd-jail -c …`,
  where the overlay creates the system user `vesperd-jail` (uid and gid 20000, no login) at boot. This needs no new
  package. util-linux's `setpriv` would need one.

Reason: this is the jailer's own model. Rewriting macOS paths would also need undoing in `GET /vm/config` and in
error messages. Bind-mounting shares into the jail is deferred: it exposes whole shares.

### 2. Relay and launch mode

Launch: at start, `vesperd` sends `LAUNCHAPI <id> [ceiling]` to the agent. This is a new top-level verb, not a
`MICROVM` mode, because `MICROVM` buffers its output until it exits. `LAUNCHAPI` runs the jailer with
`-- --api-sock /run/firecracker.socket` and no `--config-file`. Without a configuration file, Firecracker builds
the microVM from API requests (`api_server_adapter.rs`). The reply grammar is:

- `READY <id> pid=<n> firecracker_version=<v>`, sent only once the API socket exists; or
- `ERR <reason>`, after which the agent closes the connection.

After `READY` the connection is the lease (question 4). `vesperd` refuses to serve if `<v>` is not the pinned
swagger's `info.version`.

Relay: `vesperd` reuses `proxy::run_server`. For each client connection it dials the bridge and sends
`CONNECT 1024`. It reads the greeting, sends `FCAPI <id>`, waits for `OK`, and then copies bytes both ways without
parsing HTTP. The agent's `FCAPI` checks that the id is valid and in the `vesperd` namespace, replies `OK`, and runs
`exec socat -t <long> STDIO UNIX-CONNECT:<path relative to the jail directory>`. A half-close passes through
unchanged, which is what Firecracker itself does.

Bounds:

- Every read has a buffer limit. There is no total byte limit on a connection.
- A short timeout applies to these reads only: the greeting, `OK`, `READY` and `ERR`, and the bridge preface write.
- Between requests, when no client bytes are waiting for a reply, an idle timeout applies.
- While client bytes are waiting, a hard per-request ceiling applies instead. It is set above the slowest
  synchronous operation at the supported memory size, for example `PUT /snapshot/create`. The ExecPlan sets the
  value from a measurement. The ceiling is recorded as a compat exception.
- Without parsing HTTP, "waiting" is inferred from which side sent bytes last; pipelined requests defeat that
  inference.
- `run_server` has a connection cap (a semaphore). The agent listener gets `max-children`.

Each connection costs one agent shell and one `socat` in the System VM. Any client-visible effect of the bounds is
a compat exception.

Every `socat` on the path runs with a long `-t`, because by default `socat` ends a connection 0.5 s after one side
reaches EOF (GH #167). That fix is under way for the Docker and port relays; the agent's listener and `FCAPI` need
it too.

Startup: `vesperd --backend systemvm` takes a `flock` on `PATH.lock` and holds it across three steps: the
live-socket check, the unlink and the bind. If `PATH` accepts a connection, `vesperd` refuses to start. It unlinks
`PATH` only when a connection to it is refused.

### 3. Cardinality

There is one `vesperd` process and one socket per microVM. `vesperd --backend systemvm --id ID --socket PATH`
mirrors `firecracker --id ID --api-sock PATH`, which is the model Firecracker clients expect. No routing table is
needed, and a crash affects one microVM; System VM resources stay shared, bounded only by the connection caps.

### 4. Restart

A microVM lives as long as its `vesperd`, as upstream it lives as long as its Firecracker process. The relay holds
no state, so there is nothing to reconcile after a restart.

- Lease: `LAUNCHAPI` waits for end of file on its connection or for Firecracker to exit, whichever comes first. It
  then stops the microVM, cleans up, and closes the connection. The bridge passes a close on, so a SIGKILL of
  `vesperd` also ends its microVM.
- The jailer must not outlive the lease holder. The lease shell traps its exit. Before it creates the jail, it
  records its pid and its start time (field 22 of `/proc/<pid>/stat`), so a reused pid is not mistaken for it.
- A reaper kills any `vesperd` microVM whose recorded holder no longer exists with that start time. One `flock`
  serialises reaping and jail creation. A jail without a record gets a grace period before it is reaped.
- The reaper runs at each `LAUNCHAPI`, at boot and periodically; the ExecPlan sets the interval. If SIGKILL hits the
  lease shell, Firecracker can therefore outlive it for up to one interval.
- When the lease closes from the System VM side, `vesperd` removes its socket and exits non-zero. A client then
  sees what it sees when Firecracker dies. A failed `PUT /snapshot/load` ends the process upstream
  (`snapshot-support.md`), and `vesperd` follows.
- On SIGTERM `vesperd` closes the lease and waits for clean-up. A new `vesperd` whose id still has a jail waits at
  least one reaper interval, then refuses.
- At boot, `vesper-services` removes every jail in the `vesperd` namespace. There is no reattach; Firecracker has
  none.

### 5. Snapshot persistence

`vesperd` microVMs use `--chroot-base-dir /var/lib/vesper/state/vesperd/jailer`, on the data disk. A store at
`/var/lib/vesper/state/vesperd/store` holds the files that outlive a microVM. Only store files survive a reboot,
because the boot sweep removes jails. `vesperd --backend systemvm` refuses to start unless `DATA status` reports
`formatted` or `mounted`.

- `PUT /snapshot/create` writes inside the jail. Pages written to tmpfs are charged to the writer's memory cgroup
  and cannot be reclaimed without swap; ext4 pages are charged too but can be written back and reclaimed (kernel
  cgroup v2 documentation; not measured in the System VM).
- A load needs the resources "at the same relative paths" in a new process (`snapshot-support.md`), here a new
  jail. The client copies the files from the old jail into the store, then stages them into the new jail.
- On load, Firecracker opens the vmstate and memory files read-only (`File::open` in `src/vmm/src/persist.rs`). The
  documentation requires the memory file to stay unchanged. So `--stage` links these two files root-owned and
  read-only, and one store copy serves several restores. Writable drives are copied.
- `PUT /snapshot/create` truncates an existing target (`persist.rs`), so a create onto a linked read-only file fails
  where upstream would overwrite: a compat exception.
- The store and the jails sit under one bind mount, because `link(2)` fails with `EXDEV` across mount points.

### 6. Two owners, one backend

The two owners avoid collisions by naming. This is not isolation: root in the System VM, and therefore the agent,
can act on either owner's microVMs.

| Resource | Shim | `vesperd` |
| --- | --- | --- |
| Jail root | `/srv/jailer/firecracker/<id>/root` | `/var/lib/vesper/state/vesperd/jailer/firecracker/<id>/root` |
| cgroup | `/sys/fs/cgroup/firecracker/<id>` | `/sys/fs/cgroup/vesperd/<id>` (`--parent-cgroup vesperd`) |
| uid and gid | 10000 | 20000 for every `vesperd` microVM |
| Process lookup | `pgrep -u 10000 -f "firecracker --id <id> "` (adds the uid filter) | the jailer's `firecracker.pid` in the jail root |
| Clean-up | `vesper-microvm cleanup`, called by the shim | the lease, the reaper and the boot sweep |
| Ceiling | `memory.max` 640 MiB, `pids.max` 256 | `memory.max` from `LAUNCHAPI`; the default is measured in slice 5 |

- Ids are not prefixed, because `GET /` returns the `--id`; the uid filter keeps the shim off `vesperd` processes.
- The jailer accepts ids of up to 64 letters, digits and hyphens. `vesperd`, `LAUNCHAPI`, `FCAPI` and `stage` all
  check the id. Slice 3 adds the same check to `FCPARITY`, which today joins an unchecked id into a path.
- One uid per owner meets the threat table's "a jailer uid per owner". A uid per microVM is deferred, together with
  CE-0004.

### 7. `vesperd` inside the System VM

Rejected; see option (c).

### 8. Backend B

Backend B keeps one socket per microVM and the same envelope rules, starts from the model, and is measured against
the Linux capture (D-014). A shim `backend.Backend` for it needs a new host-visible listener, left to the Phase 5
ADR (GH #56).

## Alternatives considered

### (a) Relay to one jailed Firecracker per `vesperd` socket (chosen)

Against it: files are staged rather than named in place, and each request crosses the bridge. Cost: a preface,
bounds, a connection cap and the socket check in `proxy.rs`; `LAUNCHAPI`, `FCAPI`, `stage` and a reaper in shell;
the allow-list.

### (b) `vesperd` keeps its state machine as the authority and translates calls

It would accept macOS paths and keep the contract tests meaningful for every backend. But clients would see
`vesperd`'s behaviour, so each difference would be a parity bug in front of a correct backend, hidden by measuring
through the same front end (D-014). `vesperd` would also need replay after restart and an inventory. This is the
largest option and the least faithful.

### (c) `vesperd` inside the System VM, next to Firecracker

Paths would be local, but macOS still needs a socket and relay, Backend B needs `vesperd` on macOS, the crate has
no Linux cross-build (ADR 0004), and a compromised System VM would hold `vesperd`.

### Rejected or deferred for single questions

| Option | Outcome | Reason |
| --- | --- | --- |
| An error body from `vesperd` on a relayed connection | Rejected | Without parsing HTTP, `vesperd` cannot place a body correctly: on an idle keep-alive connection, in the middle of a response, or in a race. |
| Added fields on Firecracker's errors; parsing HTTP in `vesperd` | Rejected | Errors would differ from the pinned behaviour; per-read bounds suffice without a parser. |
| A relay listener on vsock 2402; `MICROVM launch-api` | Rejected | `FCAPI` needs no listener; `MICROVM` cannot reply before it exits. |
| A uid allocator; shim pid files; an id prefix; tmpfs jails; a multiplexer | Rejected or deferred | One uid per owner and a uid filter suffice; `GET /` would change; snapshots would be lost; no need yet. |

## Rationale and consequences

Option (a) wins because the lead goal is Firecracker compatibility (D-011), Firecracker's own answer is the most
faithful one, and it avoids the questions D-017 raised.

What a client sees:

- On `--backend systemvm`: Firecracker's status codes, bodies and state rules, byte for byte, with jail paths.
  `vesperd`'s own failures appear only at connection level. Before any parity claim, compat exceptions record the
  jail paths, the connection-level failures, the effects of the bounds and the create-over-link refusal. Every
  claim cites the lock revision.
- On `--backend model`: nothing changes. Its extra fields (`code`, `path`, `method`,
  `baseline_firecracker_revision`) already differ from Firecracker, and no compat exception records them. A compat
  exception is opened when this ADR is accepted.

Invariant 3 becomes: on `--backend model`, every error is a deterministic `ErrorEnvelope` with the pinned revision.
`vesperd`'s startup and command-line errors are deterministic. On `--backend systemvm`, HTTP errors are
Firecracker's own. AGENTS.md and ARCHITECTURE.md are reworded when this ADR is accepted.

Tests:

- The 122 tests in `tests/api_contract.rs` are unaffected and now test the model only.
- The behaviour matrix (173 cases) becomes the oracle for a replay through `vesperd` on Backend A (slice 6).
  Where the model and Backend A disagree, the Linux capture decides (D-014).
- CI (which cannot boot the System VM, TD-005) tests `proxy.rs` against a fake upstream; suites on developer
  hardware give the runtime proof.

The binary composes `proxy` and `control` at layer 4, so the layer table does not change.

### Security

| Threat table row | Effect | Required |
| --- | --- | --- |
| Same-user process on macOS | The `vesperd` socket controls a real microVM, and `vesperd stage` copies share files into its jail. | TD-050: an owner-only directory and socket by default. TD-027: a deny-by-default allow-list in the launch config of 1024, 2375, 2400 and 2401, enforced by the launcher. `vesperd` refuses, under `PATH.lock`, to take over a live socket. |
| Hostile container on the Docker path | Docker's default seccomp profile denies `socket(AF_VSOCK)` (moby v28.3.3, `profiles/seccomp/default.json`). A `--privileged` or `seccomp=unconfined` container is not covered; its reach over vsock loopback is unverified. | A slice 3 test: a default container fails to open `AF_VSOCK`. That `FCAPI` accepts only ids in the `vesperd` namespace avoids collisions; it is not a required control. |
| Hostile workload in a microVM | A compromised Firecracker controls the files in its jail, and other `vesperd` microVMs share its uid and the store. Data the guest influences reaches `vesperd` only as relayed bytes. | The `stage` rules in question 1: jail-side input and output run as uid 20000; root-owned, read-only links are made only into fresh jails. Per-read bounds. |
| Compromised System VM | It can send anything on the relay or the lease. | Every read is bounded and timed out (D-016). `vesperd` parses only the greeting, `OK`, `READY` and `ERR`, each against a fixed form. This is a step on TD-026, not its closure. |

The allow-list stops a later change from opening a port, not access to the listed ones. `FCAPI` adds nothing
`FCPARITY api` cannot do. Tokens and per-boot keys stay Deferred (D-018).

### Owner confirmation needed

1. The bridge allow-list: vsock ports 1024, 2375, 2400 and 2401, with every other port refused (TD-027).
2. The client surface on macOS:
   - the default `vesperd` socket path (TD-050);
   - one owner-only socket for each microVM;
   - the staging interface: `--stage SRC:DEST`, `vesperd stage` and `vesperd store list|rm`. `SRC` is limited to
     configured shares, the store, and the microVM's own jail.
   - one snapshot store shared by all of the macOS user's `vesperd` microVMs, treated as one trust domain.
3. Narrowing invariant 3. It covers the model backend and `vesperd`'s startup and command-line errors. On the
   systemvm backend, `vesperd`'s own failures are refused or closed connections with no body.

## Implementation and migration impact

An ExecPlan follows acceptance ([PLANS.md](../../PLANS.md)). It sets the read limits, the idle timeout, the
connection cap and `max-children`, the `socat -t` value, the wait times and reaper interval, the exact `stage`
checks, the per-request ceiling, the store free-space reserve, the reaper grace period, and how `FCAPI` handles
socket paths over the 108-byte `sun_path` limit.

| Slice | Change | Proof | Tracker |
| --- | --- | --- | --- |
| 1 | Owner-only default socket. | A test that starts the binary and checks the modes. | Closes TD-050 |
| 2 | Bridge allow-list in the launch config, enforced in `handleBridgeClient`. | Unit tests; a `make docker-smoke` step showing that a port that is listening but not listed is refused. | Closes TD-027 |
| 3 | `LAUNCHAPI` with lease, trap and reaper; `FCAPI`; uid 20000; `--parent-cgroup vesperd`; the uid filter in `fcpid()`; id checks in `FCPARITY`; `-t` and `max-children` on the agent listener. | A System VM suite over `CONNECT 1024`: launch, `FCAPI`, boot, `GET /` returns `Running`. Three teardown cases leave the jail clean: closing the lease, killing Firecracker, and SIGKILL of the lease shell (reaper). The reaper ignores a live holder whose pid was reused, and spares a new jail that has no record yet. A default container fails to open vsock. `make systemvm-microvm-suite` and `make ctr-conformance` still pass. | Prerequisite for TD-045 |
| 4 | `vesperd --backend systemvm` on `proxy::run_server`: preface, per-read bounds, idle timeout, per-request ceiling, connection cap, version check, live-socket refusal under `PATH.lock`. | `src/proxy.rs` tests against a fake upstream, including a refused takeover of a live socket and two racing starts on one path. A suite through the socket that boots, pauses, resumes and stops a microVM. It also checks that SIGKILL of `vesperd` removes the jail, and that a second `vesperd` with the same id is refused. | Partly closes TD-045; changes TD-048 |
| 5 | Chroot base on the data disk, boot sweep, the `vesperd-jail` user, `--stage`, `vesperd stage` and `vesperd store` with their rules, the snapshot cycle, and measurement of the memory ceiling. | Pause, create, copy into the store, stop. Then a new `vesperd` stages the snapshot by link, loads it and resumes, and the workload continues. Tests that refuse a symlinked `SRC`, a symlinked `DEST` and a `SRC` outside the roots. A test shows that jail-side copies run as uid 20000, and one shows that a copy that would breach the free-space reserve is refused. After a reboot only store files remain. | Closes TD-045; changes TD-025; gives the first load cycle for CE-0001 |
| 6 | Behaviour-matrix replay through `vesperd` on Backend A. | A suite report. | Changes TD-003; feeds GH #87 |

Slice 1 is the first that can be built and shown working on its own; it needs no System VM. Slices 1 and 2 are
gates (D-018). Slice 3 is the first to drive Backend A through the API, and it needs no change to `vesperd`.

Migration: none; the shim gains only the uid filter. New tech-debt rows: memory shared by both owners with no
budget, no reattach, no network interfaces on `vesperd` microVMs.

### Upstream facts checked

Firecracker at `d83d72b710361a10294480131377b1b00b163af8` (v1.16.0): `src/firecracker/src/main.rs`,
`api_server_adapter.rs` and `api_server/mod.rs` (socket default, optional config file, build from requests, `--id`
in `GET /`, `fault_message` bodies); `docs/jailer.md` and `src/jailer/src/chroot.rs` (jail layout, id rules,
`firecracker.pid`, `--parent-cgroup`, `pivot_root` then `chdir("/")`); `docs/snapshotting/snapshot-support.md` and
`src/vmm/src/persist.rs` (same relative paths, failed load ends the process, read-only open on load, truncate on
create). Docker: moby `v28.3.3` `profiles/seccomp/default.json`, matching the pinned `docker-engine` 28.3.3. Busybox: Alpine
`3.22-stable` `main/busybox/busyboxconfig` (busybox 1.37.0) enables `SETPRIV` and `SU` and leaves `CHPST` and
`SETUIDGID` off. Busybox's `util-linux/setpriv.c` (read on the `master` branch of the GitHub mirror, not at the 1.37.0 tag) offers
no `--reuid`, `--regid` or `--clear-groups`.

Not verified, with the slice that settles each one:

- The jailer working under the `/var/lib/vesper/state` bind mount (slice 3).
- Vsock loopback reachability from a container without seccomp (slice 3).
- Concurrent API connections to Firecracker, and whether Firecracker has an idle timeout of its own (slice 4).
- Whether busybox `su` as root, with `-s`, runs a command as a user that has no login shell in the System VM (slice 5).
- Files linked into a running jail being visible inside it (slice 5).
- `EXDEV` across mount points, and ext4 page reclaim under `memory.max`, inside the System VM; both come from Linux
  documentation (slice 5).

## References

- GH #156, GH #167; D-008, D-011, D-013, D-014, D-016, D-017, D-018 ([decision log](../decision-log.md)).
- [design-v0.md](../../specs/design-v0.md); [ADR 0003](0003-systemvm-control-channel.md),
  [ADR 0004](0004-containerd-shim-in-go.md), [ADR 0005](0005-published-ports-via-vsock-gateway.md),
  [ADR 0006](0006-guest-exec-via-runc-delegation.md).
- [systemvm-bringup.md](../../process/systemvm-bringup.md), [SECURITY.md](../../SECURITY.md),
  [ARCHITECTURE.md](../../../ARCHITECTURE.md).
- [Tech-debt tracker](../../exec-plans/tech-debt-tracker.md): TD-003, TD-025, TD-026, TD-027, TD-032, TD-045, TD-048,
  TD-050. [compat-exceptions.yaml](../../specs/compat-exceptions.yaml): CE-0001, CE-0002, CE-0004.
