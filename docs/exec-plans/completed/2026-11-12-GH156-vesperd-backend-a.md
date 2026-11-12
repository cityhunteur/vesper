# Connect `vesperd` to Backend A as a relay to one jailed Firecracker per socket (GH #156, ADR 0007)

This ExecPlan is a living document. The sections Progress, Surprises & Discoveries, Decision Log, and
Outcomes & Retrospective must be kept up to date as work proceeds. It is maintained in accordance with
docs/PLANS.md.

Tracker: GitHub #156. Design: [ADR 0007](../../design-docs/adr/0007-vesperd-to-backend-a.md) (accepted 2026-11-12,
decision D-022). Closes or changes TD-027, TD-045, TD-048 and TD-050; touches TD-025 and TD-026.

## Purpose / Big Picture

Today `vesperd` answers the 38 pinned Firecracker routes from an in-memory model and drives no microVM (TD-045).
ADR 0007 makes `vesperd --backend systemvm` a byte relay to one stock Firecracker v1.16.0, jailed inside the System
VM, with one owner-only socket on macOS per microVM. When the plan is complete a Firecracker client can point at a
`vesperd` socket on macOS, configure and boot a real microVM with ordinary API requests, pause, snapshot, load and
stop it, and see Firecracker's own responses byte for byte.

The ADR splits the work into six slices. Each slice is its own commit and pull request, with its own proof:

1. an owner-only default socket for `vesperd` (TD-050);
2. a deny-by-default vsock port allow-list in the launcher's bridge (TD-027);
3. the System VM side: `LAUNCHAPI` with its lease, trap and reaper, `FCAPI`, uid 20000, `--parent-cgroup vesperd`,
   the uid filter in the shim's process lookup, id checks in `FCPARITY`, and `-t` and `max-children` on the agent
   listener. No change to `vesperd`;
4. `vesperd --backend systemvm` on `proxy::run_server`;
5. the chroot base on the data disk, the boot sweep, the `vesperd-jail` user, staging, the store and the snapshot
   cycle;
6. the behaviour-matrix replay through `vesperd` on Backend A.

Slice 3 is the first that drives Backend A through the Firecracker API. Its proof is `make
systemvm-launchapi-suite`: a System VM suite over `CONNECT 1024` that launches a microVM, boots it through the API
socket, sees `GET /` return `Running`, and shows that each teardown path leaves nothing behind.

## Progress

