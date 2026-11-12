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
- [ ] Slice 3: commit, push, CI green, GH #156 comment with the SHA and both verifications (the lead commits).
- [x] (2026-11-12) Slice 4: `proxy::serve`, `open_upstream`, `claim_socket` (moved from `vesperd`) and
      `src/proxy/relay.rs` (`Preface`, `Bounds`, the bounded relay); `vesperd --backend systemvm` in
      `src/bin/vesperd/systemvm.rs` (lease, version check, reused-id wait, signals); unit tests against a fake
      upstream and process tests against a fake agent (`tests/vesperd_systemvm.rs`).
- [x] (2026-11-12) Slice 4: `make systemvm-vesperd-suite` (`src/control/suites/vesperd.rs`), 18/18 on an M3 Pro;
      CE-0007; the parity baseline republished with CE-0007 open; regression suites pass (Outcomes).
- [ ] Slice 4: commit, push, CI green, GH #156 comment with the SHA and both verifications (the lead commits).
- [ ] Slice 5: chroot base `/var/lib/vesper/state/vesperd/jailer`, boot sweep, `vesperd-jail` user, `--stage`,
      `vesperd stage`, `vesperd store`, the snapshot cycle, measured memory ceiling.
- [ ] Slice 6: behaviour-matrix replay through `vesperd` on Backend A.

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

## Decision Log

- Decision: slice 3 keeps the chroot base for `vesperd` microVMs on tmpfs, at `/srv/vesperd`, apart from the
  shim's `/srv/jailer`. Rationale: the ADR's implementation table puts the data-disk base and the boot sweep in
  slice 5, with staging and snapshots, which are what need it; a tmpfs base needs no boot sweep. The jailer was
  shown to work on the data disk (Surprises). Date/Author: 2026-11-12, slice 3 agent.
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
| Default `memory.max` (`LAUNCHAPI` ceiling) | 671088640 bytes (640 MiB), accepted range 128 MiB to the System VM's `MemTotal` | The shim's value, which leaves room for a 256 MiB guest and a full snapshot's memory file in a tmpfs jail. The ADR measures the default in slice 5. | 3; measured in 5 |
| `pids.max` | 256 | The shim's value. | 3 |
| `sun_path` over 108 bytes | `FCAPI` chroots socat into the jail root (`chroot-early`) and connects to `/run/firecracker.socket` | See the Decision Log. | 3 |
| Line limit of the host client | 8 KiB per line; 1 MiB per HTTP body in the suite's helper | `READY`, `OK` and `ERR` are short fixed forms; Firecracker's bodies are a few KiB. The agent discards what a client writes on a lease, so the lease buffers nothing. | 3 (suite); `vesperd`'s own limits in 4 |
| Bridge dial; `CONNECT` and preface writes; greeting, `OK` and `ERR` reads (`vesperd`) | 5 s each | The agent greets as soon as socat forks its shell (the suite's control handshakes take 1 ms) and answers `FCAPI` after a few `stat` calls. 5 s covers a loaded System VM; past it, a client whose connection waits in the agent's backlog (at `max-children`) is closed with no bytes rather than held. | 4 |
| `READY` read | 30 s | `READY` came 0.4 s after start, 1.9 s on the first launch after boot, which builds the kernel and initramfs (suite `vesperd_starts`); slice 3 measured 1.4 s for the build. `LAUNCHAPI` also waits for the lock while another lease tears down (3 s grace). | 4 |
| Line limit for greeting, `OK`, `READY` and `ERR` | 1 KiB | The longest is `READY` with a 64-character id, about 120 bytes. | 4 |
| Relay read buffer | 8 KiB per direction | The size `copy_bidirectional` uses for `vesper-proxy`; Firecracker's request bodies are at most 51200 bytes. There is no total byte limit; a 4 MiB transfer passes in the unit tests. | 4 |
| Idle timeout | 120 s | Firecracker has none (Surprises), so any value is a difference (CE-0007); a client that reconnects is unaffected. Each open connection holds an agent shell and a socat in the System VM; 120 s frees them after a client stops using them, and is above common HTTP client idle pools (Go's 90 s). Measured: closed after 120000 ms with no bytes. | 4 |
| Per-request ceiling | 120 s | The slowest measured synchronous operation is `PUT /snapshot/create` of a 512 MiB guest, 223 ms (Surprises); `InstanceStart` took 7 to 27 ms. 120 s is over 500 times that, to cover guests with dirty memory and the data-disk jails of slice 5, which were not measured. Slice 5 measures again at its memory ceiling. | 4; measured again in 5 |
| Connection cap | 16 per `vesperd` | Decision Log. Three default-size microVMs (the most the System VM holds at 640 MiB) need 3 × (16 + 1 lease) = 51 agent handlers, under `max-children` 64. | 4 |
| Reused-id wait (`ERR busy` retries) | 15 s, retried every 1 s, then refuse | ADR §4: at least one reaper interval (10 s), plus a teardown (3 s grace and removal). Measured: refused after 15.2 s while the first `vesperd` kept serving. | 4 |
| Teardown wait after closing the lease | 20 s | A teardown took 8 to 56 ms through `vesperd` and about 0.5 s in slice 3; 20 s is the slice 3 suite's `TEARDOWN_TIMEOUT`. Past it `vesperd` exits 1 with a message. | 4 |
| `stage` checks, store free-space reserve | not set | Staging and the store are slice 5. | 5 |

## Outcomes & Retrospective

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
test (the slice 3 suite has one); the `DATA status` check is slice 5.

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

Slices 5 and 6 follow the ADR's table; each gets its own section in this plan when it starts.

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

Single commands for diagnosis: `cargo run --bin systemvmctl -- control vesperd list|status ID|isolation ID|log ID
|reap`.

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

## Idempotence and Recovery

Every suite step starts by ageing and reaping leftovers of its own ids, so the suite can be rerun after a failure.
`vesper-vesperd reap` is safe to run at any time: it keeps every microVM whose holder runs. A stuck microVM is
removed with `control vesperd kill-holder ID` and the next reaper pass, or with `test-age ID 3600` and `reap` for a
jail without a record. `make systemvm-down` removes everything, because the slice 3 jails, records and logs are in
tmpfs. Rolling back slice 3 is reverting its commit and rebuilding the overlay; nothing persistent changes.

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

Tech-debt rows opened with this plan (ADR 0007 "on acceptance"): TD-056 (no shared memory budget across the two owners), TD-057 (no reattach). The third the ADR names, no network interfaces on `vesperd` microVMs, is covered by TD-032 (no microVM has a network device).
