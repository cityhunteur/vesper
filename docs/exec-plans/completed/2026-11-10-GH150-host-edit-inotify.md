# Deliver inotify events to containers for host-side edits on virtio-fs shares (GH #150 / GH #29 item 3 / TD-038)

This ExecPlan is a living document. The sections Progress, Surprises & Discoveries, Decision Log, and
Outcomes & Retrospective must be kept up to date as work proceeds. It is maintained in accordance with
docs/PLANS.md.

## Purpose / Big Picture

A developer on macOS edits a source file in a directory that is bind-mounted into a container
(`docker run -v /host/dir:/app`). A dev server in the container watches `/app` with inotify, the Linux kernel
interface that reports file changes to a process, and reloads. On Vesper today that reload never happens: the
file's contents and mtime are current the next time the container reads it, but no inotify event arrives, so
watchers must poll. This is the last open checklist item of GitHub #29 ("inotify/watch events propagate across
mount boundary") and is recorded as TD-038 in `docs/exec-plans/tech-debt-tracker.md`. The milestone log names it
as the one remaining Phase 4 gap.

After this plan, an edit made on macOS to a file inside a configured share produces an inotify event in a
container that watches that file or its directory, within a measured latency budget. The proof is
`make binds-suite`: the step `inotify_host_edits_not_propagated` is replaced by steps that fail before the change
and pass after it.

The parity unit is Docker UX parity for bind mounts (design-v0, Phase 4). It is not a Firecracker API route, so
no route in the pinned baseline (`083c4cf69281178acc910f7f857f88e69c51d3b5` in
`docs/specs/design-v0.baselines.lock.yaml`) changes.