- [x] (2026-11-12) Slice 1: owner-only default socket, `<socket>.lock`, live-socket refusal (PR #169; TD-050 resolved).
- [x] (2026-11-12) Slice 2: bridge allow-list 1024, 2375, 2400, 2401 enforced in `handleBridgeClient` (PR #171;
      TD-027 resolved).
- [x] (2026-11-12) Slice 3: guest helper `systemvm/overlay/usr/local/bin/vesper-vesperd` (launch, connect, reaper,
      inspection and test hooks), agent verbs `LAUNCHAPI`, `FCAPI` and `VESPERD`, `max-children=64` on the agent
      listener, the `vesper-vesperd-reaper` service, the uid filter in `vesper-microvm`'s `fcpid()` and
      `vesper-ctr leftovers`, id checks in `vesper-fcparity`.
- [x] (2026-11-12) Slice 3: host client `src/control/launchapi.rs`, suite `src/control/suites/launchapi.rs`,
      `systemvmctl control launchapi-suite` and `control vesperd`, `make systemvm-launchapi-suite`; unit tests
      against fake agents.
- [x] (2026-11-12) Slice 3: runtime proof on an M3 Pro: launchapi suite 20/20; the regression suites listed under
      Validation and Acceptance pass (counts in Outcomes & Retrospective).
- [x] (2026-11-12) Slice 3: merged as PR #176 (`4811ca7`), CI green, GH #156 updated.
- [x] (2026-11-12) Slice 4: `proxy::serve`, `open_upstream`, `claim_socket` (moved from `vesperd`) and
      `src/proxy/relay.rs` (`Preface`, `Bounds`, the bounded relay); `vesperd --backend systemvm` in
      `src/bin/vesperd/systemvm.rs` (lease, version check, reused-id wait, signals); unit tests against a fake
      upstream and process tests against a fake agent (`tests/vesperd_systemvm.rs`).
- [x] (2026-11-12) Slice 4: `make systemvm-vesperd-suite` (`src/control/suites/vesperd.rs`), 18/18 on an M3 Pro;
      CE-0007; the parity baseline republished with CE-0007 open; regression suites pass (Outcomes).
- [x] (2026-11-12) Slice 4: merged as PR #177 (`5cf45f8`), CI green, GH #156 updated.
- [x] (2026-11-12) Slice 5: chroot base `/var/lib/vesper/state/vesperd/jailer` and store
      `/var/lib/vesper/state/vesperd/store` on the data disk, `LAUNCHAPI` refusing without it, the boot sweep in
      `vesper-data mount`, the `vesperd-jail` user (`setup-user`, the reaper service's `start_pre`), `stage`,
      `store-list`, `store-rm`, `files`, `memory` and the hooks `test-plant` and `test-reserve` in `vesper-vesperd`;
      `vesper_mark=1` and `vesper_fill_mib=N` in the microVM init.
- [x] (2026-11-12) Slice 5: `src/control/stage.rs` (forms, client calls), `vesperd --stage`, `vesperd stage`,
      `vesperd store list|rm` (`src/bin/vesperd/stage.rs`), the `DATA status` check (the slice 4 deferral);
      `tests/vesperd_systemvm.rs` 11 tests (5 new) against the fake agent.
- [x] (2026-11-12) Slice 5: `make systemvm-snapshot-suite` (`src/control/suites/snapshot.rs`, 23 steps) and the
      `make data-disk-suite` step `vesperd_store_survives_jails_swept`; CE-0008 opened, CE-0007 updated with the
      measurement; regression suites and parity baseline (Outcomes).
- [x] (2026-11-12) Slice 5: merged as PR #179 (`4bd76cf`), CI green, GH #156 updated.
- [x] (2026-11-12) Slice 6a: replay scripts for all 173 matrix cases (`docs/specs/firecracker-api-behavior-replay.yaml`),
      `src/api_replay.rs`, the model replay in CI (`tests/api_replay.rs`), the Backend A runner
      (`src/control/suites/replay.rs`, `systemvmctl control replay-suite`, `make systemvm-replay-suite`), the guest
      balloon and virtio-mem drivers, the published report `docs/generated/backend-a-behavior-replay.json`; CE-0009,
      CE-0010, TD-058, D-026.
- [x] (2026-11-12) Slice 6a: merged as PR #181 (`bdcb467`), CI green, GH #156 updated.
- [x] (2026-11-12) Slice 6b: 35 captures corrected (the 33 plus the two snapshot-load refusals Backend A cannot
      reach without a network interface, same source line), evidence
      `run/GH-156/api/slice6b-backend-a-replay-corrections.md`; the model follows: message prefixes, Firecracker's
      memory-hotplug validation order and texts, the balloon message quoting the request, `PUT /logger`, `/metrics`
      and `/serial` pre-boot only, no minimum `guest_cid`, `PUT /cpu-config` not blocking a load, and `GET /vm/config`
      in Firecracker's shape (all 13 keys, `null` or `[]` when empty). Replay: model 170 of 170 applicable; Backend A
      156 match, 14 CE-0010, 3 not applicable.
- [x] (2026-11-12) Slice 6b: merged as PR #182 (`e5895f6`), CI green, GH #156 updated.
- [x] (2026-11-12) Owner approved CE-0009 and CE-0010 as written; GH #156 closed.

## Surprises & Discoveries

- Observation: busybox `read -t` returns status 1 both on a timeout and at end of file, so a lease shell cannot use
  it to tell the two apart.
  Evidence: in the System VM, `(sleep 3) | read -r -t 0.5 x` and `printf '' | read -r -t 0.5 x` both give `rc=1`.
  The lease loop uses `timeout 1 head -c 1` instead, whose `rc=0` with no byte means end of file (`rc=143` is a
  timeout).
- Observation: when the lease shell is killed with SIGKILL, socat 1.8.1.3 keeps the vsock connection open, so the
  client never sees the lease end, although no process holds the child's end of socat's socketpair any more. A
  normal exit of the shell closes the connection at once.
  Evidence: first suite run, step `teardown_lease_shell_killed_reaped` failed with "connection still open after
  20 s"; `/proc/<socat>/fd` still held the vsock socket after the microVM was reaped. Fix: the record also names
  the agent's per-connection socat (`conn=<pid>:<start>`), and the reaper sends it SIGTERM after a holder-gone
  teardown. The rerun closed the lease 399 ms after the reaper's pass.
- Observation: Firecracker's main thread has seccomp mode 0 until `InstanceStart`; the VMM thread installs its
  filter when the microVM starts. An isolation check on an unconfigured Firecracker reports `seccomp_ok` false.
  Evidence: first suite run, `jailer_isolation` before boot: `seccomp_mode` 0; after boot: 2. The suite checks
  isolation after the guest boots.
- Observation: the jailer works under the `/var/lib/vesper/state` bind mount on the data disk (an item the ADR left
  for slice 3). Evidence: a manual run on 2026-11-12 with `--chroot-base-dir
  /var/lib/vesper/state/vesperd-probe/jailer --parent-cgroup vesperd`: `firecracker.pid` written, API socket
  created, `Running Firecracker v1.16.0` and `API server started` in its log. The slice 3 chroot base stays on
  tmpfs, as the ADR's table puts the data-disk base in slice 5.
- Observation: vsock loopback from a container without seccomp (an item the ADR left for slice 3). A default
  container cannot create an `AF_VSOCK` socket at all (`socket(40, 1, 0): Operation not permitted`). A
  `seccomp=unconfined` container can create one, but `connect` to CID 1 port 1024 is reset and to CID 3 port 1024
  fails with `No such device`, because the System VM loads no `vsock_loopback` transport. A privileged container
  that can load kernel modules is not covered.
  Evidence: step `default_container_cannot_open_vsock` in `run/systemvm/launchapi-report.json`.
- Observation: busybox `pgrep` in Alpine 3.22 supports `-u EUID` (the ADR's `pgrep -u 10000 -f` works), `flock` has
  no `-w`, `nsenter` has no `-C`, and `tr` handles no `[:print:]` complement.
  Evidence: `pgrep --help`, `flock --help` and a `tr -c '[:print:]'` test in the System VM.

- Observation: Firecracker v1.16.0's API server (micro_http `876f3fe`, pinned by Firecracker's `Cargo.lock`) accepts
  10 connections and answers the next with its own `HTTP/1.1 503` "Too many open connections" body, then closes;
  it has no idle timeout (an item the ADR left for slice 4).
  Evidence: `src/server.rs` at that commit (`MAX_CONNECTIONS = 10`, `SERVER_FULL_ERROR_MESSAGE`, no timer);
  suite steps `firecracker_connection_limit_passes_through` (the 11th connection through `vesperd` got those bytes
  exactly) and `idle_timeout_closes_only_vesperd_connections` (a direct `FCAPI` connection idle for 125 s still
  answered `GET /`).
- Observation: `println!` panics when stdout is closed, so a `vesperd` whose stdout pipe was gone exited 101 on
  SIGTERM, after the teardown but with the wrong code.
  Evidence: first suite run, step `sigterm_stops_microvm`: `failed printing to stdout: Broken pipe`. Fix: `say()`
  writes status lines and ignores the error; `tests/vesperd_systemvm.rs` closes stdout before SIGTERM.
- Observation: `PUT /snapshot/create` (Full) of a 768 MiB guest fails with Firecracker's own 400 `No space left on
  device`: the slice 3 jails are on the System VM's tmpfs, which cannot hold the memory file. 512 MiB succeeds.
  Evidence: manual runs on 2026-11-12 through `vesperd` (`--memory-max` twice the guest plus 128 MiB): 288 MiB in
  197 ms, 512 MiB in 223 ms; 768 MiB 400 after 183 ms. The suite's 256 MiB create took 77 to 168 ms. The guest touches
  little of its memory, so these are close to tmpfs copy speed, not the worst case.
- Observation: SIGKILL of `vesperd` leaves its socket file (nothing can remove it), and `make docker-conformance`'s
  `no_raw_host_sockets` step flags any socket file under `run/`. Evidence: first `docker-conformance` after the
  suite: `unexpected: run/vesperd-suite/kill.sock`. The suite now removes that stale file and checks that no socket
  is left in its directory; a later `vesperd` on the path would replace it (slice 1).

- Observation: busybox `su -s /bin/sh vesperd-jail -c 'CMD' ARG0 ARGS` as root runs CMD as uid and gid 20000 for a
  user whose shell is `/sbin/nologin` (an item the ADR left for slice 5); `$0` and `$@` carry the arguments, stdin and
  stdout pass through, and the exit code is CMD's.
  Evidence: in the System VM, `su -s /bin/sh vesperd-jail -c 'id; cat -- "$1"' a0 /etc/shadow` printed `uid=20000`
  and `cat: can't open '/etc/shadow': Permission denied`, rc 1; `-c "exit 7"` gave rc 7.
- Observation: a file staged into a running jail is visible to Firecracker, and links made before the jailer starts
  are too (an item the ADR left for slice 5). Evidence: suite step `boot_with_both_drives`: `PUT /drives` of a file
  staged at start and of one staged while Firecracker ran both return 204 (Firecracker opens the file there).
- Observation: `link(2)` between the store and a jail works, since both are under the `/var/lib/vesper/state` bind
  mount; `EXDEV` across mount points was not provoked, because the design never needs it. Evidence: step
  `new_vesperd_stages_by_link` (`links=2`).
- Observation: page cache of files a microVM writes is charged to its cgroup and reclaimed under `memory.max` (an item
  the ADR left for slice 5). A `PUT /snapshot/create` of a 640 MiB guest with 512 MiB dirtied, under the 640 MiB
  default, held `anon` at 608 MiB and drove `memory.current` to the limit while the kernel wrote back and reclaimed
  the memory file's pages; it took 1.2 to 1.7 s and did not OOM. A guest that dirties past the limit (704 MiB guest,
  600 MiB dirtied) is killed by the memcg OOM killer (`oom_memcg=/vesperd/meas-8`), and `vesperd` exits 1 when the
  lease closes. Evidence: manual runs on 2026-11-12 (`memory ID` before and after) and suite step
  `measure_create_at_default_ceiling`; `dmesg` for the OOM.
- Observation: the guest's initramfs root is a tmpfs capped at half the guest's memory, so `vesper_fill_mib` writes
  into a tmpfs of its own. A guest kernel with less than about 112 MiB left after the fill panics with "System is
  deadlocked on memory" (512 MiB guest, 448 MiB fill). Evidence: manual runs on 2026-11-12.
- Observation: Firecracker's memory file is written in full, not sparse: 256 MiB guest, 268435456 bytes; 640 MiB
  guest, 671088640 bytes. Copying the 256 MiB file into the store as uid 20000 through a pipe took 0.73 to 0.93 s.
- Observation: a restored guest on the serial console of the new microVM continues from the snapshot: the same
  token, ticks 4 and 5 after tick 3, and the kernel's `random: crng reseeded due to virtual machine fork`.

## Decision Log

- Decision: slice 3 keeps the chroot base for `vesperd` microVMs on tmpfs, at `/srv/vesperd`, apart from the
  shim's `/srv/jailer`. Rationale: the ADR's implementation table puts the data-disk base and the boot sweep in
  slice 5, with staging and snapshots, which are what need it; a tmpfs base needs no boot sweep. The jailer was
  shown to work on the data disk (Surprises). Date/Author: 2026-11-12, slice 3 agent. Superseded by slice 5, which
  moved the base to `/var/lib/vesper/state/vesperd/jailer`.
- Decision: `FCAPI` runs `socat … UNIX-CONNECT:/run/firecracker.socket,chroot-early=<jail root>` rather than
  connecting to a path relative to the jail directory. Rationale: this answers the 108-byte `sun_path` limit, since
  the path inside the jail is 23 bytes whatever the chroot base and id (the slice 5 base with a 64-character id is
  about 150 bytes), and it also stops a compromised Firecracker, which owns the jail root, from redirecting the
  root-run socat with a symlink to a socket outside the jail. A probe in the System VM showed a symlinked `run2 ->
  /run/containerd` resolving inside the chroot and failing. Date/Author: 2026-11-12, slice 3 agent.
- Decision: the jailer's `firecracker.pid` is read once, when the API socket appears and before any guest code
  runs, checked (a regular file, not a symlink, naming a process with effective uid 20000 and `--id <id>`), and
  recorded with its start time in a root-only record outside the jail. Later lookups use the record; `pgrep -u
  20000 -f "firecracker --id <id> "` is only the fallback for a holder that died before `READY` and for jails
  without a record. Rationale: Firecracker owns the jail root and could replace the pid file later; the ADR's
  table names the pid file as the lookup, and this reads it at the one moment it can be trusted.
  Date/Author: 2026-11-12, slice 3 agent.
- Decision: a third agent verb, `VESPERD <args…>`, runs `vesper-vesperd` with the buffered reply shape of the other
  guest tools, for `status`, `isolation`, `list`, `log`, `reap`, `kill-holder`, `kill-fc` and three test hooks; it
  refuses `launch` and `connect`. Rationale: the suite needs to inspect and fault the microVMs, and the agent
  already runs as root with `MICROVM crash`; a verb keeps the inspection out of `LAUNCHAPI` and `FCAPI`, whose
  grammar stays as the ADR fixed it. Date/Author: 2026-11-12, slice 3 agent.
- Decision: the lease record also names the agent's per-connection socat, and a holder-gone reap sends it SIGTERM.
  Rationale: without it a client never sees a SIGKILLed lease end (Surprises); the start-time check keeps a reused
  pid safe. Date/Author: 2026-11-12, slice 3 agent.
- Decision: the uid filter also goes into `vesper-ctr leftovers`, which counted every Firecracker process.
  Rationale: a `vesperd` microVM running during `make ctr-conformance` would otherwise make the shim suites report
  `DIRTY`; the shim itself (`shim/`) does not change. Date/Author: 2026-11-12, slice 3 agent.
- Decision: on the agent side a reused id is refused at once with `ERR busy <id>` while a record, jail or cgroup
  for it remains after a reaper pass; the wait of at least one reaper interval before refusing belongs to `vesperd`
  in slice 4. Rationale: the agent cannot know how long the client is willing to wait; `LAUNCHAPI`'s own reaper
  pass already removes a dead holder's microVM. Date/Author: 2026-11-12, slice 3 agent.

- Decision: the System VM backend lives in the `vesperd` binary (`src/bin/vesperd/systemvm.rs`, `#[path]` module),
  not in a library module. Rationale: it composes `proxy` and `control`, two layer 3 modules that may not reference
  each other; the ADR places that composition at layer 4, so the layer table does not change. `proxy` gained only
  generic pieces (`Preface`, `Bounds`, `serve`, `open_upstream`, `claim_socket`) and knows nothing of the agent.
  Date/Author: 2026-11-12, slice 4 agent.
- Decision: `claim_socket` moved from `vesperd.rs` to `proxy`, unchanged, and both backends call it. Rationale: the
  ADR's proof puts the live-socket and racing-start tests in `src/proxy.rs`; the model backend's behaviour and its
  `tests/vesperd_socket.rs` stay as they were. `vesper-proxy` keeps its remove-and-bind behaviour.
  Date/Author: 2026-11-12, slice 4 agent.
- Decision: the socket lock is taken before `LAUNCHAPI`, and the socket is bound only after `READY` and the version
  check. Rationale: a second `vesperd` on the same socket then launches nothing (suite step
  `second_vesperd_on_socket_refused`), and a client sees the socket only once Firecracker can answer. The lock is
  held across the live-socket check, the unlink and the bind, as the ADR requires, with the launch in between.
  Date/Author: 2026-11-12, slice 4 agent.
- Decision: the connection cap is 16, above Firecracker's own 10. Rationale: the 11th to 16th connections then reach
  Firecracker and get its own 503 byte for byte, so `vesperd`'s cap is seen only when more than 16 connections are
  open at once (Firecracker closes the extra ones at once). With a cap of 10, the 11th would get a closed connection
  instead of Firecracker's answer. Date/Author: 2026-11-12, slice 4 agent.
- Decision: the per-request ceiling runs from the first client byte after the upstream last sent bytes, and the
  idle timeout from the last byte either way; the watchdog wakes on its deadline or when the side that spoke last
  changes (`tokio::sync::Notify`). Rationale: this is the ADR's inference without parsing HTTP; a request with a
  slow upload counts towards the ceiling, which is harmless for Firecracker's bodies of at most 51200 bytes
  (micro_http `MAX_PAYLOAD_SIZE`). Date/Author: 2026-11-12, slice 4 agent.
- Decision: `vesperd` removes its socket on every exit of the System VM backend, also after SIGTERM. Rationale: the
  microVM is gone, so the path can only refuse; the ADR requires it for a lease closed from the System VM side.
  Date/Author: 2026-11-12, slice 4 agent.
- Decision: the `DATA status` check before start (ADR §5) is left to slice 5. Rationale: it guards the chroot base on
  the data disk, which slice 5 introduces; slice 4 jails are still in tmpfs. Date/Author: 2026-11-12, slice 4 agent.

- Decision: the `stage` interfaces reach the System VM as `stage=SRC:DEST` tokens on `LAUNCHAPI` (for `--stage`)
  and as `VESPERD stage|store-list|store-rm` (for `vesperd stage` and `vesperd store`), not as new verbs. Rationale:
  `--stage` must run in the fresh jail before the jailer starts, which only `LAUNCHAPI` sees (the jail is created
  under its lock); the running-microVM operations have the buffered reply shape that `VESPERD` already gives; no new
  listener or host socket. The ADR named `MICROVM stage`; `vesper-microvm` is the shim's tool, so the `vesperd`
  helper owns these. Date/Author: 2026-11-12, slice 5 agent.
- Decision: `SRC:DEST` grammar. `SRC` is `share:/PATH`, `store:NAME` or `jail:/PATH`; `DEST` is `/PATH` or
  `store:NAME`. Paths are absolute, of letters, digits and `._+,@=-` between single slashes, no `.` or `..`
  component, at most 1024 bytes; names are 1 to 128 of the same characters without `/`, not starting with a dot.
  Rationale: the agent splits its line on white space and expands globs, and `su -c` takes a shell string, so a
  path must be one safe word; no colon in a path makes `SRC:DEST` unambiguous (the second colon ends `SRC`). A share
  path with a space or a colon cannot be staged; copy it inside the share first. Date/Author: 2026-11-12, slice 5
  agent.
- Decision: `--stage` writes only into the jail and takes `share:` or `store:`; `vesperd stage` takes any `SRC`, any
  `DEST`, refuses store to store and jail to jail, and needs `--id` only when a side is in the jail. Store files named
  `*.mem` or `*.vmstate` are linked by `--stage`; everything else is copied. Rationale: the ADR links only the
  snapshot memory and vmstate files, and the name is the one thing `--stage` can see; a writable drive named `*.mem`
  is linked read-only and Firecracker then fails to open it for writing, which is safe. Date/Author: 2026-11-12, slice 5
  agent.
- Decision: neither form overwrites: a jail file is created with `set -C` (`O_EXCL`), and a store name that exists is
  refused (`vesperd store rm` first). Rationale: `O_EXCL` also refuses a symlink as the last component, so no
  overwrite path through a planted link exists; the store is shared, so silent replacement would surprise another
  microVM's user. Date/Author: 2026-11-12, slice 5 agent.
- Decision: jail-side work runs as uid 20000 by re-entering `vesper-vesperd` through `su` (`jail-stat`, `jail-read`,
  `jail-write`, `jail-files`, each refused for any other uid), and the symlink checks of jail paths run there too.
  Rationale: one copy of the checks; root never opens or walks a jail path. Root reads share files (after the
  symlink check below the mount point) and store files. Date/Author: 2026-11-12, slice 5 agent.
- Decision: the free-space reserve is the System VM's `MemTotal` (2 GiB) and is checked before every `stage` copy,
  into the store or a jail, under the store lock with the copy itself. Rationale: the largest memory file one
  microVM can write is its ceiling, at most `MemTotal`, so after any copy the disk still holds one more full
  snapshot; the ext4 root reserve (5 %) stays for dockerd and containerd, which write as root. Copies into jails count
  because they fill the same disk. Date/Author: 2026-11-12, slice 5 agent.
- Decision: `--stage` copies run outside the `LAUNCHAPI` lock (the record, written before the jail, keeps the reaper
  away), and `vesperd` waits 30 s plus 120 s for `READY` when it sends `stage=` tokens. Rationale: a copy of a large
  drive must not block other launches and the reaper; the measured copy rate (about 270 MB/s) moves about 30 GiB in
  120 s. Date/Author: 2026-11-12, slice 5 agent.
- Decision: the boot sweep runs in `vesper-data mount`, after the bind mounts and only on a fresh mount; it removes
  every jail without a record (none survives a boot, since records are in tmpfs) and every `.partial.*` store file.
  Rationale: the disk is mounted in the background after networking, and `LAUNCHAPI` refuses until it is, so no
  launch can race the sweep. Date/Author: 2026-11-12, slice 5 agent.
- Decision: the default ceiling stays 640 MiB, and the per-request ceiling stays 120 s. Rationale: with jails on ext4
  the memory file's page cache is reclaimable, so a 640 MiB guest with 512 MiB dirtied snapshots under the limit in
  1.2 to 1.7 s (2.4 s worst seen, a 576 MiB guest); 120 s is about 50 times that, and lowering it would buy nothing
  measurable. The 640 MiB default no longer needs to hold a memory file in tmpfs, so a larger guest fits than in
  slice 4; the ceiling is still the shim's, and TD-056 holds the shared budget. Date/Author: 2026-11-12, slice 5
  agent.
- Decision: a separate `make systemvm-snapshot-suite` rather than more steps in the vesperd suite. Rationale: it
  needs a share in the launch config, which the vesperd suite does not, and it runs in about 30 s while the vesperd
  suite waits 2.5 min for its idle timeout; each suite keeps one precondition. The reboot proof is a step of `make
  data-disk-suite`, which already reboots. Date/Author: 2026-11-12, slice 5 agent.
- Decision: the observable guest state is a random token the guest keeps in memory and prints with a tick every
  second on its serial console (`vesper_mark=1`); a restored guest must print the same token with later ticks.
  Rationale: the token exists only in guest memory, so the new process can print it only from the snapshot; the
  serial console is how the other suites observe the guest. Date/Author: 2026-11-12, slice 5 agent.

### Values

Each value is set where it is used. The guest values are constants at the top of
`systemvm/overlay/usr/local/bin/vesper-vesperd`; `max-children` is in `systemvm/overlay/etc/init.d/vesper-agent`;
the reaper interval is the `vesper-vesperd-reaper` service's argument.

| Value | Set to | Reason | Slice |
|---|---|---|---|
| Agent listener `max-children` | 64 | Each lease and each `FCAPI` relay holds one handler (a socat and a shell, a few MiB) for its whole life. The System VM has 2 GiB and 2 vCPUs, so at most about 12 microVMs fit even at the 128 MiB minimum ceiling (3 at the 640 MiB default). 64 leaves room for their leases, their relays under the slice 4 connection cap, the suites and the control connections, and bounds the handlers to well under 10 % of memory. At the limit socat stops accepting and new connections wait in the listen backlog. | 3 |
| Agent listener and `FCAPI` `socat -t` | 86400 s | After a half-close socat keeps relaying the other direction until it closes; the default ends it 0.5 s later (GH #167). Firecracker keeps a connection open as long as the client does. | 3 |
| Reaper interval | 10 s | Bounds how long Firecracker outlives a SIGKILLed lease shell; a pass is a few `stat` calls and costs nothing measurable. Measured: a lease shell killed with SIGKILL had its microVM reaped and its lease closed 399 ms to 10 s later. | 3 |
| Reaper grace period for a jail without a record | 60 s | `LAUNCHAPI` writes the record before it creates the jail, under the same `flock`, so only a jail made outside `LAUNCHAPI` can lack one. 60 s is six reaper intervals and well above the 10 s launch wait plus the 3 s stop grace, so no jail is reaped while something may still be setting it up. | 3 |
| Wait for the API socket after the jailer starts | 10 s | The socket appeared 0.2 s after the jailer started (cached kernel and initramfs); the first launch after boot, which builds them, took 1.4 s in total. | 3 |
| Stop grace, SIGTERM to SIGKILL | 3 s | A teardown took about 0.5 s; an unconfigured or idle Firecracker exits on SIGTERM at once. The lock is held during teardown, so the grace bounds how long other launches wait. | 3 |
| Lease poll | 1 s | Firecracker's exit and the client's end of file are noticed within 1 s (measured: the lease closed 1015 ms after Firecracker was killed). Each poll forks `timeout` and `head`. | 3 |
| Default `memory.max` (`LAUNCHAPI` ceiling) | 671088640 bytes (640 MiB), accepted range 128 MiB to the System VM's `MemTotal` | The shim's value. Slice 5 measured it with jails on the data disk and kept it (Decision Log). | 3; measured in 5 |
| `pids.max` | 256 | The shim's value. | 3 |
| `sun_path` over 108 bytes | `FCAPI` chroots socat into the jail root (`chroot-early`) and connects to `/run/firecracker.socket` | See the Decision Log. | 3 |
| Line limit of the host client | 8 KiB per line; 1 MiB per HTTP body in the suite's helper | `READY`, `OK` and `ERR` are short fixed forms; Firecracker's bodies are a few KiB. The agent discards what a client writes on a lease, so the lease buffers nothing. | 3 (suite); `vesperd`'s own limits in 4 |
| Bridge dial; `CONNECT` and preface writes; greeting, `OK` and `ERR` reads (`vesperd`) | 5 s each | The agent greets as soon as socat forks its shell (the suite's control handshakes take 1 ms) and answers `FCAPI` after a few `stat` calls. 5 s covers a loaded System VM; past it, a client whose connection waits in the agent's backlog (at `max-children`) is closed with no bytes rather than held. | 4 |
| `READY` read | 30 s | `READY` came 0.4 s after start, 1.9 s on the first launch after boot, which builds the kernel and initramfs (suite `vesperd_starts`); slice 3 measured 1.4 s for the build. `LAUNCHAPI` also waits for the lock while another lease tears down (3 s grace). | 4 |
| Line limit for greeting, `OK`, `READY` and `ERR` | 1 KiB | The longest is `READY` with a 64-character id, about 120 bytes. | 4 |
| Relay read buffer | 8 KiB per direction | The size `copy_bidirectional` uses for `vesper-proxy`; Firecracker's request bodies are at most 51200 bytes. There is no total byte limit; a 4 MiB transfer passes in the unit tests. | 4 |
| Idle timeout | 120 s | Firecracker has none (Surprises), so any value is a difference (CE-0007); a client that reconnects is unaffected. Each open connection holds an agent shell and a socat in the System VM; 120 s frees them after a client stops using them, and is above common HTTP client idle pools (Go's 90 s). Measured: closed after 120000 ms with no bytes. | 4 |
| Per-request ceiling | 120 s | Slice 4: `PUT /snapshot/create` of a 512 MiB guest that touched little memory, on tmpfs, 223 ms. Slice 5, jails on the data disk, guest memory dirtied with `vesper_fill_mib`, default 640 MiB `memory.max`: 256 MiB guest 156 to 309 ms; 512 MiB guest with 400 MiB dirtied 1.24 s; 576/464 2.44 s; 608/480 1.32 s; 640/512 1.25 to 1.75 s. 120 s is about 50 times the slowest; kept (Decision Log). | 4; measured again in 5 |
| Connection cap | 16 per `vesperd` | Decision Log. Three default-size microVMs (the most the System VM holds at 640 MiB) need 3 × (16 + 1 lease) = 51 agent handlers, under `max-children` 64. | 4 |
| Reused-id wait (`ERR busy` retries) | 15 s, retried every 1 s, then refuse | ADR §4: at least one reaper interval (10 s), plus a teardown (3 s grace and removal). Measured: refused after 15.2 s while the first `vesperd` kept serving. | 4 |
| Teardown wait after closing the lease | 20 s | A teardown took 8 to 56 ms through `vesperd` and about 0.5 s in slice 3; 20 s is the slice 3 suite's `TEARDOWN_TIMEOUT`. Past it `vesperd` exits 1 with a message. | 4 |
| `stage` path and name forms | absolute paths of `[A-Za-z0-9._+,@=-]` between single slashes, no `.` or `..`, at most 1024 bytes; store names of the same characters, 1 to 128, no leading dot | Decision Log. | 5 |
| `stage=` tokens per `LAUNCHAPI` | 16 | A snapshot needs two files plus its drives; 16 keeps the `LAUNCHAPI` line under about 33 KiB. | 5 |
| Store free-space reserve | the System VM's `MemTotal` (2068926464 bytes today); `test-reserve` overrides it until reboot | Decision Log. | 5 |
| `READY` wait with `--stage` | 30 s + 120 s | Decision Log. | 5 |
| `vesperd stage` and `vesperd store` read timeout | 600 s | The agent replies after the copy; the slowest measured copy (256 MiB) took under 1 s. | 5 |
| `DATA status` reply | at most 4 lines of 1 KiB, 5 s per read | `vesper-data status` prints one line. | 5 |
| Default `memory.max`, measured | 640 MiB kept | Decision Log; measurements in Surprises and Outcomes. Per microVM, outside the cgroup: about 2.9 MiB RSS (0.6 MiB PSS) for the lease (socat and shell) and about 3.8 MiB RSS (1.2 MiB PSS) per open connection (socat and the chrooted socat); on the data disk, 44 MB of kernel and initramfs copies per jail (TD-056). | 5 |

## Outcomes & Retrospective

Plan complete, 2026-11-12. All six slices merged (PRs #169, #171, #176, #177, #179, #181, #182), each with green
CI. A Firecracker client can point at a `vesperd --backend systemvm` socket on macOS, configure, boot, pause,
snapshot, load and stop a stock Firecracker v1.16.0 microVM, and get Firecracker's own responses. The behaviour matrix
is now an executed oracle: 156 of its 173 cases match on Backend A, 14 need a network interface (CE-0010, TD-058) and
3 are x86_64-only; the model matches all applicable cases in CI. Retrospective: the matrix had been written from
reading source and was wrong in 35 places, which only running the real binary showed (D-026); the guest image
lacked the balloon and virtio-mem drivers, which no earlier suite exercised. Open follow-ups: TD-053, TD-055 to
TD-058, the Linux reference run (D-024).

Slice 5, 2026-11-12, MacBook Pro M3 Pro, one System VM boot sequence from `make systemvm-overlay` on this branch,
with `VESPER_SHARES=$PWD/run/vesperd-share` (`make data-disk-suite` reboots four times and leaves the last boot
running; every other suite ran on that last boot):

| Check | Result |
|---|---|
| `cargo test` | 302 passed, 0 failed: 162 library unit tests (4 new in `control::stage`), `api_contract` 122, `contract_golden` 1, `vesperd_socket` 6, `vesperd_systemvm` 11 (5 new); contract, matrix, entitlement, `compatexctl`, `check_docs.py` and `check_structure.py` (no errors) pass |
| `make data-disk-suite` | 10/10, including the new `vesperd_store_survives_jails_swept` |
| `make systemvm-launchapi-suite` | 20/20 |
| `make systemvm-vesperd-suite` | 18/18 |
| `make systemvm-snapshot-suite` (new) | 23/23 (also 23/23 on an earlier boot); the 640 MiB create took 1.2 to 1.7 s |
| `make ctr-conformance` | 74/74 |
| `make docker-conformance` | 26/26 |
| `make systemvm-parity-baseline PARITY_FLAGS=--publish` | passed; `docs/specs/backend-a-parity-baseline.json` lists CE-0008; `make parity-baseline-check` valid |

Gaps left by slice 5: `run_snapshot_suite` has no fake-agent unit test; the TOCTOU between a jail-side check and its
open is closed only by uid 20000's rights, and whether Firecracker's seccomp filter lets it create symlinks is not
checked; share files are read as root; the reserve is not checked before Firecracker's own snapshot writes and does
not see the macOS volume's free space (TD-053); a share path with a space or a colon cannot be staged; the
create-over-link refusal (CE-0008) has no owner approval yet; no compat exception records that request paths are jail
paths (the ADR lists it among the exceptions before a parity claim); `EXDEV` across mounts was not provoked.

Slice 4, 2026-11-12, MacBook Pro M3 Pro, macOS 27.0.1, one System VM boot from `make systemvm-overlay` (unchanged
in slice 4) on this branch:

| Check | Result |
|---|---|
| `cargo test` | 293 passed, 0 failed: 158 library unit tests, `api_contract` 122, `contract_golden` 1, `vesperd_socket` 6, `vesperd_systemvm` 6; no build warnings. `make contract-check`, `make contract-test`, both matrix checks, `compatexctl`, `check_docs.py` and `check_structure.py` (no errors) pass |
| `cargo test --lib proxy` | 16 tests: the 9 existing plus the preface and byte-for-byte pass-through, refused `FCAPI`, overlong and missing greeting, idle and ceiling through `run_server`, the connection cap, `claim_socket` with eight racing starts, and three `relay` tests (line bound, idle, ceiling, 4 MiB with half-close) |
| `cargo test --test vesperd_systemvm` | 6/6 (fake agent) |
| `cargo test --test vesperd_socket` | 6/6, unchanged |
| `make systemvm-vesperd-suite` (new) | 18/18 (second and third runs; the first found the stdout panic, Surprises) |
| `make systemvm-launchapi-suite` | 20/20 |
| `make ctr-conformance` | 74/74 |
| `make docker-up && make docker-conformance` | 26/26, after the vesperd suite on the same boot |
| `make systemvm-parity-baseline PARITY_FLAGS=--publish` | passed (24 steps); `docs/specs/backend-a-parity-baseline.json` now lists CE-0007; `make parity-baseline-check` valid |

Gaps left by slice 4: the per-request ceiling is set from a guest that touches little of its memory, on tmpfs, up to
512 MiB (768 MiB does not fit); `vesperd`'s own connection cap was exercised only against the fake upstream, because
Firecracker closes connections past its 10 at once; the agent's `max-children` (64) is shared by every `vesperd`, so
more than three microVMs with full connection pools can make new connections wait and then be closed; the version
mismatch and the `ERR busy` retry are proven only against the fake agent; `run_vesperd_suite` has no fake-agent unit
test (the slice 3 suite has one); the `DATA status` check is slice 5 (done there).

Slice 3, 2026-11-12, MacBook Pro M3 Pro, macOS 27.0, System VM from `make systemvm-overlay` on this branch:

| Suite | Result |
|---|---|
| `make systemvm-launchapi-suite` (new) | 20/20, twice (before and after the other suites) |
| `make systemvm-microvm-suite` | 13/13 |
| `make systemvm-shim-suite` | 22/22 |
| `make ctr-conformance` | 74/74 (shim 22, bundle 15, restart 18, exec 19) |
| `make systemvm-control-suite` | 4/4 |
| `make docker-conformance` | 26/26, `passed=true` |
| `make systemvm-services-suite` | 10/10 on a fresh boot. Run after `ctr-conformance` on the same boot it gave 9/10: `containerd_logs_capture_startup_and_recovery` found one startup line in the last 200 lines of `containerd.log`, because the restart suite's two containerd restarts and their output push the boot-time startup line out of that window. The suite expects a fresh boot; slice 3 does not touch containerd. |

Gaps left by slice 3: the agent listener's behaviour at `max-children` was not exercised; `vesper-microvm prepare`
is serialised only among `LAUNCHAPI` calls, not against a concurrent shim launch; the serial console log of a
`vesperd` microVM (`/var/log/vesperd/<id>.log`, tmpfs) is unbounded and kept after teardown (TD-055); the cost of
the 1 s lease poll with many leases is unmeasured. The ADR's new tech-debt rows for memory shared by both owners,
no reattach and no network interfaces on `vesperd` microVMs are not yet in the tracker.

## Context and Orientation

The System VM is an Alpine 3.22 Linux guest that `vesper-vmlauncher` boots on macOS. Its root is a tmpfs; the
state directories live on a data disk (GH #164). Inside it, OpenRC supervises `socat VSOCK-LISTEN:1024,fork
EXEC:/usr/local/bin/vesper-agent`: one shell per vsock connection, speaking a line protocol (version 1) documented
at the top of `systemvm/overlay/usr/local/bin/vesper-agent` and in `systemvm/overlay/README.md`. macOS reaches it
through the launcher's bridge socket `run/systemvm/vsock/3.sock` with a `CONNECT 1024` preface
(`src/control.rs:connect`).

Backend A today serves the containerd shim: `vesper-microvm launch ID` boots a microVM from `--config-file` as uid
10000 under `/srv/jailer`, in cgroup `/sys/fs/cgroup/firecracker/ID`, and finds its process with `fcpid()`.

Terms: a jail is the jailer's chroot `<chroot base>/firecracker/<id>/root`. A lease is the `LAUNCHAPI` connection
after `READY`; the lease shell (the holder) is the agent process serving it. A record is
`/run/vesper/vesperd/leases/<id>`, one line `holder=<pid>:<start> conn=<pid>:<start> fc=<pid>:<start>`, where a
start is field 22 of `/proc/<pid>/stat`, so a reused pid is not mistaken for the original process. The reaper
stops any microVM whose holder no longer runs with its recorded start.

Guides: ADR 0007 (all of it, especially §2, §4, §6 and the implementation table),
[systemvm-bringup.md](../../process/systemvm-bringup.md) (Control channel, Backend A, the data disk),
[SECURITY.md](../../SECURITY.md), `systemvm/overlay/README.md`, `src/README.md`, `src/bin/README.md`,
[TESTING.md](../../TESTING.md). Upstream facts are from Firecracker `d83d72b710361a10294480131377b1b00b163af8`:
`docs/jailer.md` and `src/jailer/src/env.rs` (`firecracker.pid` written in the jail root before the exec,
`--parent-cgroup` creates `<cgroup base>/<parent>/<id>` when `--cgroup` is given, ids of letters, digits and
hyphens up to 64), `src/firecracker/src/api_server_adapter.rs` (without `--config-file` the microVM is built from
API requests), `docs/getting-started.md` and the vendored swagger (`PUT /machine-config`, `PUT /boot-source` with
`kernel_image_path`, `initrd_path` and `boot_args`, `PUT /actions` `InstanceStart`).

## Plan of Work

Slice 3, in the order it was built:

1. `systemvm/overlay/usr/local/bin/vesper-vesperd`: `launch ID [CEILING]` validates the id and ceiling, takes the
   `flock` on `/run/vesper/vesperd/lock`, runs a reaper pass, refuses an id that still has a record, jail or
   cgroup (`ERR busy <id>`), runs `vesper-microvm prepare`, writes the record naming itself and its socat, sets the
   `EXIT`, `HUP`, `INT` and `TERM` traps, creates the jail with `/Image` and `/initrd.gz`, starts `jailer --uid
   20000 --gid 20000 --chroot-base-dir /srv/vesperd --parent-cgroup vesperd --cgroup-version 2 --cgroup
   pids.max=256 --cgroup memory.max=<ceiling> -- --api-sock /run/firecracker.socket` with the lock's descriptor
   closed, releases the lock, waits for the socket, reads and checks the pid file, records Firecracker's pid and
   start, prints `READY`, and polls the lease. The trap tears down under the lock only while the record is still
   its own. `connect ID` checks the id and the record and `exec`s the chrooted socat. `reap` and `reap-loop`
   implement the reaper; `status`, `isolation`, `list`, `log`, `kill-holder`, `kill-fc` and the `test-*` hooks
   serve the suite.
2. `vesper-agent`: `LAUNCHAPI` and `FCAPI` `exec` the helper; `VESPERD` joins the buffered tools.
   `etc/init.d/vesper-agent`: `max-children=64`. `etc/init.d/vesper-vesperd-reaper` plus its default-runlevel link.
3. `vesper-microvm`: `fcpid()` adds `-u "$JUID"`. `vesper-ctr leftovers` adds `-u 10000`. `vesper-fcparity`:
   `valid_id` in `api` and `conformance`.
4. `src/control/launchapi.rs`: `Session::launch_api`, `fcapi`, `http`, `wait_closed`, `vesperd`; `parse_ready`,
   `valid_microvm_id`. `src/control/suites/launchapi.rs`: `run_launchapi_suite`. Both registered in
   `scripts/harness/check_structure.py`. `src/bin/systemvmctl.rs`: `control vesperd` and `control
   launchapi-suite`, and a `print_steps` helper. `Makefile`: `systemvm-launchapi-suite`.
5. Documentation listed under Validation and Acceptance.

Slice 4, in the order it was built:

1. `src/proxy/relay.rs`: `Preface`, `Bounds`, `read_line_bounded`, `write_line`, `run_preface`, and `bounded`, the
   relay with one task per direction (`try_join!`), a shared clock of which side spoke last, and a watchdog.
   `src/proxy.rs`: `ServerConfig.preface` and `.bounds` (defaults keep the old relay), `serve` (the accept loop with
   the semaphore), `open_upstream`, `claim_socket` (moved from `vesperd.rs`). Registered in `check_structure.py`.
2. `src/bin/vesperd.rs`: `--backend model|systemvm` and the System VM flags; `bind_owner_only`, shared by both
   backends. `src/bin/vesperd/systemvm.rs`: `Options::from_args`, `relay_config`, `launch` (with the `ERR busy`
   retry), `run` (claim, lease, version check, bind, `select!` over `serve`, the lease and the signals).
3. `src/control/launchapi.rs`: `http_request`, `read_http_response`, `parse_http_response`, `Session::http_raw`.
   `src/control/suites/vesperd.rs`: `run_vesperd_suite`. `systemvmctl control vesperd-suite`;
   `make systemvm-vesperd-suite`.
4. CE-0007; the parity baseline republished; the documentation listed under Validation and Acceptance.

Slice 5, in the order it was built:

1. `vesper-vesperd`: `BASE` and `STORE` under `/var/lib/vesper/state/vesperd`; `data_ok` and `data_dirs` (`LAUNCHAPI`
   refuses without the data disk); `setup-user` and `user_ok`; the staging block (`valid_path`, `valid_name`,
   `share_file`, `store_file`, `jail_guard`, `jail_walk`, `jail-stat`, `jail-read`, `jail-write`, `copy_one`,
   `stage_forms`, `stage`, `launch_stage`, `reserve_check`, the store lock); `store-list`, `store-rm`, `sweep`,
   `files`, `memory`, `test-plant`, `test-reserve`; `launch` parses `[ceiling] [stage=…]…` and stages into the fresh
   jail between the kernel copy and the jailer. `vesper-data mount` runs `sweep`; the reaper service's `start_pre`
   runs `setup-user`; `vesper-microvm-init` gains `vesper_fill_mib` and `vesper_mark`; `vesper-agent`'s header.
2. `src/control/stage.rs` (registered in `check_structure.py`): the forms and `Session::stage`, `store_list`,
   `store_rm`. `src/bin/vesperd.rs`: the `stage` and `store` subcommands, `--stage`, global connection flags.
   `src/bin/vesperd/stage.rs`: the subcommands. `src/bin/vesperd/systemvm.rs`: `bridge_and_port`, `check_data_disk`,
   `launch_request`, the longer `READY` wait.
3. `tests/vesperd_systemvm.rs`: the fake agent answers `DATA status` and `VESPERD`, and records every request.
4. `src/control/suites/snapshot.rs` (registered), `systemvmctl control snapshot-suite`, `make systemvm-up-snapshot`
   and `make systemvm-snapshot-suite`; the vesperd suite's helpers became `pub(super)` with `spawn_with` and
   `start_with`. `scripts/systemvm/data-disk-suite.sh`: step `vesperd_store_survives_jails_swept`.
5. CE-0008, CE-0007's text, the parity baseline, and the documents listed under Validation and Acceptance.

### Slice 6: behaviour-matrix replay

The matrix records 173 cases as a description, a status and assertions, with no requests, and no test ran them. Slice
6 gives each case an executable script and runs it on both backends with one oracle:

1. `docs/specs/firecracker-api-behavior-replay.yaml`: per case `setup` steps (each must return 2xx) and the
   `request` under test; macros (`@boot`, `@boot_settled`, `@net0`, …); `WAIT <ms>`; `{root}`, `{pad:N}` and
   `{rep:N:TEXT}`; the files every case root holds and a `snapshot_fixture`. Paths are `{root}/…`: a temporary
   directory on the model, empty on Backend A, so they are jail paths there (CE-0009). The root is removed from
   response bodies before judging.
2. `src/api_replay.rs` (layer: `api_behavior_matrix` only): loads and validates the scripts (exact coverage, macro
   depth, the request is the case's route), expands them, judges a response (status; empty body; `fault_message`
   exact, plus `code` on the model; JSON-pointer assertions), and `replay()` over a backend callback.
3. `tests/api_replay.rs`: every case on a fresh model router in CI; all must match (the 3 x86_64 cases are not
   applicable on an arm64 runner, the 2 aarch64 cases on x86_64).
4. `src/control/suites/replay.rs` (matrix-agnostic): writes the files into a new share directory per run (a removed
   and rewritten share file read 0 bytes in the System VM), makes the snapshot fixture once and copies it into the
   store, then runs each case in its own `vesperd --backend systemvm` with the files and, when marked, the
   fixture staged; one connection per step; SIGTERM and the jail must be gone. `src/bin/systemvmctl/replay.rs` joins
   it to the scripts and writes the report; `--publish` writes the committed one with the baseline metadata.
5. The microVM guest now loads `virtio_balloon` and `virtio_mem`: without them Firecracker's balloon and
   memory-hotplug devices never activate ("Device not activated yet", "Device is not active").

Triage of the first full run (Backend A, M3 Pro, `d83d72b`), 173 cases: 123 match, 3 not applicable (x86_64), 47
differ.

- 14 need a network interface (CE-0010, TD-058): 12 setup failures at `PUT /network-interfaces/eth0` and the 2
  cases whose own request is one (`put_valid_preboot`; `mtu_below_minimum_preboot`, whose MTU check runs after the
  tap is opened).
- 33 show the matrix is wrong: Backend A returns the text or rule in the pinned source (`run/GH-166/firecracker` at
  `d83d72b`), and the captures were written from reading source, never from a running Firecracker.
  - Missing prefixes from `VmmActionError` (`src/vmm/src/rpc_interface.rs`, `displaydoc`): `MMDS error: ` (4 cases),
    `MMMDS config error: ` (3; the typo is Firecracker's), `MMDS limit exceeded error: ` (2), `Machine config
    error: ` (2), `Metrics error: ` (1), `Start microvm error: ` (1).
  - Different text: snapshot load after configuring a boot resource is "Load snapshot error: Loading a microVM
    snapshot not allowed after configuring boot-specific resources." (5); snapshot create while running is "Create
    snapshot error: Cannot save the microVM state: Operation not allowed: save/restore unavailable while running";
    the memory-hotplug validation texts (3); the balloon runtime limit quotes the requested size ("less than
    256MiB" for a 256 MiB request; `balloon/device.rs` `update_size`).
  - Different rules: `PUT /cpu-config` does not block a snapshot load (`set_custom_cpu_template` leaves `boot_path`
    false); `PUT /logger`, `/metrics` and `/serial` after boot return "not supported after starting the microVM"
    (3); `PUT /vsock` accepts `guest_cid` 2; `GET /vm/config` writes `"cpu-config":null`, `"logger":null` and
    `"metrics":null` where the matrix expects the key absent (5; `serial` is absent, as the matrix says).
- Script fixes found on the way: the MMDS store limit counts the re-serialized length (`to_vec` in
  `mmds/data_store.rs`), so `PUT /mmds#oversized_store` sends `1e2` 12000 times (48 kB under the 51200-byte HTTP
  limit, about 72 kB as `100.0`); a 128 MiB guest inflates its balloon only to about 52 MiB, so
  `enabled_stats_runtime` uses 256 MiB; MAC addresses are lowercase, as Firecracker prints them.

## Concrete Steps

From the repository root on an Apple-silicon Mac (M3 or later):

    make systemvm-overlay && bash scripts/systemvm/generate-launch-config.sh && make systemvm-up
    make systemvm-launchapi-suite
    # ok   launch: READY vesper-launchapi pid=… firecracker_version=1.16.0
    # ok   get_root_running: GET / -> 200 {"id":"vesper-launchapi","state":"Running",…}
    # …
    # launchapi suite passed (report: run/systemvm/launchapi-report.json)
    make systemvm-microvm-suite systemvm-shim-suite systemvm-control-suite systemvm-services-suite
    make ctr-conformance
    make docker-up && make docker-conformance
    make systemvm-down

Slice 4:

    make systemvm-up
    make systemvm-vesperd-suite
    # ok   vesperd_starts: vesperd listening on run/vesperd-suite/main.sock (backend systemvm, microVM vesper-vesperd, Firecracker 1.16.0 pid …)
    # …
    # vesperd suite passed (report: run/systemvm/vesperd-report.json)
    make systemvm-launchapi-suite ctr-conformance
    make docker-up && make docker-conformance

Slice 5 (the share must be in the launch config; `make data-disk-suite` keeps `VESPER_SHARES` for its reboots):

    export VESPER_SHARES="$PWD/run/vesperd-share"
    make systemvm-up-snapshot && make docker-up
    make data-disk-suite               # includes vesperd_store_survives_jails_swept
    make systemvm-snapshot-suite
    # ok   guest_state_survives: the guest kept token … in memory and counts on: tick 3 before the snapshot, …
    # snapshot suite passed (report: run/systemvm/snapshot-report.json)
    make systemvm-launchapi-suite systemvm-vesperd-suite ctr-conformance docker-conformance

Single commands for diagnosis: `cargo run --bin systemvmctl -- control vesperd list|status ID|isolation ID|log ID
|reap|files ID|memory ID|store-list`, `cargo run --bin vesperd -- store list`.

## Validation and Acceptance

Slice 3 is accepted when:

- `cargo test control` passes, including `control::launchapi::tests` and `control::suites::launchapi::tests`,
  which run the client and the whole checklist against fake agents; `cargo test` passes.
- `make systemvm-launchapi-suite` reports 20 of 20 steps ok and `passed: true` in
  `run/systemvm/launchapi-report.json`: launch with the pinned version, a duplicate id refused, `GET /` before
  boot, boot through the API, `GET /` `Running`, the guest's boot marker, isolation (uid 20000, cgroup
  `/vesperd/<id>`, seccomp mode 2, chroot), `FCAPI` and `FCPARITY` refusing bad ids, a shim microVM with the same id
  kept apart, the three teardowns, the reused id, the two reaper cases, the container `AF_VSOCK` check, and
  nothing left.
- These still pass on the same image: `make systemvm-microvm-suite`, `make systemvm-shim-suite`, `make
  ctr-conformance` (74/74), `make systemvm-control-suite`, `make systemvm-services-suite`, `make
  docker-conformance`.
- `python3 harness/check_docs.py` reports 0 errors; `python3 scripts/harness/check_structure.py --root .` reports
  no errors.

Slice 4 is accepted when `cargo test` passes (including `proxy::` and `tests/vesperd_systemvm.rs`), `make
systemvm-vesperd-suite` reports 18 of 18 and `passed: true` in `run/systemvm/vesperd-report.json`, the slice 3
suites above still pass, `compatexctl` accepts CE-0007, `make parity-baseline-check` is valid, and both harness
checks report no errors. The owning documents are `src/README.md`, `src/bin/README.md`, `docs/SECURITY.md`,
`docs/TESTING.md`, `docs/process/systemvm-bringup.md`, `ARCHITECTURE.md`, design-v0 and the tech-debt rows TD-045 and
TD-048.

Slice 5 is accepted when `cargo test` passes (including `control::stage` and the 11 tests of
`tests/vesperd_systemvm.rs`), `make systemvm-snapshot-suite` reports 23 of 23 and `passed: true` in
`run/systemvm/snapshot-report.json`, `make data-disk-suite` passes with `vesperd_store_survives_jails_swept`, the slice
3 and 4 suites, `make ctr-conformance` (74/74) and `make docker-conformance` still pass on the same boot,
`compatexctl` accepts CE-0008, the parity baseline is republished and valid, and both harness checks report no
errors. The owning documents are those of slice 4, `systemvm/overlay/README.md`, `scripts/systemvm/README.md` and the
tech-debt rows TD-025, TD-045, TD-053 and TD-056.

## Idempotence and Recovery

Every suite step starts by ageing and reaping leftovers of its own ids, so the suite can be rerun after a failure.
`vesper-vesperd reap` is safe to run at any time: it keeps every microVM whose holder runs. A stuck microVM is
removed with `control vesperd kill-holder ID` and the next reaper pass, or with `test-age ID 3600` and `reap` for a
jail without a record. `make systemvm-down` removes everything, because the slice 3 jails, records and logs are in
tmpfs. Rolling back slice 3 is reverting its commit and rebuilding the overlay; nothing persistent changes.

Since slice 5 the jails and the store are on the data disk. A reboot (`make systemvm-down && make systemvm-up`)
removes every jail; store files stay until `vesperd store rm NAME`. The snapshot suite removes its own store names
at start and end, and clears its reserve override (`test-reserve clear`; a reboot clears it too). Rolling back slice
5 leaves `/var/lib/vesper/state/vesperd` on the disk; remove it with `vesperd store rm` for each file before
reverting, or delete the directory from the System VM.

## Artifacts and Notes

The record of a running microVM and the reaper's log line for a SIGKILLed holder:

    LEASE vesper-launchapi holder=3069:2265 conn=3068:2265 fc=3272:2280
    vesperd-reaper: REAPED vesper-launchapi-reap reason=holder-gone holder=3839 CLEAN vesper-launchapi-reap

The container check (step `default_container_cannot_open_vsock`):

    VSOCK default=denied cid=1 detail=… E socket(40, 1, 0): Operation not permitted
    VSOCK unconfined=allowed cid=1 detail=… E connect(6, AF=40 cid:1 port:1024, 16): Connection reset by peer
    VSOCK unconfined=allowed cid=3 detail=… E connect(6, AF=40 cid:3 port:1024, 16): No such device

## Interfaces and Dependencies

Agent verbs (protocol version 1, no version change; new verbs only):

    LAUNCHAPI <id> [ceiling]  -> READY <id> pid=<n> firecracker_version=<v> | ERR <reason> (then close)
    FCAPI <id>                -> OK, then raw bytes to the API socket | ERR <reason> (then close)
    VESPERD <args…>           -> VESPERD <rc> <count>, then <count> lines of `vesper-vesperd <args>`

`<id>` is 1 to 64 letters, digits and hyphens; `[ceiling]` is `memory.max` in bytes. `ERR` reasons from
`LAUNCHAPI`: `usage: …`, `bad id`, `bad ceiling`, `ceiling out of range <min>..<max>`, `firecracker missing`,
`state directories`, `firecracker version unknown`, `own start time`, `busy <id>`, `prepare: …`, `record`, `jail`,
`stage kernel`, `start failed: <log tail>`, `no pid file`, `pid file names no Firecracker for <id>`, `firecracker
exited`. A failure after the record is written tears down what was made before the connection closes. From `FCAPI`: `bad id`, `no vesperd microVM <id>`,
`<id> not ready`, `<id> has no API socket`, `usage: …`.

Rust (`vesper::control`): `Ready { id, pid, firecracker_version }`, `HttpResponse { status, body }`,
`parse_ready(line, id) -> Result<Ready, String>`, `valid_microvm_id(id) -> bool`, `Session::launch_api(id, ceiling:
Option<u64>, timeout) -> Result<Ready, String>`, `Session::fcapi(id)`, `Session::http(method, path, body) ->
Result<HttpResponse, String>`, `Session::wait_closed(timeout) -> Result<Duration, String>`,
`Session::vesperd(args)`, `run_launchapi_suite(endpoint, id, expected_version) -> SuiteReport`.

Slice 4 (Rust): `vesper::proxy::{Preface, Bounds, RelayEnd, Upstream, serve, open_upstream, claim_socket,
read_line_bounded, write_line}`; `ServerConfig { …, preface: Option<Preface>, bounds: Bounds }`;
`vesper::control::{http_request, read_http_response, parse_http_response, run_vesperd_suite, VesperdRun}` and
`Session::http_raw`. Command line: `vesperd --backend systemvm --id ID [--socket PATH] [--memory-max BYTES]
[--launch-config PATH | --bridge PATH] [--agent-port N]`; `systemvmctl control vesperd-suite [--report] [--id]
[--swagger] [--vesperd] [--workdir]`. No new crates; no guest change. No new crates, no
new guest packages: the guest side uses busybox `sh`, `flock`, `setsid`, `timeout`, `pgrep -u`, `stat`, `touch -d`
and the pinned socat 1.8.1.3.

Slice 5 interfaces, exactly as built:

    vesperd --backend systemvm --id ID [--stage SRC:DEST]...   SRC share:/PATH | store:NAME; DEST /PATH (at most 16)
    vesperd stage [--id ID] SRC DEST                             SRC share:/PATH | store:NAME | jail:/PATH;
                                                                 DEST /PATH | store:NAME
    vesperd store list                                           STORE NAME bytes=N links=L …, FREE avail=N reserve=N
    vesperd store rm NAME                                        REMOVED NAME
    exit codes of stage and store: 0 done; 1 refused or failed in the System VM, or unreachable; 2 a bad form,
    refused before connecting (`vesperd stage: <reason>` on stderr)

    LAUNCHAPI <id> [ceiling] [stage=SRC:DEST]...  -> as slice 3; new ERR reasons: `data disk not in use`, `no
                                                     vesperd-jail user`, `at most 16 stage= tokens`, `usage: …`,
                                                     `stage SRC:DEST: <reason>`
    VESPERD stage ID|- SRC DEST  -> STAGED SRC DEST copy bytes=N | ERR <reason>
    VESPERD store-list           -> STORE NAME bytes=N links=L lines, then FREE avail=N reserve=N
    VESPERD store-rm NAME        -> REMOVED NAME | ERR no store file NAME
    VESPERD sweep|setup-user|files ID|memory ID|test-plant ID|test-reserve BYTES|clear

`stage` reasons, each prefixed `ERR`: `usage: stage ID|- SRC DEST`, `bad SRC …`, `bad DEST …`, `store to store is
not a stage`, `jail to jail is not a stage`, `bad id`, `no vesperd microVM ID`, `no jail for ID`, `no vesperd-jail
user`, `data disk not in use`, `PATH is not inside a configured share`, `PATH: symlink at PATH`, `PATH: … is not a
directory`, `PATH is not a regular file`, `no store file NAME`, `symlink at /P` (jail side), `/P is not a regular
file`, `/P exists`, `/P: permission denied for uid 20000`, `store:NAME exists`, `reserve: copying N bytes would
leave A of the R-byte free-space reserve (avail V)`, `copied N of M bytes from SRC`.

Rust: `vesper::control::{StageSource, StageDest, StageSpec, StoreEntry, valid_stage_path, valid_store_name,
check_pair, needs_id, parse_store_list, data_disk_in_use, MAX_STAGES, MAX_STAGE_PATH, MAX_STORE_NAME}`,
`Session::{stage, store_list, store_rm}`, `run_snapshot_suite`, `SnapshotRun`, `prepare_workdir`. No new crates, no
new guest packages: the guest side adds busybox `su`, `addgroup`, `adduser`, `head -c`, `stat -c`, `find -xdev`,
`ln`, `df -k` and `set -C`, all in Alpine 3.22's busybox 1.37.0.

Tech-debt rows opened with this plan (ADR 0007 "on acceptance"): TD-056 (no shared memory budget across the two owners), TD-057 (no reattach). The third the ADR names, no network interfaces on `vesperd` microVMs, is covered by TD-032 (no microVM has a network device).