Tracker: GitHub #150 carries this work. GitHub #29 stays closed; its item 3 was ticked on 2026-11-11 as a
qualified tick with the residuals in TD-038 (see "Council rework" under Outcomes & Retrospective).
Linear mirror: WOR-107 (linkback comment on GitHub #150).

## Progress

- [x] (2026-11-10) Read the WOR-39 plan, `vesper-share`, `vesper-agent`, the launcher and `binds-suite.sh`; drafted
  this plan.
- [x] (2026-11-10) The four decisions recorded in the Decision Log (cityhunteur: the recommended option for each).
- [x] (2026-11-10) Tracker: GitHub #150 created; this file renamed from `…-GH29-…` to `…-GH150-…`.
- [x] (2026-11-10) Milestone 0 (prototype) run on an M3 Pro, macOS 27.0, at `19045c4`; raw evidence in
  `run/TD-038/`; results in Surprises & Discoveries. The prototype tools are not promoted. Not measured: Go
  `fsnotify`, shares on external or network volumes, and the effect of the ctime change on host tools.
- [x] (2026-11-10) Milestone 1: guest sink. `vesper-share touch PATH...` and `vesper-share notify` (stdin stream),
  the `vesper-share-notify` OpenRC service on guest vsock port 2401, `probe` reports `notify=`. Proof:
  `make share-notify-selftest` (19 checks, also in the System VM Image workflow) and a booted VM
  (`run/GH-150/m1-runtime.txt`): through the agent and through port 2401 a touched file gave the watching
  container `IN_MODIFY` with the host mtime unchanged to the nanosecond; the share root, a read-only share, a
  missing path, `/etc/hostname` and a `..` path were skipped or rejected.
- [x] (2026-11-10) Milestone 2: host watcher. `launcher/Sources/vesper-vmlauncher/ShareWatcher.swift`
  (`ShareReplayFilter`, `ShareWatcher`), `--watch-dry-run`, `watch` in `SharedDirectory` (Rust and Swift),
  `PATH:nowatch`. Because host edits now produce events, the `binds-suite` step that asserted the limitation was
  replaced in the same change (five `inotify_*` steps; 14/14). Proof on an M3 Pro: `make share-watch-selftest`
  (11 checks), `cargo test systemvm` (14), `run/GH-150/m2-watchers.txt` (inotifywait, chokidar and watchexec
  react with no prototype bridge), `m2-latency.txt` (30 edits, 12 to 24 ms), `m2-burst-reconnect.txt` (2,000
  creates in a new directory: chokidar 2,000 in about 1 s; reconnect after the sink's handler was killed),
  `m2-summary-nowatch.txt`; `make docker-conformance`, `make ports-suite` and `make systemvm-shim-suite` pass
  with watching on.
- [x] (2026-11-10) Milestone 3, partly: the core `binds-suite` steps landed with Milestone 2 (see above).
- [x] (2026-11-10) Milestone 3, rest. `binds-suite` gained `inotify_host_delete_seen_on_parent`,
  `inotify_host_rename_seen`, `inotify_share_root_delete_not_propagated`, `inotify_burst_delivered` (500 files) and
  `inotify_nowatch_share_silent`; `make systemvm-up-shares` boots a third, `:nowatch` share; 19/19, six runs in a
  row once a stream-order flake in the older `readonly_share_enforced` step (stdout and stderr could swap; it
  failed one earlier run) was fixed.
  Unknowns measured (`run/GH-150/m3-unknowns.txt`): Go `fsnotify`, the `git status` cost of the ctime change, and
  disk images of three file systems. Bundle in `.symphony/runs/GH-150/current/`.
- [x] (2026-11-10) Milestone 4: owning docs, TD-038 narrowed, parity row for #29, milestone entry, GH #150
  updated, plan moved to completed. GH #29 item 3 is deliberately not ticked: that is a human decision.
  `docs/RELIABILITY.md`, which the Milestone 4 text lists, was not updated in this step.
- [x] (2026-11-11) Council rework. Four judges reviewed `19045c4..30cf8f0`; the runtime judge rejected. Fixed in
  the launcher (`ShareWatcher.swift`, `main.swift`), the sink (`vesper-share`, `vesper-share-notify`) and the
  tests: `binds-suite` 20 steps, `make share-watch-selftest` 20 checks, `make share-notify-selftest` 20 checks.
  Evidence: `run/GH-150/m5-stalled-sink.txt`, `run/GH-150/m5-suites.txt`. Docs corrected, `docs/RELIABILITY.md`
  now covers the watcher, TD-038 lists every residual, and TD-043 and TD-044 were added. Details under
  "Council rework" in Outcomes & Retrospective.

## Surprises & Discoveries

- Observation: bind mounts reach only containers that share the System VM's kernel (dockerd with runc). The
  Vesper containerd shim (`shim/`) runs each task in a nested Firecracker microVM from an ext4 root drive and
  has no bind-mount path into that microVM.
  Evidence: no bind or virtio-fs handling under `shim/internal/`; `scripts/systemvm/binds-suite.sh` drives
  `docker run -v` only.
  Consequence: this plan covers Docker bind mounts in the System VM. Shares inside microVM tasks are out of
  scope and stay **Unknown**; resolve by a separate design if Phase 3 tasks need host directories.
- Observation: setting a file's mtime from the guest produces `IN_MODIFY`, not only `IN_ATTRIB`. `touch -c -m -r P P`
  (set the mtime to the value the file already has) gives a watcher on the directory a real modify event;
  `chmod` to the current mode gives `IN_ATTRIB`; `stat` and `cat` give nothing.
  Evidence: `run/TD-038/m0-masks.txt` (busybox `inotifyd`: `c` for touch, `e` for chmod).
- Observation: `touch -r P P` keeps the host mtime to the nanosecond; `touch -d @<seconds>` truncates it
  (`…125.667200135` became `…125.000000000`). Either one changes the host ctime.
  Evidence: `run/TD-038/m0-ns.txt`; ctime values in `run/TD-038/m0-masks.txt`.
- Observation: every guest replay comes back to the host as an FSEvents event, and FSEvents flags are
  cumulative per file (a replay shows `InodeMetaMod|Modified`), so flags cannot tell a replay from an edit.
  Dropping events whose `(mtime_ns, size)` was already forwarded stops the loop: 5 s idle after edits produced
  no events.
  Evidence: `run/TD-038/m0-fsevents.raw`, `run/TD-038/m0-watchers-bridge.txt` (`drop` lines).
- Observation: with the prototype bridge, real watchers react without polling. `inotifywait -m -r` reports
  `MODIFY` for appends, atomic saves (temp file renamed over the target) and creates. chokidar 3 reports `change`,
  `add` and, after the parent directory is replayed, `unlink` for host renames and deletes. `watchexec` reruns
  its command. With no bridge all three stay silent.
  Evidence: `run/TD-038/m0-watchers-baseline.txt`, `run/TD-038/m0-watchers-bridge.txt`.
- Observation: latency from a host write to the `inotifywait` line read back on the host was 13 to 23 ms over
  30 edits (p50 19 to 20 ms), the same for FSEvents latency settings of 50 ms and 200 ms with the no-defer flag.
  Evidence: `run/TD-038/m0-latency.txt`.
- Observation: residuals that no guest operation removed. (1) The share's root directory rejects `touch` and
  `chmod` from the guest (`Operation not permitted`), so a delete or rename directly in the share root cannot be
  signalled; subdirectories work. (2) A host create arrives as `IN_MODIFY`, never `IN_CREATE`, and a host delete
  only as `IN_MODIFY` on the parent directory. Watchers that rescan a directory on change (chokidar) recover;
  watchers that add watches only on `IN_CREATE` (`inotifywait -r`) never watch a directory created on the host
  after they started: 2,000 files written into a new host directory gave chokidar 2,000 `add` events and
  `inotifywait` none. (3) No `IN_CLOSE_WRITE` is produced. (4) A watch on a single file does not follow an
  atomic save. (5) Read-only shares reject both operations (`Read-only file system`).
  Evidence: `run/TD-038/m0-masks.txt`, `run/TD-038/m0-burst.txt`.
- Observation: a burst of 2,000 host creates made FSEvents report `MustScanSubDirs` once and the bridge
  forwarded 1,769 file paths, yet chokidar still saw all 2,000 because the directory replay made it rescan. The
  System VM stayed healthy; the prototype drained the burst in about 22 s through `docker exec`, which is not
  the production path.
  Evidence: `run/TD-038/m0-burst.txt`.

- Observation: `URL.resolvingSymlinksInPath()` strips `/private` from a path, FSEvents does not, so a share
  under `/var` or `/tmp` matched nothing until the watcher used `realpath(3)`.
  Evidence: the first `make share-watch-selftest` run failed six checks; it passes with `realpath`.
- Observation: a container write to a share gives a watcher one extra `IN_MODIFY`: the write reaches the host,
  FSEvents reports it, and the guest replays it once. The replay's own echo is dropped.
  Evidence: `inotify_guest_writes_seen` in `run/GH-150/binds-report.json` (`n c w c`); `m2-burst-reconnect.txt`.
- Observation: two of the reconnect test attempts took the sink's supervisor down by mistake (a `pkill -f`
  pattern that also matched `supervise-daemon`, then a restart from inside a container's cgroup). They are test
  errors, kept in `m2-burst-reconnect.txt` with corrections. With only the handler killed, the watcher logged
  the failed write, reconnected (`connection 2`) and later edits arrived.

- Observation: Go `fsnotify` reports `WRITE` for a file on host create, append and atomic save, and `WRITE` for
  the directory on rename and delete; it never reports `CREATE`, `REMOVE` or `RENAME` for host changes.
  Evidence: `run/GH-150/m3-unknowns.txt`.
- Observation: the ctime change had no measurable `git status` cost. On a 2,000-file repository created inside
  a share, the first `git status` took 41 ms watched and 40 ms on the `:nowatch` share, later ones 12 ms, also
  after a checkout that rewrote all 2,000 files.
  Evidence: `run/GH-150/m3-unknowns.txt`.
- Observation: the watcher's dry-run self-test passes on APFS, case-sensitive APFS and exFAT disk images. No
  physical external disk or network volume was available, so those stay untested.
  Evidence: `run/GH-150/m3-unknowns.txt`.

## Decision Log

- Decision: option A, a host watcher plus guest replay.
  Rationale: low latency and no idle cost; polling grows with tree size; keeping the limitation is not parity.
  Date/Author: 2026-11-10, cityhunteur (agent recommendation accepted).
- Decision: the host watcher lives in `vesper-vmlauncher` (Swift).
  Rationale: FSEvents is native there, the launcher owns the shares and the vsock device, no new dependency
  and no new host-visible socket.
  Date/Author: 2026-11-10, cityhunteur.
- Decision: watching is on by default for read-write shares, with a per-share opt-out `PATH:nowatch`.
  Rationale: parity by default. Milestone 0 measured a host ctime change and no mtime or content change; the
  effect of the ctime change on host tools is not yet measured.
  Date/Author: 2026-11-10, cityhunteur.
- Decision: read-only shares get no host-edit events; this is a documented residual.
  Rationale: the alternative weakens the guarantee that no container can override `:ro`.
  Date/Author: 2026-11-10, cityhunteur.
- Decision: the replay operation is "set the mtime to the file's own current mtime" (`touch -c -m -r P P`, or
  `utimensat` with the stat value), never an mtime sent by the host and never `touch -d @<seconds>`.
  Rationale: it yields `IN_MODIFY`, keeps the host mtime exact, and needs no timestamp on the wire.
  Date/Author: 2026-11-10, agent, from Milestone 0.
- Decision: the watcher breaks the echo loop by remembering `(mtime_ns, size)` per forwarded path, not by
  FSEvents flags; for a path that no longer exists it replays the parent directory.
  Rationale: flags are cumulative; the stat key held in every Milestone 0 run.
  Date/Author: 2026-11-10, agent, from Milestone 0.
- Decision: rework after the council's reject. The watcher never waits for the guest: writes to the sink are
  non-blocking and the exit report reads published counters.
  Rationale: a guest that stopped reading port 2401 could hold up SIGTERM until the launcher was killed
  (decision log D-006).
  Date/Author: 2026-11-11, maintainer agent.
- Decision: the replay becomes three steps (atime-only set, mtime-only set, mtime set to now when the size
  changed in between), and the mtime guarantee is reworded. This replaces the 2026-11-10 decision on the replay
  operation and its rationale "keeps the host mtime exact". The guarantee is now: the replay leaves the host
  mtime equal to the last host write, except that a host write between the two sets gets the time of the replay
  and a same-size overwrite inside that window can keep the previous write's mtime.
  Rationale: the new suite step `inotify_host_rapid_edits_mtime_preserved` showed the single-step replay moving
  host mtimes backwards by about a second, because the guest read the mtime from its attribute cache
  (decision log D-005).
  Date/Author: 2026-11-11, maintainer agent.
- Decision (recorded late): the burst step was delivered as `inotify_burst_delivered` with 500 files, not the
  planned `inotify_burst_coalesced` with 2,000 files.
  Rationale: none was recorded when Milestone 3 landed. The 2,000-file burst was measured by hand in Milestone 2
  (`run/GH-150/m2-burst-reconnect.txt`); since the rework the watcher self-test sends a 3,000-file burst to a
  stand-in sink.
  Date/Author: 2026-11-11, maintainer agent.
- Decision: reconnect is covered by the watcher self-test (`sink_reconnects`, `sink_delivers_after_reconnect`,
  `sink_resends_unsent_edit`), not by a `binds-suite` step.
  Rationale: the self-test runs in CI on `macos-15` without a VM; `binds-suite` runs only on developer hardware.
  Against the real guest, reconnect is shown only by the manual run in `m2-burst-reconnect.txt`.
  Date/Author: 2026-11-11, maintainer agent.

## Outcomes & Retrospective

Reached: a host edit in a watched read-write share produces an inotify event in a container, measured at 12 to
24 ms, with host contents and mtimes untouched. chokidar, `watchexec`, `inotifywait` and Go `fsnotify` react
without polling. `make binds-suite` proves it (19 steps); two self-tests run in CI without a VM. Merged as PRs
#149 (plan), #151 (guest sink), #152 (launcher watcher) and #153 (`30cf8f0`, suite steps and measurements).

Not reached, and recorded in TD-038: exact inotify parity. Host creates, deletes and renames are visible only as
`IN_MODIFY` (on the file or its parent directory), there is no `IN_CLOSE_WRITE`, deletes in a share's root
directory and all edits on read-only shares are silent, and `IN_CREATE`-driven recursive watchers do not follow
directories created on the host. Whether GH #29 item 3 is met with these differences is left to a human.

What went well: the prototype milestone fixed the replay operation and the loop-breaking rule before any
product code, and both held unchanged. What cost time: the `/private` prefix difference between Foundation and
FSEvents, and two reconnect test attempts that killed the sink's supervisor by mistake. Left untested: a
physical external disk and a network volume; shares inside microVM tasks remain out of scope.

### Council rework (2026-11-11)

A four-judge council reviewed the merged work on 2026-11-10/11. The parity, regression and release judges
approved with gaps; the runtime judge rejected. This section replaces two statements above: that host mtimes
are untouched, and that GH #29 item 3 awaits a decision.

Two defects were on main at `30cf8f0`. First, the watcher wrote to the sink with a blocking `write` and the
exit report waited for the watcher's queue, so a guest that stopped reading port 2401 could hold up SIGTERM
until the launcher was killed. Second, the single-step replay read the mtime from the guest's attribute cache
and wrote it back: when one file was edited twice within about a second, the host mtime went back by 0.6 to
1.4 s.

The rework made the watcher non-blocking (batches of 1,000 lines, a retry every 50 ms, a queue of 100,000
lines, counters published under a lock) and the replay three steps. `binds-suite` gained
`inotify_host_rapid_edits_mtime_preserved`, which failed on the `30cf8f0` code and passes now (20 steps). The
watcher self-test grew from 11 to 20 checks through `--watch-sink-socket`, including an exit under a stalled
sink, and the sink self-test from 19 to 20. On a booted VM with the sink handler stopped and a 30,000-file
burst, the launcher exited 0.18 s after SIGTERM and 8.05 s after a guest shutdown
(`run/GH-150/m5-stalled-sink.txt`; suite runs in `run/GH-150/m5-suites.txt`).

The repository owner accepted the TD-038 residuals on 2026-11-10, and GH #29 item 3 was ticked on 2026-11-11
as a qualified tick: modify-style events only, on watched read-write shares only. TD-038 stays open and now
lists every residual. New debt: TD-043 (`src/systemvm.rs` over the structure budget) and TD-044 (no test of the
`watch` field between Rust and Swift, no CI run of the vsock path).

Lesson: CI and the suite passed both defects. Neither had a step with a sink that stops reading or a file
edited faster than the guest's attribute cache expires; runtime-sensitive units now get the council before
their issue closes (decision log D-004).

## Context and Orientation

The System VM is the Alpine Linux virtual machine that Vesper boots on macOS through Apple's
Virtualization.framework. `launcher/Sources/vesper-vmlauncher/main.swift` is the Swift program that boots it.
The launch config (`build/systemvm/launch-config.json`, generated by `systemvmctl launch-config`, types in
`src/systemvm.rs`: `SharedDirectory`, `LaunchConfig.shared_directories`) lists the host directories to share.
The launcher attaches each as a virtio-fs device, a paravirtual file system that lets the guest read and write
a host directory. In the guest, `systemvm/overlay/usr/local/bin/vesper-share mount TAG PATH ro|rw` mounts each
device at the host's own absolute path, so Docker bind mounts need no path rewriting.

The host talks to the guest over vsock, a socket family between a host and its virtual machines. The guest
agent (`systemvm/overlay/usr/local/bin/vesper-agent`, started per connection by `socat` from
`systemvm/overlay/etc/init.d/vesper-agent`) listens on vsock port 1024 and speaks a line protocol; `SHARE
<args>` runs `vesper-share`. The Rust client is `Session::share` in `src/control.rs`. The launcher also owns the
`VZVirtioSocketDevice` and can open guest vsock ports directly, without the host-visible bridge socket.

Why events are missing: inotify events are generated by the guest kernel when an operation passes through the
guest's own file-system layer. A host edit changes the file behind virtio-fs; the guest kernel is not told.
Virtualization.framework exposes no notification channel for this. `scripts/systemvm/binds-suite.sh` (steps
`inotify_guest_writes_seen` and `inotify_host_edits_not_propagated`) records both facts.

FSEvents is the macOS API that reports changes under a directory tree to a process, with per-file paths and a
configurable coalescing latency.

Guides to read first: `docs/specs/design-v0.md` (Phase 4, virtio-fs requirement), `docs/SECURITY.md` ("Shared
host directories"), `docs/RELIABILITY.md`, `docs/process/systemvm-bringup.md` ("Bind mounts over virtio-fs"),
ADR 0001 (`docs/design-docs/adr/0001-systemvm-launcher-swift-helper.md`), the completed plan
`docs/exec-plans/completed/2026-11-08-WOR-39-bind-mounts.md`, and the README of `launcher/`, `systemvm/overlay/`
and `scripts/systemvm/`.

## Plan of Work

Milestone 0 was a prototype and is finished; its tools (an FSEvents printer, a Python bridge that replayed
through `docker exec`) stay in `run/TD-038/` as evidence and are not promoted. Its results are in Surprises &
Discoveries and fix the replay operation, the loop-breaking rule and the residuals.

Milestone 1 adds the guest sink. `vesper-share notify` reads one absolute path per line on stdin (the wire carries no mtime)
and, for each, checks that the path lies under a mounted virtio-fs share from `/proc/mounts`, contains no `..`
component and exists, then resolves the path with `realpath` and sets the target's mtime to its own current
value (`touch -c -m -r P P`) without creating files; a target outside the shares is rejected. (Corrected on
2026-11-11: this text said the sink does not follow a final symlink. Since the council rework the replay is
three steps; see the Decision Log.) A path that is a share's root directory is counted and skipped. It answers nothing per line and prints a counter summary on EOF. A new OpenRC service beside
`vesper-agent` runs it under `socat VSOCK-LISTEN:<port>,fork`, on a dedicated port so that a long-lived stream
does not occupy the agent. `vesper-share probe` reports whether the sink is present. Unit-level proof: a guest
script that feeds accepted and rejected paths and asserts the counters.

Milestone 2 adds the host watcher. `SharedDirectory` gains `watch: bool` in `src/systemvm.rs` and in the
launcher's `LaunchConfig`; `shared_directories_from_specs` parses the opt-out suffix; `systemvmctl launch-config`
and `scripts/systemvm/generate-launch-config.sh` pass it through. In `main.swift`, after the guest is ready, the
launcher starts one FSEvents stream for the watched shares (file-level events, no-defer, 50 ms coalescing latency), connects to the sink port through `VZVirtioSocketDevice`, and writes one line per changed path. It
drops events whose `(mtime_ns, size)` it has already forwarded, which breaks the echo loop; for a path that no
longer exists it sends the parent directory; on `MustScanSubDirs` or dropped events it walks the reported
directory and sends each entry modified in the last 60 s (at most 50,000 entries visited); when the queue is
full it drops the oldest lines; and it reconnects every 2 s when the sink
restarts. (Corrected on 2026-11-11: this text said it sends the affected directories instead of each file.) It logs counts, never file contents. If `main.swift` would grow past the structure sensor's budget,
put the watcher in a second Swift source file.

Milestone 3 changes `scripts/systemvm/binds-suite.sh`. The step `inotify_host_edits_not_propagated` becomes
`inotify_host_edit_propagated` (append to an existing file) and `inotify_host_create_propagated` (new file), each
with a 1 s bound (Milestone 0 measured about 20 ms; the bound leaves room for a loaded host). New steps: `inotify_no_echo_storm` (one host edit yields a bounded
number of events over five seconds), `inotify_burst_coalesced` (writing 2,000 files on the host neither stalls
the VM nor loses the final state), `inotify_ro_share_behaviour` (records that a read-only share yields no events),
`inotify_nowatch_share_silent`, `inotify_host_mtime_preserved` (host mtime identical to the nanosecond before
and after the replay), and `inotify_delete_rename_behaviour` (records what a watcher sees in a subdirectory and
in the share root). Docker
conformance, ports and shim suites must still pass with watching enabled.

Milestone 4 updates the owning docs in the same change set as the behaviour: `docs/process/systemvm-bringup.md`,
`docs/SECURITY.md` (the guest now writes metadata into shares on the host's instruction; path validation
rules), `docs/RELIABILITY.md` (watcher failure is degraded, not fatal), the three READMEs, the parity matrix row
for #29, TD-038 (narrowed to the residuals found), GH #29 item 3, GH #150 and the milestone log.

## Concrete Steps

Run from the repository root on an Apple silicon Mac that can boot the System VM.

    make build
    make systemvm-up-shares && make docker-up
    make binds-suite                      # before: inotify_host_edits_not_propagated is "ok" (the limitation)
    # ... implement Milestones 1 and 2 ...
    cargo test systemvm
    make systemvm-down && make systemvm-up-shares && make docker-up
    make binds-suite                      # after: the new inotify_* steps pass
    make docker-conformance && make ports-suite && make systemvm-shim-suite
    python3 harness/check_docs.py && python3 scripts/harness/check_structure.py --root .
    scripts/harness/collect_verification_bundle.sh
    make docker-down && make systemvm-down && make systemvm-launch-config

Expected new lines in `run/systemvm/binds-report.json` after the change:

    ok   inotify_host_edit_propagated: event for hello.txt after <N> ms
    ok   inotify_host_create_propagated: event for from-host.txt after <N> ms
    ok   inotify_no_echo_storm: 1 host edit, <K> events in 5 s

## Validation and Acceptance

The work is accepted when all of these hold on developer hardware, with artifacts under `run/` and a
verification bundle under `.symphony/runs/GH-150/current/`:

- A host append and a host create inside a watched read-write share each produce an inotify event in a
  container watching the directory, within 1 s. The steps fail on `main`
  before the change.
- chokidar reports `change` for a host edit and `add` for a host create without polling.
- One host edit does not produce an unbounded event stream, and host file contents are never changed by the
  replay; host mtimes are unchanged to the nanosecond.
- A path outside the mounted shares, a path with `..`, and a symlink that leaves the share are rejected by the
  sink; a test shows each.
- Killing the sink or the watcher leaves the VM, Docker and existing shares working; the watcher reconnects.
- No new host-visible socket and no new entitlement: `make validate-entitlements` unchanged, and
  `docs/SECURITY.md` states the new guest write path.
- `cargo test`, the sibling suites, both docs sensors and CI for the pushed SHA are green, and the issue
  records local and remote results.

The residuals from Milestone 0 (no events on `:ro` shares, no `IN_CREATE`, `IN_DELETE` or `IN_CLOSE_WRITE`,
deletes in the share root, directories created on the host for watchers that depend on `IN_CREATE`) are
written into TD-038 as a narrowed entry rather than hidden behind a ticked box. Whether GH #29 item 3 can be
ticked with those residuals is a human decision at Milestone 4.

## Idempotence and Recovery

Every step can be repeated. `vesper-share notify` is stateless; a restart loses only queued notifications, and
the files themselves are always current on access. The watcher is disabled per share by the opt-out suffix and
globally by regenerating the launch config without it (`make systemvm-launch-config`). Rollback is a revert of
the merge commit plus a System VM image rebuild; no host state persists outside `run/`. Milestone 0 changes
nothing in the repository.

## Artifacts and Notes

Current behaviour, from `run/WOR-39/systemvm/binds-report.json`:

    ok   inotify_host_edits_not_propagated: host edits produce no inotify events in the guest (TD-038)

Milestone 0, with the prototype bridge (`run/TD-038/m0-watchers-bridge.txt`, `m0-latency.txt`):

    host append f.txt        inotifywait: MODIFY /data/f.txt   chokidar: change /data/f.txt
    host create sub/b.txt    inotifywait: MODIFY /data/sub/b.txt   chokidar: add /data/sub/b.txt
    host delete sub/c.txt    inotifywait: MODIFY,ISDIR /data/sub   chokidar: unlink /data/sub/c.txt
    host delete root-new.txt (share root)   no event in any watcher
    edits=30 seen=30 min=13ms p50=19ms p95=21ms max=22ms

Unknowns, each with its resolution:

- Whether Go `fsnotify`-based tools act on these events: **Unknown**; add one to the Milestone 3 suite.
- Whether the host ctime change disturbs host tools (backup, `git` index refresh): **Unknown**; check `git
  status` cost on a watched repository in Milestone 3 and record it.
- FSEvents behaviour for shares on external or network volumes: **Unknown**; test one external volume in
  Milestone 3 or record it as unsupported.
- Whether any guest operation can signal a change in a share's root directory: **Unknown**; none found. Record
  as a residual unless Milestone 1 finds one.

## Interfaces and Dependencies

Host: FSEvents through CoreServices, already available to the Swift launcher; no new package. Guest: busybox
`touch` or `chmod` and `socat`, both already in the image; one new OpenRC service and one new vsock port
(guest-side only). Launch config: `shared_directories[].watch` (boolean; true for read-write shares unless `PATH:nowatch`). Agent protocol
version stays 1; the sink is a separate port, not a new agent command. No new Rust crate, no new Go module, no
entitlement change.
