---
status: verified
owner: cityhunteur
last_reviewed: 2026-11-11
---

# Symphony milestones

### 2026-11-11 — GH-155 — design, parity matrix and specs index brought back in line with the code
- GitHub issue: #155
- PR: see the issue
- Commit: see the issue
- Phase: governance (all phases)
- Parity unit: none; documentation truth. `docs/specs/design-v0.md` rewritten from an EGM drift report: as-built text where the code is the intended design, and every unmet requirement kept and labelled with a tracker pointer
- Result: keep
- Verification bundle: none (no runtime change); review papers in `run/egm/` (local, not tracked): drift report, four stage reports, requirement audit, consolidated report
- Council summary: not run (no code path changed). EGM review instead: zoom-out, comprehension, critic (design, structure, style) and a second zoom-out, with remediation after each stage; 4 blockers, 41 majors and 17 nits, all applied. A separate audit checked about 220 requirements of the previous text: none lost after remediation, no number changed
- Harness update: `docs/design-docs/decision-log.md` rows D-009 to D-018; tech-debt rows TD-045 to TD-052; GH #156 opened for connecting `vesperd` to Backend A, with the questions its ADR must settle; both design diagrams regenerated from their Mermaid sources; stale statements fixed in `ARCHITECTURE.md`, `docs/TESTING.md` and `docs/process/shim-operations.md`
- Remaining risks: the architecture is coherent for Backend A only; Docker on Backend B is undesigned (D-017). The threat table has no reviewer (TD-051). The parity matrix and specs index were corrected from the drift report but not reviewed stage by stage. No reviewer re-read the design after the final pass
- Truthful project state after this milestone: Docker commands work from macOS on runc containers in the System VM; microVMs run only for a client that names the Vesper runtime (`ctr`); `vesperd` answers the Firecracker API from memory and drives no microVM; Backend B has no code. Next work, in order: a share-path prototype and the parity reference (both time-boxed), then `vesperd` to Backend A (GH #156), then Docker containers in microVMs (GH #53), then Phase 5 (GH #56)

### 2026-11-11 — GH-150 — council rework (shutdown hang and host mtime rollback fixed)
- GitHub issue: #150 (item 3 of #29); Linear mirror WOR-107
- PR: #154
- Commit: `9c5a865` (squash merge; CI green on the PR head `51fd52e` and on main)
- Phase: 4
- Parity unit: the same as the 2026-11-10 entry, made safe: the launcher's share watcher never waits for the guest, and the guest replay no longer moves host mtimes backwards
- Result: keep
- Verification bundle: `.symphony/runs/GH-150/current/` (the four verdicts are in `council/`); raw evidence in `run/GH-150/m5-stalled-sink.txt` and `run/GH-150/m5-suites.txt` (local, not tracked)
- Council summary: four judges reviewed `19045c4..30cf8f0` on 2026-11-10/11. Parity, regression and release: approve-with-gaps. Runtime: reject, because shutdown could hang on a sink that stops reading and no runtime evidence covered back-pressure or shutdown. This entry is the rework
- Defects found: (1) the watcher wrote to the sink with a blocking `write` and the exit report waited for the watcher's queue, so a guest that stopped reading port 2401 held up SIGTERM until the launcher was killed; (2) the single-step replay (`touch -c -m -r P P`) read the mtime from the guest's attribute cache and wrote it back, which moved the host mtime backwards by 0.6 to 1.4 s when one file was edited twice within about a second. Both were on main at `30cf8f0`
- Fixes: non-blocking writes in batches of 1,000 lines with a retry every 50 ms, an exit report read from counters published under a lock, a queue of 100,000 lines whose dropped lines are forgotten so that a later edit is sent again, one reconnect attempt per 2 s, a `share-watch-failed` status line, and a log line and counter for each rescan; a three-step replay (atime-only set, mtime-only set, mtime set to now when the size changed in between); `max-children=4` on the sink's listener and literal matching of mount points that contain `*`, `?` or `[`. The claim "host mtimes are never changed" is withdrawn (decision log D-005, D-006; the guarantee is in `docs/SECURITY.md`)
- Harness update: `binds-suite` 19 → 20 steps (`inotify_host_rapid_edits_mtime_preserved`; `inotify_share_root_delete_not_propagated` now counts every event); `make share-watch-selftest` 11 → 20 checks through the new `vesper-vmlauncher --watch-sink-socket` (burst of 3,000 files, reconnect, exit under a stalled sink, queue limit); `make share-notify-selftest` 19 → 20 checks (`glob_share_name_literal`); the System VM Image workflow now also runs when `scripts/macos/share-watch-selftest.sh` changes and, on push, when `systemvm/**` or the packages lock change
- Runtime evidence: with the guest sink handler stopped and a 30,000-file host burst, the launcher exited 0.18 s after SIGTERM (`queued=26149 stalls=621 rescans=5`) and 8.05 s after a guest shutdown with status `stopped` (`m5-stalled-sink.txt`). The new suite step failed on the `30cf8f0` code (the host mtime went back 0.6 to 1.4 s in each round) and passes now. With the blocking write restored, the self-test check `stalled_sink_sigterm_exits` reports `hung`. On the rework tree `make binds-suite` passed 20/20 five times in a row, and `make docker-conformance`, `make ports-suite`, `make systemvm-shim-suite` and `cargo test` passed (`m5-suites.txt`)
- Remaining risks: the residuals in TD-038, now listed in full, including the two mtime exceptions, the atime and ctime changes and the loss of the lines in flight when the sink handler dies; `make binds-suite` runs only on developer hardware (TD-005); no test covers the launch-config `watch` field between Rust and Swift and no CI job runs the vsock path end to end (TD-044); `src/systemvm.rs` is over the structure budget (TD-043)
- Truthful project state after this milestone: GH #29 item 3 was ticked on 2026-11-11 as a qualified tick (modify-style events only, on watched read-write shares only; the owner accepted the TD-038 residuals on 2026-11-10, decision log D-003). TD-038 stays open. GH #150 closes when this rework is merged with green CI. Phase 5 has not started

### 2026-11-10 — GH-150 — inotify events for host edits on virtio-fs shares (TD-038 narrowed)
- GitHub issue: #150 (item 3 of #29)
- PR: #149 (plan and prototype), #151 (guest sink), #152 (launcher watcher), #153 (suite steps and measurements)
- Commit: `e8aadc3` (#149), `1356881` (#151), `3aa5f85` (#152), `30cf8f0` (#153); squash merges, CI green incl. System VM Image with both self-tests
- Phase: 4
- Parity unit: a host edit in a watched read-write share reaches containers as inotify `IN_MODIFY`; the launcher watches shares with FSEvents and the guest replays each path as an mtime-preserving touch (`vesper-share notify`, guest vsock port 2401)
- Result: keep
- Verification bundle: `.symphony/runs/GH-150/current/`; raw evidence in `run/GH-150/` and `run/TD-038/`
- Council summary: not run before the merge (CI green). A four-judge council reviewed the unit on 2026-11-10/11 and the runtime judge rejected it; see the 2026-11-11 entry above
- Harness update: `make share-notify-selftest`, `make share-watch-selftest` (both in the System VM Image workflow), `vesper-vmlauncher --watch-dry-run`, `binds-suite` 10 → 19 steps, `make systemvm-up-shares` boots a third `:nowatch` share
- Runtime evidence: `binds-suite` 19/19 six runs in a row after a stream-order flake in an older step was fixed; 12 to 24 ms over 30 edits; chokidar, watchexec, inotifywait and Go fsnotify react; 2,000-file burst delivered to chokidar in about 1 s; reconnect after the sink handler was killed; docker-conformance, ports and shim suites pass with watching on
- Remaining risks: not exact inotify parity (no `IN_CREATE`/`IN_DELETE`/`IN_CLOSE_WRITE`, share-root deletes and read-only shares silent, `inotifywait -r` does not follow host-created directories), all in TD-038; physical external and network volumes untested; developer-hardware evidence only (TD-005)
- Truthful project state after this milestone: Phase 4's last gap is narrowed, not closed. GH #29 item 3 is still unticked and needs a human decision on whether the residuals are acceptable; Phase 5 has not started.

### 2026-11-09 — WOR-32 — runc guest execution implemented, reviewed and merged (PR #145)
- GitHub issue: #23 (mirror #72)
- PR: [#145](https://github.com/cityhunteur/vesper/pull/145) merged
- Commit: `47ad201` (squash merge; all CI green, incl. validate-compat-exceptions and the parity-baseline cross-check). Implementation `74e6544`; review fixes `cf18596`
- Phase: 3
- Parity unit: init and exec lifecycle, independent stdio, interactive terminals and resize, signal exits 0/7/137/143 through runc inside the microVM
- Review: a post-implementation review raised four findings, all resolved in `cf18596` — the workload's requested in-guest security posture is deliberately overridden (full capabilities, no-new-privileges off, LSM labels dropped) because the microVM is the isolation boundary, now tracked as compat exception CE-0004 (approved by cityhunteur on 2026-11-09 as a pre-GA deviation); a latent `watchDone` hang guarded; the `OpenSession` ordering comment corrected; two known limits recorded (exec `SIGNAL` ignores `all`; kill/exit race)
- Result: keep, merged
- Verification bundle: `.symphony/runs/WOR-32/current/`; raw evidence in `run/WOR-32/`; committed [ctr capture](../specs/ctr-conformance-baseline.json) records the clean source SHA and baseline revisions
- Council summary: two independent reviewers plus primary review; resource, abort, orphan, event-order and shutdown findings fixed and rechecked
- Harness update: `make systemvm-exec-suite` (19/19); ctr conformance now 74/74; source-stamped captures; reliability summary quoting fixed
- Runtime evidence: complete pinned kernel config and runc hello-world; legacy microVM/rootfs/workload/control and fresh services suites passed; 50/50 boots with maximum 2846 ms against 5000 ms, services and Docker smoke green, zero leaked resources
- Remaining risks: CE-0004 (security-posture override) is approved (cityhunteur, 2026-11-09) and time-boxed to 2027-03-31; port reclamation after 16,125 execs per task (TD-041); pause/snapshot semantics (TD-042); dedicated performance measurement and CI hardware (TD-033, TD-005)
- Truthful project state after this milestone: Phase 3 guest execution is implemented, reviewed and on main at `47ad201`. The WOR-32 trackers (Linear Done, GH #23/#72 closed) were already closed before this implementation and were updated with evidence comments only, not reopened. CE-0004 is approved as a pre-GA deviation (cityhunteur, 2026-11-09).

### 2026-11-08 — WOR-26 — compat exceptions CE-0001..CE-0003 approved as pre-GA deviations, GH #21 complete, Phase 2 epic closed
- GitHub issue: #21 (mirror #63); epic #4 (WOR-14)
- PR: #143
- Commit: `ddf7fb2` (squash merge of PR #143; remote CI green incl. Compat Exceptions Policy and Parity Baseline)
- Phase: 2
- Parity unit: human approval of the three Phase 2 compat exceptions recorded in the registry (`approved_by`, `approved_on`), validated by `compatexctl` (pair set together, date not in the future) and reported as `approved=`
- Result: keep
- Verification bundle: `.symphony/runs/WOR-26/current/` (unchanged capture; registry validation only)
- Council summary: skipped; governance-only change
- Harness update: `src/compat.rs` approval fields and checks, `compatexctl` summary, lifecycle doc, parity matrix row #21
- Remaining risks: the exceptions stay open until the upstream pytest suite runs (CE-0001), the pinned commit is built or the lock moves to the tag (CE-0002, TD-029) and a Linux same-host baseline exists (CE-0003); all expire 2027-03-31
- Truthful project state after this milestone: Phase 2 is closed with three approved, time-boxed deviations; Phase 3 waits on the WOR-32 guest agent; Phase 4 waits on a TD-038 decision; Phase 5 has not started

### 2026-11-08 — WOR-35 — ctr conformance, metrics, CI validation and runbook, GH #26 (3 of 4 items; ctr exec open)
- GitHub issue: #26 (mirror #79)
- PR: #142
- Commit: `eeac45f` (squash merge of PR #142; remote CI green, eight workflows incl. the new ctr Conformance)
- Phase: 3
- Parity unit: shim `Stats` from the microVM's cgroup v2; `make ctr-conformance` (shim 21 + bundle 15 + restart 18 steps) assembled with the baseline revisions; committed capture validated and published by CI; `docs/process/shim-operations.md`
- Result: keep
- Verification bundle: `.symphony/runs/WOR-35/current/` (evidence in `run/WOR-35/systemvm/`, incl. `ctr-conformance-report.json`)
- Council summary: skipped; CI green
- Harness update: `vesper-ctr metrics`, `task_metrics` step, `ctr-conformance.sh`, `ctr_conformance_report.py`, `make ctr-conformance[-check]`, `ctr-conformance.yml`, runbook
- Remaining risks: `ctr exec` on the Vesper runtime needs the WOR-32 guest agent; developer-hardware capture only (TD-005)
- Truthful project state after this milestone: Phase 3 is complete except the exec/TTY/stdin paths (WOR-32); Phase 4 is complete except inotify for host edits (TD-038); Phase 5 has not started

### 2026-11-08 — WOR-55 — linux/amd64 containers through binfmt_misc and qemu-user, GH #39 complete (and GH #31 item 6)
- GitHub issue: #39 (mirror #95); also GH #31 item 6
- PR: #141
- Commit: `20c9c28` (squash merge of PR #141; remote CI green)
- Phase: 4
- Parity unit: pinned `qemu-x86_64` registered by `vesper-binfmt` (OCF), `vesper-arch` execution-mode metadata, `make crossarch-suite` 9/9, workflow suite amd64 step required
- Result: keep
- Verification bundle: `.symphony/runs/WOR-55/current/` (evidence in `run/WOR-55/systemvm/`, incl. `crossarch-report.json`)
- Council summary: skipped; CI green
- Harness update: packages lock 22 entries; `vesper-binfmt`; `vesper-arch`; agent `ARCH`; `control arch`; `make crossarch-suite`; DNS determinism step compares resolver and subnet; TD-040
- Remaining risks: x86-64 only, shell-loop benchmark only (TD-040); `docker run --platform` re-tags the pulled reference (documented)
- Truthful project state after this milestone: every Phase 4 sub-issue is landed (#27, #28, #30, #31, #39 complete; #29 complete except inotify for host edits, TD-038); the Phase 4 epic goes to review; Phase 3 keeps two gated items (WOR-32 agent, WOR-35 CI conformance) and Phase 5 is untouched

### 2026-11-08 — WOR-41 — Developer workflow suite, GH #31 (5 of 6 items; amd64 open for #39)
- GitHub issue: #31 (mirror #81)
- PR: #140
- Commit: `c88e4f8` (squash merge of PR #140; remote CI green)
- Phase: 4
- Parity unit: `make workflow-suite` (compose, build from a host context, logs -f streaming, interactive exec -it, HUP/TERM/KILL, amd64 probe), report stamped with the baseline revisions
- Result: keep
- Verification bundle: `.symphony/runs/WOR-41/current/` (evidence in `run/WOR-41/systemvm/`, incl. `workflow-report.json`)
- Council summary: skipped; no code change; CI green
- Harness update: `scripts/systemvm/workflow-suite.sh`, `fixtures/wf-build/`, `make workflow-suite`, native-image architecture guard
- Remaining risks: `--platform linux/amd64` not available (GH #39); developer-hardware only (TD-005)
- Truthful project state after this milestone: the everyday Docker workflow works from macOS; Phase 4 has one issue left (#39 cross-architecture) plus the epic

### 2026-11-08 — WOR-40 — DNS and network defaults verified and documented, GH #30 complete
- GitHub issue: #30 (mirror #80)
- PR: #139
- Commit: `7d35acc` (squash merge of PR #139; remote CI green)
- Phase: 4
- Parity unit: `make dns-suite` with a compose fixture, 12 checks from macOS (resolver, external DNS and egress, embedded DNS, service discovery, aliases, NXDOMAIN, determinism, cleanup); defaults and caveats documented
- Result: keep
- Verification bundle: `.symphony/runs/WOR-40/current/` (evidence in `run/WOR-40/systemvm/`, incl. `dns-report.json`)
- Council summary: skipped; no code change; CI green
- Harness update: `scripts/systemvm/dns-suite.sh`, `fixtures/dns-stack.yml`, `make dns-suite`, TD-039
- Remaining risks: no `host.docker.internal`, IPv6 answers without a route (TD-039)
- Truthful project state after this milestone: Phase 4 has #27, #28, #29 (3 of 4 items) and #30 done; #31 (workflow suite) and #39 (cross-arch) remain

### 2026-11-08 — WOR-39 — Bind mounts over virtio-fs at host paths, GH #29 (3 of 4 items; inotify for host edits open)
- GitHub issue: #29 (mirror #77)
- PR: #138
- Commit: `f83dc1d` (squash merge of PR #138; remote CI green)
- Phase: 4
- Parity unit: `VESPER_SHARES` → virtio-fs devices in the launcher → `vesper-share` mounts at the host paths; `docker run -v` from macOS binds host directories; semantics recorded by `make binds-suite`
- Result: keep
- Verification bundle: `.symphony/runs/WOR-39/current/` (evidence in `run/WOR-39/systemvm/`, incl. `binds-report.json`)
- Council summary: skipped; CI green
- Harness update: `shared_directories` in the launch config, `--share`, `VESPER_SHARES`, `vesper-share`, agent `SHARE`, `control share`, `up.sh` mounting, `make systemvm-up-shares`, `make binds-suite`, TD-038
- Remaining risks: no inotify for host edits (TD-038); ownership fixed to the sharing user; no default share (a human decides what to share)
- Truthful project state after this milestone: `docker run -v /host/dir:/x` works from macOS for configured shares with documented semantics; Phase 4 continues with GH #30 (DNS/network defaults), #31 (workflow suite), #39 (cross-arch)

### 2026-11-08 — WOR-34 — Published ports on localhost through vesper-proxy and a guest vsock gateway, GH #28 complete
- GitHub issue: #28 (mirror #104)
- PR: #137
- Commit: `3400ba4` (squash merge of PR #137; remote CI green)
- Phase: 4
- Parity unit: `vesper-proxy --publish-ports` (loopback listener per TCP `-p` binding, forwarded through the bridge to `vesper-port-relay` in the guest), `ports.json` status, conflict handling, teardown, republish after restart (ADR 0005)
- Result: keep
- Verification bundle: `.symphony/runs/WOR-34/current/` (evidence in `run/WOR-34/systemvm/`, incl. `ports-report.json`)
- Council summary: skipped; CI green; the launcher SIGPIPE fix is a reliability repair found by the suite
- Harness update: `src/ports.rs`; `vesper-port-relay`; `make ports-suite`; HEALTH `ip`; launcher ignores SIGPIPE; TD-037
- Remaining risks: TCP and loopback only, no UDP, no metrics (TD-037); the host now opens loopback TCP listeners while containers with `-p` run (SECURITY.md)
- Truthful project state after this milestone: `docker run -p` from macOS reaches the container on localhost; Phase 4 continues with GH #29 (bind mounts), #30 (DNS), #31 (workflow suite), #39 (cross-arch)

### 2026-11-08 — WOR-38 — Docker Engine API conformance through vesper-proxy, GH #27 complete
- GitHub issue: #27 (mirror #78)
- PR: #136
- Commit: `5914811` (squash merge of PR #136; remote CI green)
- Phase: 4
- Parity unit: `make docker-conformance`, 22 checks from macOS through the canonical ingress (protocol, context, images, run/exec/stop/kill/rm, pull, dockerd error passthrough, boundary)
- Result: keep
- Verification bundle: `.symphony/runs/WOR-38/current/` (evidence in `run/WOR-38/systemvm/`, incl. `docker-conformance-report.json`)
- Council summary: skipped; no crate code changed; CI green
- Harness update: `scripts/systemvm/docker-conformance.sh`; `make docker-conformance`; TD-036
- Remaining risks: Vesper-specific unsupported behaviours (ports from macOS, bind mounts) pass through with guest-local semantics until #28/#29 (TD-036); the pull step needs outbound network
- Truthful project state after this milestone: the Docker CLI works from macOS against the System VM's dockerd for the core workflow; Phase 4 continues with GH #28 (published ports on localhost)

### 2026-11-08 — WOR-36 — Shim restart resilience and reconciliation, GH #25 complete
- GitHub issue: #25 (mirror #75)
- PR: #135
- Commit: `ccf239d` (squash merge of PR #135; remote CI green, seven workflows)
- Phase: 3
- Parity unit: durable `vesper-task.json` per transition; containerd's `delete` after a dead shim reconciles exit status and removes the microVM; tasks survive a containerd restart
- Result: keep
- Verification bundle: `.symphony/runs/WOR-36/current/` (evidence in `run/WOR-36/systemvm/`, incl. `restart-report.json`)
- Council summary: skipped; no control-plane path changed; CI green
- Harness update: `vesper-ctr shim-pid|kill-shim|wait-gone|state-file|inflight-start|inflight-result`; `run_restart_suite` (`src/control/suites/restart.rs`); `make systemvm-restart-suite`
- Remaining risks: containerd's own recovery of an in-flight Create is containerd behaviour (the suite proves only a clean failure); exec/TTY/stdin still open (WOR-32)
- Truthful project state after this milestone: Phase 3 has 4 of 5 sub-issues done (#22, #24, #25 complete; #23 transport only; #26 gated on exec and on CI boot); Phase 4 can start with GH #27

### 2026-11-08 — WOR-37 — OCI bundle validation and block-first rootfs cache, GH #24 complete
- GitHub issue: #24 (mirror #76)
- PR: #134
- Commit: `d52cff2` (squash merge of PR #134; remote CI green, seven workflows)
- Phase: 3
- Parity unit: `shim/internal/bundle` validation before any mount; root drives built once per image layer chain with digest-verified reuse and a private copy per task; `vesper-rootfs.json` record; documented direct-build fallback
- Result: keep
- Verification bundle: `.symphony/runs/WOR-37/current/` (evidence in `run/WOR-37/systemvm/`, incl. `bundle-report.json`)
- Council summary: skipped; no control-plane path changed; CI green
- Harness update: `vesper-ctr import-layered|run-image|start-image|meta|cache`; `run_bundle_suite` (`src/control/suites/bundle.rs`); `make systemvm-bundle-suite`; shim runs without the framework reaper; TD-034 narrowed, TD-035 added
- Remaining risks: cache keyed per containerd instance, never pruned, whole-drive copy per task (TD-035); exec/TTY/stdin still open (WOR-32)
- Truthful project state after this milestone: containerd tasks boot microVMs from cached block-first root drives with validated bundles; Phase 3 continues with WOR-36 (#25 restart resilience), WOR-32 (agent) and WOR-35 (#26 conformance)

### 2026-11-08 — WOR-32 slice 1 — vsock transport for microVMs (GH #23 in progress)
- GitHub issue: #23 (mirror #72, open)
- PR: #133
- Commit: `ecba8ac` (squash merge of PR #133; remote CI green)
- Phase: 3
- Parity unit: `vesper-microvm launch --vsock` attaches a Firecracker vsock device, the initramfs carries and loads the guest transport modules, `status` reports the host-side socket
- Result: keep
- Verification bundle: `run/WOR-32/systemvm/` (launch, status, `GET /vm/config` evidence in the ExecPlan)
- Council summary: skipped; no control-plane path changed
- Harness update: `--vsock`, `vesper_vsock=1`, `VESPER-VSOCK-OK`, `status … vsock=present|absent`; ExecPlan with the guest agent protocol contract
- Remaining risks: the guest agent, shim exec sessions and the exec suite are open (TD-034); the agent is the next unit
- Truthful project state after this milestone: the microVM has a bidirectional channel to the System VM; exec, TTY, stdin and signals still do not work through the shim

### 2026-11-06 — WOR-29 — containerd shim v2 Task lifecycle RPCs, GH #22 complete
- GitHub issue: #22 (mirror #70)
- PR: #132
- Commit: `d1bd355` (squash merge of PR #132; remote CI green, seven workflows incl. the new Shim Tests)
- Phase: 3
- Parity unit: `containerd-shim-vesper-v2` (Go, ADR 0004) serving Create/Start/Kill/Delete/State/Wait, each task a jailed Firecracker microVM booted from an ext4 built out of the bundle rootfs; `ctr run` end to end with exit code and output
- Result: keep
- Verification bundle: `.symphony/runs/WOR-29/current/` (evidence in `run/WOR-29/systemvm/`, incl. `shim-report.json`)
- Council summary: skipped; no control-plane path changed; CI green
- Harness update: `shim/` module and tests; `build-shim.sh`; overlay ships the shim; `vesper-ctr`; agent `CTR`; `run_shim_suite`; `make shim-build shim-test systemvm-shim-suite`; `shim-tests.yml`; Go in `systemvm-image.yml`; TD-034
- Remaining risks: shell required in the image, no stdin/terminal/exec/resize (P3.2, TD-034), no rootfs cache (P3.3), Go is a new toolchain for contributors and CI
- Truthful project state after this milestone: containerd inside the System VM can run tasks as microVMs through the Vesper shim; Phase 3 continues with WOR-32 (#23 exec, resize, stdio streaming, exit status) and WOR-37 (#24 bundle unpack and block-first rootfs)

### 2026-11-06 — WOR-26 — Backend A parity and performance baseline, GH #21 (conformance subset; upstream suite under CE-0001)
- GitHub issue: #21 (mirror #63, left open pending approval of the compat exceptions)
- PR: #131
- Commit: `c50effa` (squash merge of PR #131; remote CI green, eight workflows incl. the new Parity Baseline)
- Phase: 2
- Parity unit: `vesper-fcparity` conformance (19 checks against the stock v1.16.0 Firecracker in a jailed microVM) and perf (two runs of 20 cycles, boot p50 1510/1520 ms, drift 0.7 %), assembled with the lock revisions, parity metadata and compat summary into a capture CI validates and publishes
- Result: keep
- Verification bundle: `.symphony/runs/WOR-26/current/` (evidence in `run/WOR-26/systemvm/`, incl. `parity-baseline-report.json`)
- Council summary: skipped; no control-plane path changed; CI green
- Harness update: `vesper-fcparity`; agent `FCPARITY`; `run_parity_suite`; `parity-baseline.sh` + `parity_baseline_report.py`; `make systemvm-parity-baseline` / `parity-baseline-check`; `parity-baseline.yml`; CE-0001..CE-0003; microVM memory cap 640 MiB; TD-033
- Remaining risks: the upstream pytest suite is not run (CE-0001); release binary vs pinned commit (CE-0002, TD-029); no Linux same-host baseline (CE-0003); sequential-only perf, no overhead or I/O numbers (TD-033); the exceptions await a human's approval
- Truthful project state after this milestone: Backend A has a reproducible, CI-validated parity and performance capture against stock Firecracker; the Phase 2 exit bar ("upstream suite with no exclusions") is not met and is tracked as CE-0001; Phase 2 epic WOR-14 (#4) stays open on that item

### 2026-11-06 — WOR-30 — MicroVM-per-container workload lifecycle, GH #20 complete
- GitHub issue: #20 (mirror #69)
- PR: #130
- Commit: `2475a4f` (squash merge of PR #130; remote CI green)
- Phase: 2
- Parity unit: `vesper-workload run|ps|logs|wait|stop|rm|isolation` mapping one workload to one jailed Firecracker microVM with a private rootfs copy; command on the kernel command line; exit code and output mapped back
- Result: keep
- Verification bundle: `.symphony/runs/WOR-30/current/` (evidence in `run/WOR-30/systemvm/`, incl. `workload-report.json`)
- Council summary: skipped; no control-plane path changed; six remote workflows green
- Harness update: `vesper-workload`; `vesper-microvm launch --cmd`; agent `WORKLOAD`; `run_workload_suite`; `make systemvm-workload-suite`; trivial image with all busybox applets; TD-032 added
- Remaining risks: copied rootfs without CoW, command-line-bound command, no microVM network, no resource accounting beyond cgroup limits (TD-032); evidence is developer-hardware only (TD-005)
- Truthful project state after this milestone: Backend A runs concurrent container-style workloads one per jailed microVM with isolation and clean teardown; Phase 2 continues with WOR-26 (upstream parity and performance baseline) and the epic WOR-14

### 2026-11-06 — WOR-28 — Reproducible OCI image to ext4 rootfs with provenance, booted in a microVM, GH #19 complete
- GitHub issue: #19 (mirror #71)
- PR: #129
- Commit: `fa30660` (squash merge of PR #129; remote CI green)
- Phase: 2
- Parity unit: guest-side `docker export` to deterministic `mke2fs -d` with embedded provenance (source image id, layers, pinned baseline revisions, tool versions) and a digest-verified cache; microVM initramfs with root-drive modules; a jailed microVM boots the rootfs and executes from it
- Result: keep
- Verification bundle: `.symphony/runs/WOR-28/current/` (evidence in `run/WOR-28/systemvm/`, incl. `rootfs-report.json`)
- Council summary: skipped; no control-plane path changed; six remote workflows green
- Harness update: `vesper-rootfs`; `vesper-microvm launch --rootfs`; agent `ROOTFS`; `control/suites.rs` split; `make systemvm-rootfs-suite`; `e2fsprogs-extra` pinned; baselines lock shipped into the overlay; TD-030 resolved, TD-031 added
- Remaining risks: reproducibility keyed by the image id (TD-031); only the guest-built trivial image converted; the microVM has no network (WOR-30); evidence is developer-hardware only (TD-005)
- Truthful project state after this milestone: Backend A can turn a Docker image into a bootable, reproducible, provenance-stamped ext4 rootfs and boot it in a jailed Firecracker microVM; Phase 2 continues with WOR-30 (microVM per container) and WOR-26 (upstream parity baseline)

### 2026-11-06 — WOR-31 — Stock Firecracker runs under the jailer inside the System VM, GH #18 complete
- GitHub issue: #18 (mirror #68)
- PR: #128
- Commit: `0e66d26` (squash merge of PR #128; remote CI green)
- Phase: 2
- Parity unit: digest-pinned Firecracker and jailer v1.16.0 provisioned through the overlay; a guest microVM manager that launches jailed microVMs (uid 10000, pivot-root jail, cgroup v2 limits, seccomp) and proves stop, SIGKILL cleanup and guest power-off from the host
- Result: keep
- Verification bundle: `.symphony/runs/WOR-31/current/` (evidence in `run/WOR-31/systemvm/`, incl. `microvm-report.json`)
- Council summary: skipped; no control-plane path changed; six remote workflows green
- Harness update: packages lock schema 2 with `archives`; `systemvm/archives.yaml`; `systemvmctl archive-list`; `vesper-microvm`; agent `MICROVM`; `control::run_microvm_suite`; `make systemvm-microvm-suite`; TD-029, TD-030
- Remaining risks: release binary rather than the exact pinned commit (TD-029); no rootfs or network in the microVM yet (TD-030); evidence is developer-hardware only (TD-005)
- Truthful project state after this milestone: Backend A exists on an M3 Pro: a stock Firecracker microVM boots in about 1.5 s under the jailer on nested KVM inside the System VM; Phase 2 continues with WOR-28 (OCI to ext4 rootfs), WOR-30 (microVM per container) and WOR-26 (upstream parity baseline)

### 2026-11-06 — WOR-24 — Nested virtualization detection and backend gating, GH #17 complete
- GitHub issue: #17 (mirror #66)
- PR: #127
- Commit: `8469d90` (squash merge of PR #127; remote CI green, incl. the M2 runner's fallback path)
- Phase: 2
- Parity unit: host capability probe through Apple's API, `auto|require|off` nested policy in the lock and launch config, Backend A/B selection with diagnostics, KVM presence reported by the guest
- Result: keep
- Verification bundle: `.symphony/runs/WOR-24/current/` (evidence in `run/WOR-24/systemvm/`)
- Council summary: skipped; no control-plane path changed; six remote workflows green
- Harness update: `vesper-vmlauncher --host-capabilities`; `platform::HostCapabilities`, `NestedPolicy`, `select_backend`; `systemvmctl backend-select`; lock schema 6; `HEALTH.kvm`; CI gating step on the M-series runner; a structure-checker ERROR was caught in CI after a `tail -2` hid it locally
- Remaining risks: Backend B fallback is not implemented (design-v0 Phase 5); no M1/M2 host booted a guest (CI's M2 runner cannot boot at all)
- Truthful project state after this milestone: an M3 Pro boots the System VM with `/dev/kvm` available for Backend A; unsupported hosts get a diagnostic and the fallback selection; Phase 2 continues with WOR-31 (Firecracker and jailer lifecycle in the System VM)

### 2026-11-06 — WOR-12 — Phase 1 System VM bring-up complete on developer hardware
- GitHub issue: #3 (mirror #54)
- PR: none (epic close-out; sub-issues landed as PRs #119 to #126)
- Commit: `e152da1` (last sub-issue merge)
- Phase: 1
- Parity unit: close the Phase 1 epic now that #12, #13, #14, #15 and #16 pass their checklists
- Result: keep
- Verification bundle: `.symphony/runs/WOR-27`, `WOR-25`, `WOR-33`, `WOR-23`, `WOR-21` (evidence under `run/WOR-*/systemvm/`)
- Council summary: skipped; epic close-out with no code change
- Harness update: parity audit row for #3 and the design-docs catalogue updated
- Remaining risks: boot and socket evidence is developer-hardware only (TD-005); bridge socket exposure (TD-027); packages reinstalled per boot (TD-025); shell agent (TD-026); packaging and launchd absent
- Truthful project state after this milestone: design-v0's Phase 1 exit criteria hold on an M3 (reliable boots, `docker version` and a trivial container from macOS through `vesper-proxy`, raw sockets private, automated reliability suite); Phase 2 (Backend A, stock Firecracker in the System VM under nested virtualisation) is next

### 2026-11-06 — WOR-21 — Reliability suite with a JSON health report, GH #16 complete
- GitHub issue: #16 (mirror #62)
- PR: #126
- Commit: `e152da1` (squash merge of PR #126; remote CI green)
- Phase: 1
- Parity unit: N-cycle boot stress plus control, services and Docker socket suites with host leak checks and a machine-readable report
- Result: keep
- Verification bundle: `.symphony/runs/WOR-21/current/` (evidence in `run/WOR-21/systemvm/`, incl. `reliability-report-50.json` and the first failing run's log)
- Council summary: skipped; the suite itself is the sensor; six remote workflows green
- Harness update: `reliability-suite.sh`, `reliability_report.py` (self-test in CI), `make systemvm-reliability`; `vesper-services` detaches networking and the daemons from the runlevel (TD-028 resolved)
- Remaining risks: the suite needs Apple silicon (TD-005)
- Truthful project state after this milestone: the first run caught three boots over the 5 s threshold, the fix brought the login prompt back to about 2.2 s, and the third run passed 50/50 with zero leaks; all GH #16 checklist items are proven

### 2026-11-06 — WOR-23 — Docker socket ingress through vesper-proxy, trivial container runs from macOS, GH #15 complete
- GitHub issue: #15 (mirror #65); advances the #3 epic
- PR: #125
- Commit: `9da6e5f` (squash merge of PR #125; remote CI green)
- Phase: 1
- Parity unit: owner-only host socket served by `vesper-proxy` with startup upstream validation, reaching the guest's Docker through the launcher bridge and a supervised vsock relay; offline trivial image from the pinned ISO; raw-socket denial proven
- Result: keep
- Verification bundle: `.symphony/runs/WOR-23/current/` (evidence in `run/WOR-23/systemvm/`, incl. `docker-smoke-report.json`)
- Council summary: skipped; no control-plane path changed; six remote workflows green
- Harness update: `vesper-proxy --listen-mode --validate-upstream` + SIGTERM; `vesper-docker-relay`; `docker-up.sh`, `docker-down.sh`, `build-trivial-image.sh`, `docker-smoke.sh`; `make docker-up/docker-down/docker-smoke/systemvm-trivial-image`; SECURITY.md host-socket inventory; invariant 1b Verified
- Remaining risks: packaged `/var/run/vesper/docker.sock` and launchd supervision do not exist; bridge exposure (TD-027); relay limits (TD-021); evidence is developer-hardware only (TD-005)
- Truthful project state after this milestone: design-v0's Phase 1 exit criterion holds on developer hardware (`docker version` and a trivial container from macOS through `vesper-proxy`); Phase 1 closes with WOR-21 (reliability suite) and the WOR-12 epic

### 2026-11-06 — WOR-33 — containerd and dockerd supervised in the System VM, GH #14 complete
- GitHub issue: #14 (mirror #73)
- PR: #124
- Commit: `f64ac7b` (squash merge of PR #124; remote CI green)
- Phase: 1
- Parity unit: start `containerd` and `dockerd` at boot under OpenRC `supervise-daemon` with cgroup v2 and DHCP networking, expose their health through the agent, and prove crash recovery and log capture from the host
- Result: keep
- Verification bundle: `.symphony/runs/WOR-33/current/` (evidence in `run/WOR-33/systemvm/`, incl. `services-report.json`)
- Council summary: skipped; no control-plane path changed; six remote workflows green
- Harness update: overlay `networking`, `cgroups`, `rc.conf`, `vesper-services`; agent `HEALTH` fields, `KILL`, `LOGS`; `control::run_services_suite`; `systemvmctl control kill|logs|services-suite`; `make systemvm-services-suite`; `containerd-ctr` pinned; TD-028
- Remaining risks: boot to login 3.4 to 4.5 s against the 5 s threshold (TD-028); dockerd reachable only inside the guest (WOR-23); no container run yet
- Truthful project state after this milestone: all four GH #14 checklist items are proven (both daemons healthy at boot, recovered within 600 ms after SIGKILL, startup and respawn lines captured); Phase 1 continues with WOR-23 (Docker socket ingress) and WOR-21 (reliability suite)

### 2026-11-06 — WOR-25 — vsock control channel proven end to end, GH #13 complete
- GitHub issue: #13 (mirror #67)
- PR: #123
- Commit: `cf091e9` (squash merge of PR #123; remote CI green)
- Phase: 1
- Parity unit: launcher-served vsock bridge (`CONNECT <port>` preface, owner-only socket), supervised socat-backed guest agent (protocol 1), Rust control client with keepalive and reconnect, `make systemvm-up/down/control-suite`
- Result: keep
- Verification bundle: `.symphony/runs/WOR-25/current/` (evidence in `run/WOR-25/systemvm/`, incl. `control-report.json`)
- Council summary: skipped; no control-plane path changed; six remote workflows green
- Harness update: ADR 0003; `src/control.rs` (+ RULES row); `systemvmctl control …`; image lock schema 5 (`control`); `vesper-packages` OpenRC oneshot and `vesper-agent` service in the overlay; `scripts/systemvm/up.sh` / `down.sh`; TD-026, TD-027
- Remaining risks: shell line-protocol agent (TD-026); host-visible owner-only bridge socket reaching any guest port (TD-027, human decision before Phase 1 exit); `containerd`/`dockerd` installed but not started (WOR-33)
- Truthful project state after this milestone: all four GH #13 checklist items are proven against the real guest (handshake 1 ms, 11 keepalive pings over 20 s, reconnect after a server-side drop in 1 attempt, 2 MiB echoed intact); Phase 1 continues with WOR-33 (services) and WOR-23 (Docker socket ingress)

### 2026-11-06 — WOR-25 — Pinned guest packages installed at boot through an initramfs overlay
- GitHub issue: #13 (mirror #67); also advances #14
- PR: #122
- Commit: `e4fed7c` (squash merge of PR #122; remote CI green incl. the overlay determinism check)
- Phase: 1
- Parity unit: pin the Alpine closure for `socat`, `containerd` and Docker, pack it with `systemvm/overlay/` into a deterministic apkovl appended to the pinned initramfs (ADR 0002), install it at every boot, and prove it from the guest probe
- Result: keep
- Verification bundle: `.symphony/runs/WOR-25/current/` (evidence in `run/WOR-25/systemvm/`)
- Council summary: skipped; no control-plane path changed; six remote workflows green
- Harness update: `docs/specs/systemvm-packages.lock.yaml` + `scripts/systemvm/resolve-apks.py`; `src/systemvm/packages.rs`; image lock schema 4 with `overlay`; `build-initramfs.py`, `fetch-packages.sh`, `build-overlay.sh`; `make systemvm-overlay` / `systemvm-prepare`; `systemvmctl package-list` / `verify-packages` / `verify-image --require-overlay`; CI caches packages and asserts determinism; TD-025
- Remaining risks: packages are reinstalled on every boot and nothing persists (TD-025); no service runs yet; the vsock bridge and agent are the next slices
- Truthful project state after this milestone: the guest boots in about 2 s with `socat`, `containerd` and `dockerd` installed from pinned, digest-verified packages; GH #13 and #14 remain partial

### 2026-11-06 — WOR-27 — NAT proven from the guest, 5 s boot threshold enforced, GH #12 complete
- GitHub issue: #12 (mirror #74)
- PR: #121
- Commit: `8ae5a7d` (squash merge of PR #121; remote CI green)
- Phase: 1
- Parity unit: console-driven guest probe (root login, DHCP from the NAT, gateway ping, `poweroff`) pinned in the image lock and run by the launcher on every boot; boot smoke enforces the owner's 5 s threshold against the launcher's ready time
- Result: keep
- Verification bundle: `.symphony/runs/WOR-27/current/` (evidence in `run/WOR-27/systemvm/`, incl. `boot-report-probe-200.json`)
- Council summary: skipped; no control-plane path changed; six remote workflows green
- Harness update: image lock schema 3 (`boot.max_boot_ms`, `boot.probe`); `ProbeSettings`; `run_boot_check` reads the launcher's `ready` JSON line and reports `boot_ms`; `systemvmctl boot-smoke --max-boot-ms`; launcher probe state machine and `--skip-probe`; ExecPlan moved to `docs/exec-plans/completed/`
- Remaining risks: no workload runs inside the guest and the console script is not a control channel (vsock under WOR-25, containerd under WOR-33); boot evidence is developer-hardware only (TD-005)
- Truthful project state after this milestone: every GH #12 acceptance criterion and checklist item is proven (200/200 probed boots, max boot 1197 ms under the 5 s threshold, DHCP lease and gateway ping from the guest, clean power-off); Phase 1 continues with WOR-25 (vsock control channel) and WOR-33 (containerd in the System VM)

### 2026-11-06 — WOR-27 — System VM boots on Virtualization.framework, 200/200 to the login prompt
- GitHub issue: #12 (mirror #74)
- PR: #120
- Commit: `a638c34` (squash merge of PR #120; remote CI green incl. the macOS `launcher-build` job)
- Phase: 1
- Parity unit: boot the pinned image through a thin Swift Virtualization.framework helper (ADR 0001) with NAT and a virtio console, and record repeated-boot diagnostics
- Result: keep
- Verification bundle: `.symphony/runs/WOR-27/current/` (evidence in `run/WOR-27/systemvm/`, incl. `boot-report-200.json`)
- Council summary: skipped; no control-plane path changed; seven remote CI jobs green
- Harness update: `launcher/` + `make vmlauncher-build` + `packaging/macos/vesper-vmlauncher.entitlements`; image lock schema 2 on the Alpine 3.22.8 virt ISO with `scripts/systemvm/unpack-zboot.py` (Alpine's arm64 kernel is an EFI zboot PE that VZ cannot start) and the ISO as read-only boot media; `systemvmctl launch-config --from-lock`; `boot-smoke` drives the launcher and records `durations_ms`; `src/systemvm/image_lock.rs` split out; first ADR
- Remaining risks: nothing runs inside the guest and NAT is unproven from the guest side; the launcher stops the VM without a clean shutdown; boot evidence is developer-hardware only because GitHub's macOS runners lack nested virtualisation (TD-005)
- Truthful project state after this milestone: GH #12 checklist items "200-cycle boot reliability" and "kernel boots to healthy state with console output" are proven (mean 1091 ms, max 1251 ms on an M3); "NAT functional from guest" and "boot time within threshold" remain open

### 2026-11-06 — WOR-27 — Phase 1 System VM image pinned, verified and reproducible in CI
- GitHub issue: #12 (mirror #74)
- PR: #119
- Commit: `8bd505b` (squash merge of PR #119; remote CI green)
- Phase: 1
- Parity unit: replace the placeholder kernel/rootfs with a pinned Alpine 3.22.8 aarch64 netboot kernel + initramfs, digest-verified by `make systemvm-fetch` and reproduced in CI
- Result: keep
- Verification bundle: `.symphony/runs/WOR-27/current/` (evidence in `run/WOR-27/systemvm/`)
- Council summary: skipped; no handler or runtime path changed, six remote workflows green including the new `System VM Image` job
- Harness update: added `docs/specs/systemvm-image.lock.yaml`, `systemvm::ImageLock`/`verify_image`, `systemvmctl lock-env`/`verify-image`, `scripts/systemvm/fetch-image.sh`, `make systemvm-fetch`, `.github/workflows/systemvm-image.yml`; manifest schema 2 with optional rootfs and initrd; `kernel_cmdline` in the launch config; `assets/systemvm/` removed; ExecPlan `docs/exec-plans/active/2026-11-06-WOR-27-systemvm-image-pipeline.md`
- Remaining risks: nothing boots the image yet (TD-005); the launcher technology (Swift Virtualization.framework helper vs Rust bindings) is an open human decision; the image is aarch64-only so Linux CI verifies digests but cannot boot it
- Truthful project state after this milestone: Phase 1 has started; GH #12's reproducibility deliverable is met, its boot, console, NAT and boot-time checklist items remain open

### 2026-04-12 — WOR-71 — `vesper-proxy` gains a Phase 0 vsock CONNECT bridge
- GitHub issue: #9
- PR: #118
- Commit: `badc4a6` (squash merge of PR #118; remote CI green)
- Phase: 0
- Parity unit: replace the explicit `vsock` dial stub in `vesper-proxy` with a testable host UDS bridge that speaks Firecracker's `CONNECT <port>` preface and prove the proxy survives a failed upstream dial
- Result: keep
- Verification bundle: `.symphony/runs/WOR-71/current/`
- Council summary: local review found the bridge intentionally narrow and legible; this slice satisfies the Phase 0 proxy-forwarding acceptance bar without claiming the broader Phase 1 keepalive, reconnect, or large-payload control-channel work
- Harness update: `src/proxy.rs` now resolves `vsock://<cid>:<port>` through a configurable `vsock_socket_dir`, writes the `CONNECT <port>` preface before relay, and adds focused tests for valid/invalid vsock target parsing, end-to-end vsock forwarding, and recovery after a failed upstream dial; `src/bin/vesper-proxy.rs` now exposes `--vsock-socket-dir`; `docs/specs/firecracker-feature-parity.md` and `docs/process/systemvm-bringup.md` now document the new Phase 0 bridge truth
- Remaining risks: the broader GH `#13` / WOR-73 control-channel issue still needs handshake/versioning, keepalive, reconnect, and larger transfer proof; this slice proves the proxy bridge skeleton, not the full System VM vsock transport contract
- Truthful project state after this milestone: `vesper-proxy` no longer treats `vsock://<cid>:<port>` as unimplemented, and the repo now carries executable proof that unix-socket ingress can relay through the Phase 0 host UDS bridge while surviving a failed upstream dial

### 2026-04-12 — WOR-15 — State-machine umbrella reconciled to current repo truth
- GitHub issue: #8
- PR: #117
- Commit: `c64ce51` (squash merge of PR #117; remote CI green)
- Phase: 0
- Parity unit: reconcile the stale-open state-machine umbrella to the current explicit lifecycle model, guarded-route contract coverage, and generated behavior-matrix evidence
- Result: keep
- Verification bundle: `.symphony/runs/WOR-15/current/`
- Council summary: not run for this docs/tracker truth-repair slice; local review found no code-path changes and confirmed the existing state-machine evidence already satisfies the scoped acceptance bar
- Harness update: refreshed the `#8` parity audit entry in `docs/specs/firecracker-feature-parity.md` to point at the current `preboot`/`running`/`paused` model in `src/api.rs`, the `documented_preboot_only_routes_reject_after_start` and `documented_postboot_only_routes_reject_before_start` contract assertions covering `24` guarded routes in `tests/api_contract.rs`, and the pinned `GET /`, `PUT /actions`, and `PATCH /vm` lifecycle cases in `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: the broader GH `#2` parity umbrella remains partial, and GitHub mirror issue `#58` plus Linear duplicate `WOR-65` still need cleanup after the source issue state is reconciled
- Truthful project state after this milestone: GH `#8` no longer needs to remain open for missing state-machine implementation work, because the current repo already carries explicit lifecycle state, deterministic invalid-state envelopes, and guarded-route verification across the routed phase-0 surface

### 2026-04-12 — WOR-105 — `src/api.rs` structure budget restored with route-family extractions
- GitHub issue: #110
- PR: #116
- Commit: `41444e8` (squash merge of PR #116; remote CI green)
- Phase: 0
- Parity unit: extract coherent `actions`, `boot_source`, `logger_metrics`, and `machine_config` route families plus the inline API tests out of `src/api.rs` without changing pinned contract behavior
- Result: keep
- Verification bundle: `.symphony/runs/WOR-105/current/`
- Council summary: local review found no parity or dispatch regression in the module split; the main residual risk is that `src/api.rs` still sits above the warning budget and will need additional extraction work in later slices
- Harness update: added `src/api/{actions,boot_source,logger_metrics,machine_config}.rs`, moved the inline API tests to `src/api/tests.rs`, kept route dispatch legible in `src/api.rs`, restored the structure sensor from an error to a warning at `3153` lines, and kept `make contract-test`, `make surface-matrix-check`, `make behavior-matrix-check`, and `python3 scripts/harness/check_docs.py --root .` green
- Remaining risks: `src/api.rs` remains above the warning budget and still contains other route families plus `/vm/config` export logic; issue update, push, CI, and human review are still pending
- Truthful project state after this milestone: the repo no longer fails the structure budget on `src/api.rs`, and the extracted route families retain the existing pinned Firecracker behavior, but the broader architecture hotspot is only partially decomposed

### 2026-04-12 — WOR-104 — Network-interface endpoint baseline packet captured
- GitHub issue: #109
- PR: #115
- Commit: `be35fee` (squash merge of PR #115; remote CI green)
- Phase: 0
- Parity unit: package the existing `/network-interfaces/{iface_id}` family baseline into an issue-local endpoint packet without repopulating the placeholder capture registry
- Result: keep
- Verification bundle: `.symphony/runs/WOR-104/current/`
- Council summary: not run for this docs/harness truth-repair slice; local review confirmed no code-path change and the acceptance gate stayed green
- Harness update: added `run/WOR-104/api/network-interface-endpoint-baseline-packet.md` tying together the existing network create, conflict, runtime patch, and `/vm/config` export evidence, reused the placeholder-registry boundary clarified in WOR-106, and kept the generated surface and behavior matrices unchanged and valid
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; `src/api.rs` remains the long-standing architectural hotspot; `WOR-105` is the next ready backlog slice after this issue lands
- Truthful project state after this milestone: `/network-interfaces/{iface_id}` now has a durable issue-local endpoint baseline packet tied to GH `#109`/WOR-104, and this slice does not claim new network-interface handler behavior or broader GH `#2` completion

### 2026-04-11 — WOR-106 — Hotplug-memory endpoint baseline packet reconciled to the current harness model
- GitHub issue: #108
- PR: #114
- Commit: `1b42818` (squash merge of PR #114; remote CI green)
- Phase: 0
- Parity unit: package the existing `/hotplug/memory` family baseline into an issue-local endpoint packet and clarify that concrete routes stay out of the placeholder capture registry
- Result: keep
- Verification bundle: `.symphony/runs/WOR-106/current/`
- Council summary: not run for this docs/harness truth-repair slice; local review confirmed no code-path change and the acceptance gate stayed green
- Harness update: added `run/WOR-106/api/hotplug-memory-endpoint-baseline-packet.md`, clarified `docs/process/baseline-governance.md` and `docs/specs/firecracker-api-baseline-captures.yaml` so concrete route families use issue-local packets plus `docs/specs/firecracker-api-behavior-captures.yaml` instead of the placeholder capture registry, and kept the generated surface and behavior matrices unchanged and valid
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; `src/api.rs` remains the long-standing architectural hotspot; `WOR-104` is the next ready backlog slice after this issue lands
- Truthful project state after this milestone: `/hotplug/memory` now has a durable issue-local endpoint baseline packet tied to GH `#108`/WOR-106, and the repo no longer instructs agents to repopulate the placeholder-only capture registry for concrete routes; no new hotplug handler behavior is claimed by this slice

### 2026-04-07 — WOR-20 — Runtime `PATCH /hotplug/memory` invalid-requested-size rejection captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: make the already-implemented runtime `PATCH /hotplug/memory` invalid-requested-size `400 InvalidRequest` rejection explicit in the route-level proof bundle
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice surfaces the already-enforced hotplug resize validator branch while expanding the behavior matrix to `37` routes and `157` pinned cases
- Harness update: added a dedicated full-router hotplug invalid-requested-size test, added `run/WOR-20/api/step147-upstream-hotplug-invalid-requested-size-baseline.md`, added `run/WOR-20/api/step147-hotplug-memory-invalid-requested-size-note.md`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with `invalid_requested_size_runtime` for `PATCH /hotplug/memory`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `PATCH /hotplug/memory` now records the preboot invalid-state rejection, the booted resize success path, and the runtime invalid-requested-size rejection, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Preboot `PUT /hotplug/memory` invalid-block-size rejection captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: make the already-implemented preboot `PUT /hotplug/memory` invalid-block-size `400 InvalidRequest` rejection explicit in the route-level proof bundle
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice surfaces the already-enforced hotplug block-size validator branch while expanding the behavior matrix to `37` routes and `156` pinned cases
- Harness update: added a dedicated full-router hotplug invalid-block-size test, added `run/WOR-20/api/step146-upstream-hotplug-invalid-block-size-baseline.md`, added `run/WOR-20/api/step146-hotplug-memory-invalid-block-size-note.md`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with `invalid_block_size_preboot` for `PUT /hotplug/memory`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `PUT /hotplug/memory` now records the preboot invalid-block-size rejection alongside its existing success, invalid-total-size, and postboot-invalid-state cases, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Preboot `PUT /hotplug/memory` invalid-total-size rejection captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: make the already-implemented preboot `PUT /hotplug/memory` invalid-total-size `400 InvalidRequest` rejection explicit in the route-level proof bundle
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice surfaces the already-enforced hotplug total-size validator branch while expanding the behavior matrix to `37` routes and `155` pinned cases
- Harness update: strengthened the existing full-router hotplug invalid-request test to assert the exact total-size message, added `run/WOR-20/api/step145-upstream-hotplug-invalid-total-size-baseline.md`, added `run/WOR-20/api/step145-hotplug-memory-invalid-total-size-note.md`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with `invalid_total_size_preboot` for `PUT /hotplug/memory`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `PUT /hotplug/memory` now records the preboot invalid-total-size rejection alongside its existing success and postboot-invalid-state cases, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — `PUT /snapshot/load` `resume_vm=true` success captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: make the already-implemented `PUT /snapshot/load` `resume_vm=true` `204` success path explicit in the route-level proof bundle
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice records the already-implemented `resume_vm=true` snapshot-load success path directly on the route and expands the behavior matrix to `37` routes and `154` pinned cases
- Harness update: attached `run/WOR-20/api/step144-upstream-snapshot-load-resume-success-baseline.md` to a new `allowed_with_resume_vm_returns_no_content` case in `docs/specs/firecracker-api-behavior-captures.yaml`, added `run/WOR-20/api/step144-snapshot-load-resume-success-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `PUT /snapshot/load` now records both the default and the explicit `resume_vm=true` `204` success paths at the route level, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Preboot `GET /` instance-info surface surfaced explicitly
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: make the already-pinned preboot `GET /` `Not started` instance-info surface explicit in the route-level proof bundle
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice surfaces the already-pinned preboot instance-info proof while keeping the matrix at `37` routes and `153` pinned cases
- Harness update: split the old shared preboot identity/version proof in `tests/api_contract.rs` into dedicated `GET /` and `GET /version` tests, attached `run/WOR-20/api/step143-upstream-instance-info-preboot-baseline.md` to the existing `preboot_instance_info` case in `docs/specs/firecracker-api-behavior-captures.yaml`, added `run/WOR-20/api/step143-instance-info-preboot-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `GET /` now has explicit route-level preboot coverage for the `Not started` instance-info path before boot, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Runtime `GET /hotplug/memory` updated requested-size surface surfaced explicitly
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: make the already-implemented runtime `GET /hotplug/memory` requested-size-after-patch surface explicit in the route-level proof bundle
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice surfaces the already-implemented hotplug read-after-resize proof while growing the matrix to `37` routes and `153` pinned cases
- Harness update: split the existing booted `GET /hotplug/memory` proof into dedicated initial-status and post-resize requested-size coverage in `tests/api_contract.rs`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new `runtime_updated_requested_size` case, added `run/WOR-20/api/step142-upstream-hotplug-runtime-updated-status-baseline.md`, added `run/WOR-20/api/step142-hotplug-memory-runtime-updated-status-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `GET /hotplug/memory` now has explicit route-level coverage for the booted initial-status and post-resize requested-size read paths, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Runtime `GET /balloon/hinting/status` read surfaces surfaced explicitly
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: make the already-pinned runtime `GET /balloon/hinting/status` missing-device, hinting-disabled, and idle-status surfaces explicit in the route-level proof bundle
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice surfaces the already-pinned runtime balloon-hinting-status read proofs while keeping the matrix at `37` routes and `152` pinned cases
- Harness update: added `run/WOR-20/api/step141-upstream-balloon-hinting-status-runtime-surfaces-baseline.md`, split the existing booted `GET /balloon/hinting/status` proof into dedicated runtime missing-device, hinting-disabled, and idle-status coverage in `tests/api_contract.rs`, attached the step-local evidence note to the existing runtime `GET /balloon/hinting/status` cases in `docs/specs/firecracker-api-behavior-captures.yaml`, added `run/WOR-20/api/step141-balloon-hinting-status-runtime-surfaces-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `GET /balloon/hinting/status` now has explicit route-level runtime coverage for missing-device, hinting-disabled, and idle-status read paths after boot, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Runtime `GET /balloon/statistics` read surfaces surfaced explicitly
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: make the already-pinned runtime `GET /balloon/statistics` missing-device, stats-disabled, and enabled-stats surfaces explicit in the route-level proof bundle
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice surfaces the already-pinned runtime balloon-statistics read proofs while keeping the matrix at `37` routes and `152` pinned cases
- Harness update: added `run/WOR-20/api/step140-upstream-balloon-stats-runtime-surfaces-baseline.md`, added dedicated runtime `GET /balloon/statistics` missing-device, stats-disabled, and enabled-stats coverage in `tests/api_contract.rs`, attached the step-local evidence note to the existing runtime `GET /balloon/statistics` cases in `docs/specs/firecracker-api-behavior-captures.yaml`, added `run/WOR-20/api/step140-balloon-stats-runtime-surfaces-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `GET /balloon/statistics` now has explicit route-level runtime coverage for missing-device, stats-disabled, and enabled-stats read paths after boot, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Runtime `GET /balloon` read surfaces surfaced explicitly
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: make the already-pinned runtime `GET /balloon` missing-device and updated-target surfaces explicit in the route-level proof bundle
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice surfaces the already-pinned runtime balloon read proofs while keeping the matrix at `37` routes and `152` pinned cases
- Harness update: added `run/WOR-20/api/step139-upstream-balloon-get-runtime-surfaces-baseline.md`, added dedicated runtime `GET /balloon` missing-device and updated-target coverage in `tests/api_contract.rs`, attached the step-local evidence note to the existing runtime `GET /balloon` cases in `docs/specs/firecracker-api-behavior-captures.yaml`, added `run/WOR-20/api/step139-balloon-get-runtime-surfaces-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `GET /balloon` now has explicit route-level runtime coverage for the missing-device and updated-target read paths after boot, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Preboot `GET /balloon` read surfaces surfaced explicitly
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: make the already-pinned preboot `GET /balloon` missing-device and configured-defaults surfaces explicit in the route-level proof bundle
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice surfaces the already-pinned preboot balloon read proofs while keeping the matrix at `37` routes and `152` pinned cases
- Harness update: added `run/WOR-20/api/step138-upstream-balloon-get-preboot-surfaces-baseline.md`, added dedicated preboot `GET /balloon` missing-device and configured-defaults coverage in `tests/api_contract.rs`, attached the step-local evidence note to the existing preboot `GET /balloon` cases in `docs/specs/firecracker-api-behavior-captures.yaml`, added `run/WOR-20/api/step138-balloon-get-preboot-surfaces-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `GET /balloon` now has explicit route-level preboot coverage for the missing-device and configured-defaults read paths before boot, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Preboot `GET /mmds` empty-store success surfaced explicitly
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: make the already-pinned preboot `GET /mmds` empty-store `{}` success path explicit in the route-level proof bundle
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice surfaces the already-pinned preboot MMDS empty-store proof while keeping the matrix at `37` routes and `152` pinned cases
- Harness update: added `run/WOR-20/api/step137-upstream-mmds-preboot-empty-store-baseline.md`, added dedicated preboot `GET /mmds` empty-store coverage in `tests/api_contract.rs`, attached the step-local evidence note to the existing `preboot_empty_store` capture in `docs/specs/firecracker-api-behavior-captures.yaml`, added `run/WOR-20/api/step137-mmds-preboot-empty-store-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `GET /mmds` now has explicit route-level preboot coverage for the empty-store `{}` path before MMDS initialization, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Booted `GET /mmds` configured-store success surfaced explicitly
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: make the already-pinned booted `GET /mmds` configured-store success path explicit in the route-level proof bundle
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice surfaces the already-pinned booted MMDS configured-store success proof while keeping the matrix at `37` routes and `152` pinned cases
- Harness update: added `run/WOR-20/api/step136-upstream-mmds-runtime-configured-store-baseline.md`, added dedicated booted `GET /mmds` configured-store coverage in `tests/api_contract.rs`, attached the step-local evidence note to the existing `runtime_configured_store` capture in `docs/specs/firecracker-api-behavior-captures.yaml`, added `run/WOR-20/api/step136-mmds-runtime-configured-store-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `GET /mmds` now has explicit route-level runtime coverage for the configured-store success path after MMDS networking is configured and the VM is booted, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Runtime `GET /vm/config` metrics omission captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `GET /vm/config` records the runtime metrics omission surface after a successful booted metrics update
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice registers the already-aligned booted `/vm/config` metrics omission proof and expands the matrix to `37` routes and `152` pinned cases
- Harness update: added `run/WOR-20/api/step135-upstream-vm-config-metrics-runtime-omission-baseline.md`, added dedicated runtime `GET /vm/config` metrics-omission coverage in `tests/api_contract.rs`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new runtime vm-config-metrics-omission case, added `run/WOR-20/api/step135-vm-config-metrics-runtime-omission-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `GET /vm/config` now has explicit route-level runtime coverage for the metrics omission path after boot, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Runtime `GET /vm/config` serial omission captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `GET /vm/config` records the runtime serial omission surface after a successful booted serial update
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice registers the already-aligned booted `/vm/config` serial omission proof and expands the matrix to `37` routes and `151` pinned cases
- Harness update: added `run/WOR-20/api/step134-upstream-vm-config-serial-runtime-omission-baseline.md`, added dedicated runtime `GET /vm/config` serial-omission coverage in `tests/api_contract.rs`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new runtime vm-config-serial-omission case, added `run/WOR-20/api/step134-vm-config-serial-runtime-omission-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `GET /vm/config` now has explicit route-level runtime coverage for the serial omission path after boot, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Runtime `GET /vm/config` logger omission captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `GET /vm/config` records the runtime logger omission surface after a successful booted logger update
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice registers the already-aligned booted `/vm/config` logger omission proof and expands the matrix to `37` routes and `150` pinned cases
- Harness update: added `run/WOR-20/api/step133-upstream-vm-config-logger-runtime-omission-baseline.md`, added dedicated runtime `GET /vm/config` logger-omission coverage in `tests/api_contract.rs`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new runtime vm-config-logger-omission case, added `run/WOR-20/api/step133-vm-config-logger-runtime-omission-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `GET /vm/config` now has explicit route-level runtime coverage for the logger omission path after boot, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Runtime `GET /vm/config` pmem export captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `GET /vm/config` records the runtime pmem export surface after boot
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice registers the already-green booted `/vm/config` pmem export proof and expands the matrix to `37` routes and `149` pinned cases
- Harness update: added `run/WOR-20/api/step132-upstream-vm-config-pmem-runtime-baseline.md`, added dedicated runtime `GET /vm/config` pmem coverage in `tests/api_contract.rs`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new runtime vm-config-pmem-export case, added `run/WOR-20/api/step132-vm-config-pmem-runtime-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `GET /vm/config` now has explicit route-level runtime coverage for the pmem export path, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Runtime `GET /vm/config` entropy export captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `GET /vm/config` records the runtime entropy export surface after boot
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice registers the already-green booted `/vm/config` entropy export proof and expands the matrix to `37` routes and `148` pinned cases
- Harness update: added `run/WOR-20/api/step131-upstream-vm-config-entropy-runtime-baseline.md`, added dedicated runtime `GET /vm/config` entropy coverage in `tests/api_contract.rs`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new runtime vm-config-entropy-export case, added `run/WOR-20/api/step131-vm-config-entropy-runtime-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `GET /vm/config` now has explicit route-level runtime coverage for the entropy export path, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Runtime `GET /vm/config` balloon export captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `GET /vm/config` records the runtime balloon export surface after boot
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice registers the already-green booted `/vm/config` balloon export proof and expands the matrix to `37` routes and `147` pinned cases
- Harness update: added `run/WOR-20/api/step130-upstream-vm-config-balloon-runtime-baseline.md`, added dedicated runtime `GET /vm/config` balloon coverage in `tests/api_contract.rs`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new runtime vm-config-balloon-export case, added `run/WOR-20/api/step130-vm-config-balloon-runtime-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `GET /vm/config` now has explicit route-level runtime coverage for the balloon export path, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Runtime `GET /vm/config` boot-source export captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `GET /vm/config` records the runtime boot-source export surface after boot
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice registers the already-green booted `/vm/config` boot-source export proof, repairs the helper path used by booted export tests, and expands the matrix to `37` routes and `146` pinned cases
- Harness update: added `start_instance()` to `tests/api_contract.rs` so runtime export tests can boot without clobbering previously configured boot resources, added `run/WOR-20/api/step129-upstream-vm-config-boot-source-runtime-baseline.md`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new runtime vm-config-boot-source-export case, added `run/WOR-20/api/step129-vm-config-boot-source-runtime-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `GET /vm/config` now has explicit route-level runtime coverage for the boot-source export path, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Runtime `GET /vm/config` drive export captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `GET /vm/config` records the runtime drive export surface after a successful runtime patch
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice registers the already-green booted `/vm/config` drive export proof and expands the matrix to `37` routes and `145` pinned cases
- Harness update: added `run/WOR-20/api/step128-upstream-vm-config-drives-runtime-baseline.md`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new runtime vm-config-drive-export case, added `run/WOR-20/api/step128-vm-config-drives-runtime-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `GET /vm/config` now has explicit route-level runtime coverage for the drive export path, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Runtime `GET /vm/config` network export captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `GET /vm/config` records the runtime network-interface export surface after a successful runtime patch
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice registers the already-green booted `/vm/config` network export proof and expands the matrix to `37` routes and `144` pinned cases
- Harness update: added `run/WOR-20/api/step127-upstream-vm-config-network-runtime-baseline.md`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new runtime vm-config-network-export case, added `run/WOR-20/api/step127-vm-config-network-runtime-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `GET /vm/config` now has explicit route-level runtime coverage for the network-interface export path, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — `GET /vm/config` vsock exports now assert deprecated `vsock_id` omission
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: tighten route-level proof that `GET /vm/config` omits deprecated `vsock_id` from stored vsock exports
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice strengthens the existing preboot/runtime `/vm/config` vsock export captures without widening handler scope, and the matrix remains at `37` routes and `143` pinned cases
- Harness update: added `run/WOR-20/api/step126-upstream-vm-config-vsock-id-omission-baseline.md`, extended the existing `vsock_export_preboot` and `vsock_export_runtime` captures with `/vsock/vsock_id` absent assertions, added `run/WOR-20/api/step126-vm-config-vsock-id-omission-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `GET /vm/config` now explicitly records the deprecated `vsock_id` omission already proven by the preboot and runtime vsock export tests, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Preboot `GET /vm/config` logger omission captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `GET /vm/config` records the pinned preboot logger omission surface
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice registers the already-green preboot `/vm/config` logger omission proof and expands the matrix to `37` routes and `143` pinned cases
- Harness update: added `run/WOR-20/api/step125-upstream-vm-config-logger-absent-capture-baseline.md`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new preboot vm-config-logger-omission case, added `run/WOR-20/api/step125-vm-config-logger-absent-capture-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `GET /vm/config` now has explicit route-level preboot coverage for the logger omission path, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Preboot `GET /vm/config` metrics omission captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `GET /vm/config` records the pinned preboot metrics omission surface
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice registers the already-green preboot `/vm/config` metrics omission proof and expands the matrix to `37` routes and `142` pinned cases
- Harness update: added `run/WOR-20/api/step124-upstream-vm-config-metrics-absent-capture-baseline.md`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new preboot vm-config-metrics-omission case, added `run/WOR-20/api/step124-vm-config-metrics-absent-capture-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `GET /vm/config` now has explicit route-level preboot coverage for the metrics omission path, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Preboot `GET /vm/config` serial omission captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `GET /vm/config` records the pinned preboot serial omission surface
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice registers the already-green preboot `/vm/config` serial omission proof and expands the matrix to `37` routes and `141` pinned cases
- Harness update: added `run/WOR-20/api/step123-upstream-vm-config-serial-absent-capture-baseline.md`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new preboot vm-config-serial-omission case, added `run/WOR-20/api/step123-vm-config-serial-absent-capture-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `GET /vm/config` now has explicit route-level preboot coverage for the serial omission path, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Preboot `GET /vm/config` cpu-config omission captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `GET /vm/config` records the pinned preboot cpu-config omission surface
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice adds missing-field assertion support to the behavior-matrix harness, registers the already-green preboot `/vm/config` cpu-config omission proof, and expands the matrix to `37` routes and `140` pinned cases
- Harness update: added `run/WOR-20/api/step122-upstream-vm-config-cpu-config-absent-capture-baseline.md`, extended `src/api_behavior_matrix.rs` with the new `absent` body-assertion kind plus unit coverage, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new preboot vm-config-cpu-config-omission case, added `run/WOR-20/api/step122-vm-config-cpu-config-absent-capture-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `GET /vm/config` now has explicit route-level preboot coverage for the cpu-config omission path, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Preboot `GET /vm/config` drives default-cache export captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `GET /vm/config` records the already-proven preboot drives default-cache export surface
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice registers the already-green preboot `/vm/config` drives default-cache export proof and expands the matrix to `37` routes and `139` pinned cases
- Harness update: added `run/WOR-20/api/step121-upstream-vm-config-drives-preboot-cache-default-capture-baseline.md`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new preboot vm-config-drives-default-cache case, added `run/WOR-20/api/step121-vm-config-drives-preboot-cache-default-capture-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `GET /vm/config` now has explicit route-level preboot coverage for the default-cache drives export path, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Preboot `GET /vm/config` drives root-first export captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `GET /vm/config` records the already-proven preboot drives root-first export surface
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice registers the already-green preboot `/vm/config` drives root-first export proof in the behavior capture registry and expands the behavior matrix to `37` routes and `138` pinned cases
- Harness update: added `run/WOR-20/api/step120-upstream-vm-config-drives-preboot-root-order-capture-baseline.md`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new preboot vm-config-drives-root-order case, added `run/WOR-20/api/step120-vm-config-drives-preboot-root-order-capture-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `GET /vm/config` now has explicit route-level preboot coverage for the root-first drives export path, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Preboot `GET /vm/config` pmem export captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `GET /vm/config` records the already-proven preboot pmem export surface
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice registers the already-green preboot `/vm/config` pmem export proof in the behavior capture registry and expands the behavior matrix to `37` routes and `137` pinned cases
- Harness update: added `run/WOR-20/api/step119-upstream-vm-config-pmem-preboot-capture-baseline.md`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new preboot vm-config-pmem case, added `run/WOR-20/api/step119-vm-config-pmem-preboot-capture-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `GET /vm/config` now has explicit route-level preboot coverage for the pmem export path, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Preboot `GET /vm/config` boot-source export captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `GET /vm/config` records the already-proven preboot boot-source export surface
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice registers the already-green preboot `/vm/config` boot-source export proof in the behavior capture registry and expands the behavior matrix to `37` routes and `136` pinned cases
- Harness update: added `run/WOR-20/api/step118-upstream-vm-config-boot-source-preboot-capture-baseline.md`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new preboot vm-config-boot-source case, added `run/WOR-20/api/step118-vm-config-boot-source-preboot-capture-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `GET /vm/config` now has explicit route-level preboot coverage for the boot-source export path, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Preboot `GET /vm/config` entropy export captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `GET /vm/config` records the already-proven preboot entropy export surface
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice registers the already-green preboot `/vm/config` entropy export proof in the behavior capture registry and expands the behavior matrix to `37` routes and `135` pinned cases
- Harness update: added `run/WOR-20/api/step117-upstream-vm-config-entropy-preboot-capture-baseline.md`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new preboot vm-config-entropy case, added `run/WOR-20/api/step117-vm-config-entropy-preboot-capture-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `GET /vm/config` now has explicit route-level preboot coverage for the entropy export path, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Preboot `GET /vm/config` balloon export captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `GET /vm/config` records the already-proven preboot balloon export surface
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice registers the already-green preboot `/vm/config` balloon export proof in the behavior capture registry and expands the behavior matrix to `37` routes and `134` pinned cases
- Harness update: added `run/WOR-20/api/step116-upstream-vm-config-balloon-preboot-capture-baseline.md`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new preboot vm-config-balloon case, added `run/WOR-20/api/step116-vm-config-balloon-preboot-capture-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `GET /vm/config` now has explicit route-level preboot coverage for the balloon export path, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Preboot `GET /vm/config` network-interface export captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `GET /vm/config` records the already-proven preboot network-interface export surface
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice registers the already-green preboot `/vm/config` network-interface export proof in the behavior capture registry and expands the behavior matrix to `37` routes and `133` pinned cases
- Harness update: added `run/WOR-20/api/step115-upstream-vm-config-network-preboot-capture-baseline.md`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new preboot vm-config-network case, added `run/WOR-20/api/step115-vm-config-network-preboot-capture-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `GET /vm/config` now has explicit route-level preboot coverage for the network-interface export path, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Preboot `GET /vm/config` vsock export captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `GET /vm/config` records the already-proven preboot vsock export surface
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice registers the already-green preboot `/vm/config` vsock export proof in the behavior capture registry and expands the behavior matrix to `37` routes and `132` pinned cases
- Harness update: added `run/WOR-20/api/step114-upstream-vm-config-vsock-preboot-capture-baseline.md`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new preboot vm-config-vsock case, added `run/WOR-20/api/step114-vm-config-vsock-preboot-capture-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `GET /vm/config` now has explicit route-level preboot and runtime coverage for the vsock export path, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Runtime `GET /vm/config` MMDS-config export captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `GET /vm/config` remains callable after boot and continues to export stored MMDS networking configuration
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice adds a dedicated full-router runtime `GET /vm/config` proof for the stored MMDS-config export path, extends the behavior capture registry with the postboot export case, and expands the behavior matrix to `37` routes and `131` pinned cases
- Harness update: added `run/WOR-20/api/step113-upstream-vm-config-mmds-runtime-baseline.md`, added dedicated runtime `GET /vm/config` MMDS-config coverage in `tests/api_contract.rs`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new runtime vm-config-mmds case, added `run/WOR-20/api/step113-vm-config-mmds-runtime-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `GET /vm/config` now has explicit route-level runtime coverage for the MMDS-config export path alongside the machine-config, memory-hotplug, and vsock export paths, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Runtime `GET /vm/config` memory-hotplug export captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `GET /vm/config` remains callable after boot and continues to export stored memory-hotplug configuration
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice adds a dedicated full-router runtime `GET /vm/config` proof for the stored memory-hotplug export path, extends the behavior capture registry with the postboot export case, and expands the behavior matrix to `37` routes and `130` pinned cases
- Harness update: added `run/WOR-20/api/step112-upstream-vm-config-memory-hotplug-runtime-baseline.md`, added dedicated runtime `GET /vm/config` memory-hotplug coverage in `tests/api_contract.rs`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new runtime vm-config-memory-hotplug case, added `run/WOR-20/api/step112-vm-config-memory-hotplug-runtime-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `GET /vm/config` now has explicit route-level runtime coverage for the memory-hotplug export path alongside the machine-config and vsock export paths, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Runtime `GET /vm/config` vsock export captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `GET /vm/config` remains callable after boot and continues to export stored vsock configuration
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice adds a dedicated full-router runtime `GET /vm/config` proof for the stored vsock export path, extends the behavior capture registry with the postboot export case, and expands the behavior matrix to `37` routes and `129` pinned cases
- Harness update: added `run/WOR-20/api/step111-upstream-vm-config-vsock-runtime-baseline.md`, added dedicated runtime `GET /vm/config` vsock coverage in `tests/api_contract.rs`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new runtime vm-config-vsock case, added `run/WOR-20/api/step111-vm-config-vsock-runtime-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `GET /vm/config` now has explicit route-level runtime coverage for the vsock export path in addition to the machine-config export path, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Runtime `GET /vm/config` machine-config export captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `GET /vm/config` remains callable after boot and continues to export stored machine configuration
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice adds a dedicated full-router runtime `GET /vm/config` proof for the stored machine-config export path, extends the behavior capture registry with the postboot export case, and expands the behavior matrix to `37` routes and `128` pinned cases
- Harness update: added `run/WOR-20/api/step110-upstream-vm-config-runtime-baseline.md`, added dedicated runtime `GET /vm/config` coverage in `tests/api_contract.rs`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new runtime vm-config case, added `run/WOR-20/api/step110-vm-config-runtime-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `GET /vm/config` now has explicit route-level runtime coverage for the machine-config export path, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Runtime `PUT /metrics` success captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `PUT /metrics` remains callable after boot and returns `204 No Content` for a first-time request
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice adds a dedicated full-router runtime `PUT /metrics` proof, extends the behavior capture registry with the postboot success case, and expands the behavior matrix to `37` routes and `127` pinned cases
- Harness update: added `run/WOR-20/api/step109-upstream-metrics-runtime-baseline.md`, added dedicated runtime `PUT /metrics` coverage in `tests/api_contract.rs`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new runtime metrics case, added `run/WOR-20/api/step109-metrics-runtime-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `PUT /metrics` now has explicit route-level runtime coverage in addition to its duplicate-init fault capture, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Runtime `PUT /serial` success captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `PUT /serial` remains callable after boot and returns `204 No Content`
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice adds a dedicated full-router runtime `PUT /serial` proof, extends the behavior capture registry with the postboot success case, and expands the behavior matrix to `37` routes and `126` pinned cases
- Harness update: added `run/WOR-20/api/step108-upstream-serial-runtime-baseline.md`, added dedicated runtime `PUT /serial` coverage in `tests/api_contract.rs`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new runtime serial case, added `run/WOR-20/api/step108-serial-runtime-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `PUT /serial` now has explicit route-level runtime coverage, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Runtime `PUT /logger` success captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `PUT /logger` remains callable after boot and returns `204 No Content`
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice adds a dedicated full-router runtime `PUT /logger` proof, extends the behavior capture registry with the postboot success case, and expands the behavior matrix to `37` routes and `125` pinned cases
- Harness update: added `run/WOR-20/api/step107-upstream-logger-runtime-baseline.md`, added dedicated runtime `PUT /logger` coverage in `tests/api_contract.rs`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new runtime logger case, added `run/WOR-20/api/step107-logger-runtime-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `PUT /logger` now has explicit route-level runtime coverage, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Runtime `GET /machine-config` read captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `GET /machine-config` remains callable after boot and returns the stored configuration shape
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice adds a dedicated full-router runtime `GET /machine-config` proof, extends the behavior capture registry with the postboot read case, and expands the behavior matrix to `37` routes and `124` pinned cases
- Harness update: added `run/WOR-20/api/step106-upstream-machine-config-runtime-read-baseline.md`, added dedicated runtime `GET /machine-config` coverage in `tests/api_contract.rs`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new runtime machine-config read case, added `run/WOR-20/api/step106-machine-config-runtime-read-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `GET /machine-config` now has explicit route-level preboot and runtime read coverage, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Runtime `GET /version` response captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `GET /version` remains callable after boot and returns the pinned version response shape
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice adds a dedicated full-router runtime `GET /version` proof, extends the behavior capture registry with the postboot version case, and expands the behavior matrix to `37` routes and `123` pinned cases
- Harness update: added `run/WOR-20/api/step105-upstream-version-runtime-baseline.md`, added dedicated runtime `GET /version` coverage in `tests/api_contract.rs`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new runtime version case, added `run/WOR-20/api/step105-version-runtime-capture-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `GET /version` now has explicit route-level preboot and runtime coverage, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Instance-info lifecycle states captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `GET /` reports the pinned `Paused` and `Running` lifecycle states after successful snapshot loads
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice adds a dedicated full-router `GET /` lifecycle-state proof, extends the behavior capture registry with paused and running instance-info cases, and expands the behavior matrix to `37` routes and `122` pinned cases
- Harness update: added `run/WOR-20/api/step104-upstream-instance-info-runtime-state-baseline.md`, added dedicated `GET /` lifecycle-state coverage in `tests/api_contract.rs`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new runtime instance-info cases, added `run/WOR-20/api/step104-instance-info-runtime-state-capture-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: `GET /` now has explicit route-level lifecycle-state coverage for `Not started`, `Paused`, and `Running`, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Snapshot-create preboot invalid-state aligned and captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: align `PUT /snapshot/create` preboot requests with the pinned Firecracker invalid-state envelope and record the route-level proof
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice splits preboot snapshot-create requests from the existing running-state guard, preserves the paused success path, and expands the behavior matrix to `37` routes and `120` pinned cases
- Harness update: added an inferred upstream baseline note for the preboot `PUT /snapshot/create` invalid-state path, adjusted `src/api.rs` so preboot snapshot-create requests use the canonical Firecracker invalid-state envelope, added full-router snapshot-create preboot coverage in `tests/api_contract.rs`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new `PUT /snapshot/create` preboot invalid-state case, added `run/WOR-20/api/step103-snapshot-create-preboot-invalid-state-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: the route-level `PUT /snapshot/create` proof now includes the preboot invalid-state rejection and the local handler matches that pinned state-machine split, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Network patch preboot invalid-state rejection captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `PATCH /network-interfaces/{iface_id}` rejects preboot updates with the pinned Firecracker invalid-state envelope
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice reuses the already-aligned router-level invalid-state guard, adds explicit full-router proof for `PATCH /network-interfaces/{iface_id}`, and expands the behavior matrix to `37` routes and `119` pinned cases
- Harness update: added an inferred upstream baseline note for the preboot `PATCH /network-interfaces/{iface_id}` invalid-state path, added full-router network-patch preboot-update coverage in `tests/api_contract.rs`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new `PATCH /network-interfaces/{iface_id}` preboot invalid-state case, added `run/WOR-20/api/step102-network-patch-preboot-invalid-state-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: the route-level `PATCH /network-interfaces/{iface_id}` proof now includes the preboot invalid-state rejection, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Drive patch preboot invalid-state rejection captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `PATCH /drives/{drive_id}` rejects preboot updates with the pinned Firecracker invalid-state envelope
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice reuses the already-aligned router-level invalid-state guard, adds explicit full-router proof for `PATCH /drives/{drive_id}`, and expands the behavior matrix to `37` routes and `118` pinned cases
- Harness update: added an inferred upstream baseline note for the preboot `PATCH /drives/{drive_id}` invalid-state path, added full-router drive-patch preboot-update coverage in `tests/api_contract.rs`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new `PATCH /drives/{drive_id}` preboot invalid-state case, added `run/WOR-20/api/step101-drive-patch-preboot-invalid-state-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: the route-level `PATCH /drives/{drive_id}` proof now includes the preboot invalid-state rejection, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Balloon statistics preboot GET invalid-state rejection captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `GET /balloon/statistics` rejects preboot reads with the pinned Firecracker invalid-state envelope
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice reuses the already-aligned router-level invalid-state guard, adds explicit full-router proof for `GET /balloon/statistics`, and expands the behavior matrix to `37` routes and `117` pinned cases
- Harness update: added an inferred upstream baseline note for the preboot `GET /balloon/statistics` invalid-state path, added full-router balloon-statistics preboot-read coverage in `tests/api_contract.rs`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new `GET /balloon/statistics` preboot invalid-state case, added `run/WOR-20/api/step100-balloon-stats-get-preboot-invalid-state-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: the route-level `GET /balloon/statistics` proof now includes the preboot invalid-state rejection, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Balloon hinting-stop preboot PATCH invalid-state rejection captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `PATCH /balloon/hinting/stop` rejects preboot control requests with the pinned Firecracker invalid-state envelope
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice reuses the already-aligned router-level invalid-state guard, adds explicit full-router proof for `PATCH /balloon/hinting/stop`, and expands the behavior matrix to `37` routes and `116` pinned cases
- Harness update: added an inferred upstream baseline note for the preboot `PATCH /balloon/hinting/stop` invalid-state path, added full-router balloon-hinting-stop preboot-control coverage in `tests/api_contract.rs`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new `PATCH /balloon/hinting/stop` preboot invalid-state case, added `run/WOR-20/api/step99-balloon-hinting-stop-preboot-invalid-state-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: the route-level `PATCH /balloon/hinting/stop` proof now includes the preboot invalid-state rejection, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Balloon hinting-start preboot PATCH invalid-state rejection captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `PATCH /balloon/hinting/start` rejects preboot control requests with the pinned Firecracker invalid-state envelope
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice reuses the already-aligned router-level invalid-state guard, adds explicit full-router proof for `PATCH /balloon/hinting/start`, and expands the behavior matrix to `37` routes and `115` pinned cases
- Harness update: added an inferred upstream baseline note for the preboot `PATCH /balloon/hinting/start` invalid-state path, added full-router balloon-hinting-start preboot-control coverage in `tests/api_contract.rs`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new `PATCH /balloon/hinting/start` preboot invalid-state case, added `run/WOR-20/api/step98-balloon-hinting-start-preboot-invalid-state-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: the route-level `PATCH /balloon/hinting/start` proof now includes the preboot invalid-state rejection, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Balloon hinting-status preboot GET invalid-state rejection captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `GET /balloon/hinting/status` rejects preboot reads with the pinned Firecracker invalid-state envelope
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice reuses the already-aligned router-level invalid-state guard, adds explicit full-router proof for `GET /balloon/hinting/status`, and expands the behavior matrix to `37` routes and `114` pinned cases
- Harness update: added an inferred upstream baseline note for the preboot `GET /balloon/hinting/status` invalid-state path, added full-router balloon-hinting-status preboot-read coverage in `tests/api_contract.rs`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new `GET /balloon/hinting/status` preboot invalid-state case, added `run/WOR-20/api/step97-balloon-hinting-status-preboot-invalid-state-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: the route-level `GET /balloon/hinting/status` proof now includes the preboot invalid-state rejection, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Balloon statistics preboot PATCH invalid-state rejection captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `PATCH /balloon/statistics` rejects preboot stats updates with the pinned Firecracker invalid-state envelope
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice reuses the already-aligned router-level invalid-state guard, adds explicit full-router proof for `PATCH /balloon/statistics`, and expands the behavior matrix to `37` routes and `113` pinned cases
- Harness update: added an inferred upstream baseline note for the preboot `PATCH /balloon/statistics` invalid-state path, added full-router balloon-stats preboot-update coverage in `tests/api_contract.rs`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new `PATCH /balloon/statistics` preboot invalid-state case, added `run/WOR-20/api/step96-balloon-stats-patch-preboot-invalid-state-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: the route-level `PATCH /balloon/statistics` proof now includes the preboot invalid-state rejection, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Balloon preboot PATCH invalid-state rejection captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `PATCH /balloon` rejects preboot target updates with the pinned Firecracker invalid-state envelope
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice reuses the already-aligned router-level invalid-state guard, adds explicit full-router proof for `PATCH /balloon`, and expands the behavior matrix to `37` routes and `112` pinned cases
- Harness update: added an inferred upstream baseline note for the preboot `PATCH /balloon` invalid-state path, added full-router balloon preboot-target-update coverage in `tests/api_contract.rs`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new `PATCH /balloon` preboot invalid-state case, added `run/WOR-20/api/step95-balloon-patch-preboot-invalid-state-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: the route-level `PATCH /balloon` proof now includes the preboot invalid-state rejection, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Memory hotplug preboot PATCH invalid-state rejection captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `PATCH /hotplug/memory` rejects preboot resize requests with the pinned Firecracker invalid-state envelope
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice reuses the already-aligned router-level invalid-state guard, adds explicit full-router proof for `PATCH /hotplug/memory`, and expands the behavior matrix to `37` routes and `111` pinned cases
- Harness update: added an inferred upstream baseline note for the preboot `PATCH /hotplug/memory` invalid-state path, added full-router memory-hotplug preboot-resize coverage in `tests/api_contract.rs`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new `PATCH /hotplug/memory` preboot invalid-state case, added `run/WOR-20/api/step94-hotplug-memory-patch-preboot-invalid-state-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: the route-level `PATCH /hotplug/memory` proof now includes the preboot invalid-state rejection, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Memory hotplug preboot GET invalid-state rejection captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `GET /hotplug/memory` rejects preboot reads with the pinned Firecracker invalid-state envelope
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice reuses the already-aligned router-level invalid-state guard, adds explicit full-router proof for `GET /hotplug/memory`, and expands the behavior matrix to `37` routes and `110` pinned cases
- Harness update: added an inferred upstream baseline note for the preboot `GET /hotplug/memory` invalid-state path, added full-router memory-hotplug preboot-read coverage in `tests/api_contract.rs`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new `GET /hotplug/memory` preboot invalid-state case, added `run/WOR-20/api/step93-hotplug-memory-get-preboot-invalid-state-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: the route-level `GET /hotplug/memory` proof now includes the preboot invalid-state rejection, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Vsock postboot invalid-state rejection captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `PUT /vsock` rejects postboot updates with the pinned Firecracker invalid-state envelope
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice reuses the already-aligned router-level invalid-state guard, adds explicit full-router proof for `PUT /vsock`, and expands the behavior matrix to `37` routes and `109` pinned cases
- Harness update: added an inferred upstream baseline note for the `PUT /vsock` postboot-invalid-state path, added full-router vsock postboot-invalid-state coverage in `tests/api_contract.rs`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new `PUT /vsock` postboot invalid-state case, added `run/WOR-20/api/step92-vsock-postboot-invalid-state-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: the route-level `PUT /vsock` proof now includes the postboot invalid-state rejection, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Memory hotplug postboot invalid-state rejection captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `PUT /hotplug/memory` rejects postboot updates with the pinned Firecracker invalid-state envelope
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice reuses the already-aligned router-level invalid-state guard, adds explicit full-router proof for `PUT /hotplug/memory`, and expands the behavior matrix to `37` routes and `108` pinned cases
- Harness update: reused the direct upstream step-6 hotplug note for the runtime rejection path, added full-router memory-hotplug postboot-invalid-state coverage in `tests/api_contract.rs`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new `PUT /hotplug/memory` postboot invalid-state case, added `run/WOR-20/api/step91-hotplug-memory-postboot-invalid-state-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: the route-level `PUT /hotplug/memory` proof now includes the postboot invalid-state rejection, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Entropy postboot invalid-state rejection captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `PUT /entropy` rejects postboot updates with the pinned Firecracker invalid-state envelope
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice reuses the already-aligned router-level invalid-state guard, adds explicit full-router proof for `PUT /entropy`, and expands the behavior matrix to `37` routes and `107` pinned cases
- Harness update: added an entropy postboot baseline note grounded in the pinned entropy request-path evidence and neighboring `OperationNotSupportedPostBoot` captures, added full-router entropy postboot-invalid-state coverage in `tests/api_contract.rs`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new `PUT /entropy` postboot invalid-state case, added `run/WOR-20/api/step90-entropy-postboot-invalid-state-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: the route-level `PUT /entropy` proof now includes the postboot invalid-state rejection, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — CPU-config postboot invalid-state rejection captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `PUT /cpu-config` rejects postboot updates with the pinned Firecracker invalid-state envelope
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice reuses the now-aligned router-level invalid-state guard, adds explicit full-router proof for `PUT /cpu-config`, and expands the behavior matrix to `37` routes and `106` pinned cases
- Harness update: added a pinned upstream baseline note for the `PutCpuConfiguration -> OperationNotSupportedPostBoot` path, added full-router cpu-config postboot-invalid-state coverage in `tests/api_contract.rs`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new `PUT /cpu-config` postboot invalid-state case, added `run/WOR-20/api/step89-cpu-config-postboot-invalid-state-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: the route-level `PUT /cpu-config` proof now includes the postboot invalid-state rejection, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Boot-source postboot invalid-state rejection captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `PUT /boot-source` rejects postboot updates with the pinned Firecracker invalid-state envelope
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice reuses the now-aligned router-level invalid-state guard, adds explicit full-router proof for `PUT /boot-source`, and expands the behavior matrix to `37` routes and `105` pinned cases
- Harness update: added a pinned upstream baseline note for the `ConfigureBootSource -> OperationNotSupportedPostBoot` path, added full-router boot-source postboot-invalid-state coverage in `tests/api_contract.rs`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the new `PUT /boot-source` postboot invalid-state case, added `run/WOR-20/api/step88-boot-source-postboot-invalid-state-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: the route-level `PUT /boot-source` proof now includes the postboot invalid-state rejection, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — MMDS config postboot invalid-state rejection captured and aligned
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `PUT /mmds/config` rejects postboot updates with the pinned Firecracker invalid-state envelope
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice exposed a router-level message mismatch, aligned the preboot/postboot route guard with Firecracker's canonical invalid-state messages, and expanded the behavior matrix to `37` routes and `104` pinned cases
- Harness update: added a pinned upstream baseline note for the `SetMmdsConfiguration -> OperationNotSupportedPostBoot` path, added full-router MMDS-config postboot-invalid-state coverage in `tests/api_contract.rs`, updated `src/api.rs` so router-level preboot/postboot guards emit Firecracker's canonical invalid-state strings, added `run/WOR-20/api/step87-mmds-config-runtime-invalid-state-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: the route-level `PUT /mmds/config` proof now includes the postboot invalid-state rejection with the upstream-backed message, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — MMDS store runtime missing-config rejection captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `PUT /mmds` rejects runtime writes without MMDS networking configuration
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice adds explicit full-router contract proof for the existing MMDS runtime store-access guard and expands the behavior matrix to `37` routes and `103` pinned cases
- Harness update: added full-router MMDS runtime-store-missing-config coverage in `tests/api_contract.rs`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the pinned `PUT /mmds` runtime missing-config rejection backed by `run/WOR-20/api/step7-upstream-mmds-baseline.md`, added `run/WOR-20/api/step86-mmds-put-runtime-missing-config-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: the route-level `PUT /mmds` proof now includes the runtime missing-config rejection, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — MMDS patch runtime missing-config rejection captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `PATCH /mmds` rejects runtime writes without MMDS networking configuration
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice adds explicit full-router contract proof for the existing MMDS runtime patch-access guard and expands the behavior matrix to `37` routes and `102` pinned cases
- Harness update: added full-router MMDS patch-runtime-missing-config coverage in `tests/api_contract.rs`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the pinned `PATCH /mmds` runtime missing-config rejection backed by `run/WOR-20/api/step7-upstream-mmds-baseline.md`, added `run/WOR-20/api/step85-mmds-patch-runtime-missing-config-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: the route-level `PATCH /mmds` proof now includes the runtime missing-config rejection, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — MMDS patch missing-store rejection captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `PATCH /mmds` rejects writes before the MMDS store is initialized
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice adds explicit full-router contract proof for the existing MMDS preboot missing-store guard and expands the behavior matrix to `37` routes and `101` pinned cases
- Harness update: added full-router MMDS patch-missing-store coverage in `tests/api_contract.rs`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the pinned `PATCH /mmds` uninitialized-store rejection backed by `run/WOR-20/api/step7-upstream-mmds-baseline.md`, added `run/WOR-20/api/step84-mmds-patch-missing-store-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: the route-level `PATCH /mmds` proof now includes the preboot missing-store rejection, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied


### 2026-04-07 — WOR-20 — MMDS preboot initialized-store read captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `GET /mmds` returns the initialized datastore before boot
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice adds explicit full-router contract proof for the existing MMDS preboot initialized-store read path and expands the behavior matrix to `37` routes and `100` pinned cases
- Harness update: added full-router preboot initialized-store coverage in `tests/api_contract.rs`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the pinned `GET /mmds` preboot initialized-store success case backed by `run/WOR-20/api/step7-upstream-mmds-baseline.md`, added `run/WOR-20/api/step83-mmds-preboot-store-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: the route-level `GET /mmds` proof now includes the preboot initialized-store success case, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied


### 2026-04-07 — WOR-20 — MMDS runtime missing-config rejection captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `GET /mmds` rejects runtime reads without MMDS networking configuration
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice reuses existing full-router proof for the MMDS runtime-config guard and expands the behavior matrix to `37` routes and `99` pinned cases
- Harness update: extended `docs/specs/firecracker-api-behavior-captures.yaml` with the pinned `GET /mmds` runtime missing-config rejection backed by `run/WOR-20/api/step7-upstream-mmds-baseline.md`, added `run/WOR-20/api/step82-mmds-runtime-missing-config-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: the route-level `GET /mmds` proof now includes the runtime missing-config rejection, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied


### 2026-04-07 — WOR-20 — MMDS merge-patch oversize rejection captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `PATCH /mmds` rejects oversize merge-patch payloads
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice adds explicit full-router contract proof for the existing MMDS merge-patch overflow guard and expands the behavior matrix to `37` routes and `98` pinned cases
- Harness update: added full-router MMDS patch-oversize coverage in `tests/api_contract.rs`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the pinned `PATCH /mmds` oversize-merge rejection backed by `run/WOR-20/api/step7-upstream-mmds-baseline.md`, added `run/WOR-20/api/step81-mmds-patch-oversize-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: the route-level `PATCH /mmds` proof now includes rejection of oversize merge-patch payloads, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied


### 2026-04-07 — WOR-20 — MMDS oversize-store rejection captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `PUT /mmds` rejects oversized datastore payloads
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice adds explicit full-router contract proof for the existing MMDS overflow guard and expands the behavior matrix to `37` routes and `97` pinned cases
- Harness update: added full-router MMDS oversize coverage in `tests/api_contract.rs`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the pinned `PUT /mmds` oversized-store rejection backed by `run/WOR-20/api/step7-upstream-mmds-baseline.md`, added `run/WOR-20/api/step80-mmds-oversize-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: the route-level `PUT /mmds` proof now includes rejection of oversized datastore payloads, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied


### 2026-04-07 — WOR-20 — MMDS config invalid-IPv4 rejection captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that preboot `PUT /mmds/config` rejects non-link-local IPv4 addresses
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice adds explicit full-router contract proof for the existing invalid-IPv4 MMDS-config guard and expands the behavior matrix to `37` routes and `96` pinned cases
- Harness update: added full-router invalid-IPv4 coverage in `tests/api_contract.rs`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the pinned `PUT /mmds/config` non-link-local IPv4 rejection backed by `run/WOR-20/api/step7-upstream-mmds-baseline.md`, added `run/WOR-20/api/step79-mmds-invalid-ipv4-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: the route-level `PUT /mmds/config` proof now includes rejection of non-link-local IPv4 addresses, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied


### 2026-04-07 — WOR-20 — Balloon oversize rejection captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that preboot `PUT /balloon` rejects oversize balloon targets
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice adds route-level matrix proof for the existing preboot oversize-balloon guard and expands the behavior matrix to `37` routes and `95` pinned cases
- Harness update: extended `docs/specs/firecracker-api-behavior-captures.yaml` with the pinned `PUT /balloon` oversize rejection backed by `run/WOR-20/api/step9-upstream-balloon-config-baseline.md`, added `run/WOR-20/api/step78-balloon-oversize-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: the balloon create route now records the pinned preboot oversize rejection in the generated behavior matrix, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Pmem root-device conflicts captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that preboot `PUT /pmem/{id}` rejects block-root and duplicate-pmem-root conflicts
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice adds route-level matrix proof for the existing preboot pmem root-device conflict guards and expands the behavior matrix to `37` routes and `94` pinned cases
- Harness update: extended `docs/specs/firecracker-api-behavior-captures.yaml` with the pinned `PUT /pmem/{id}` block-root and duplicate-pmem-root conflict rejections backed by `run/WOR-20/api/step16-upstream-pmem-create-baseline.md`, added `run/WOR-20/api/step77-pmem-root-conflicts-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: the pmem create route now records the pinned preboot root-device conflict envelopes in the generated behavior matrix, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Drive root-device conflicts captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that preboot `PUT /drives/{drive_id}` rejects pmem-root and duplicate-block-root conflicts
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice adds route-level matrix proof for the existing preboot drive root-device conflict guards and expands the behavior matrix to `37` routes and `92` pinned cases
- Harness update: extended `docs/specs/firecracker-api-behavior-captures.yaml` with the pinned `PUT /drives/{drive_id}` pmem-root and duplicate-block-root conflict rejections backed by `run/WOR-20/api/step17-upstream-drive-create-baseline.md`, added `run/WOR-20/api/step76-drive-root-conflicts-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: the drive create route now records the pinned preboot root-device conflict envelopes in the generated behavior matrix, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Machine-config balloon floor guards captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that preboot `PUT /machine-config` and `PATCH /machine-config` reject `mem_size_mib` below a configured balloon target
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice adds route-level matrix proof for the existing machine-config balloon-floor guards and expands the behavior matrix to `37` routes and `90` pinned cases
- Harness update: extended `docs/specs/firecracker-api-behavior-captures.yaml` with the pinned preboot `PUT /machine-config` and `PATCH /machine-config` balloon-target floor rejections backed by `run/WOR-20/api/step9-upstream-balloon-config-baseline.md`, added `run/WOR-20/api/step75-machine-config-balloon-floor-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: the machine-config write-route proof now includes the pinned preboot balloon-target floor rejection for both `PUT` and `PATCH`, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Network-create path/body mismatch captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that preboot `PUT /network-interfaces/{iface_id}` rejects a path/body `iface_id` mismatch
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice adds explicit full-router proof for the existing preboot network-create parser guard and expands the behavior matrix to `37` routes and `88` pinned cases
- Harness update: added full-router network-create mismatch coverage in `tests/api_contract.rs`, extended `docs/specs/firecracker-api-behavior-captures.yaml` with the pinned `PUT /network-interfaces/{iface_id}` mismatch case backed by `run/WOR-20/api/step15-upstream-network-update-baseline.md`, added `run/WOR-20/api/step74-network-create-id-mismatch-note.md`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: the route-level `PUT /network-interfaces/{iface_id}` proof now includes the pinned preboot path/body `iface_id` mismatch rejection, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — MMDS config unknown-network-interface rejection captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `PUT /mmds/config` rejects non-existent network interface IDs
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice adds explicit full-router contract proof for the existing unknown-network-interface MMDS-config guard and expands the behavior matrix to `37` routes and `87` pinned cases
- Harness update: added `run/WOR-20/api/step73-upstream-mmds-config-unknown-interface-baseline.md`, added full-router unknown-network-interface coverage in `tests/api_contract.rs`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json` with the new pinned `PUT /mmds/config` invalid-request case
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: the route-level `PUT /mmds/config` proof now includes rejection of non-existent network interface IDs, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — MMDS config empty-network-list rejection captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `PUT /mmds/config` rejects an empty `network_interfaces` list
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice adds explicit full-router contract proof for the existing empty-network-list MMDS-config guard and expands the behavior matrix to `37` routes and `86` pinned cases
- Harness update: added `run/WOR-20/api/step72-upstream-mmds-config-empty-network-list-baseline.md`, added full-router empty-network-list coverage in `tests/api_contract.rs`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json` with the new pinned `PUT /mmds/config` invalid-request case
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: the route-level `PUT /mmds/config` proof now includes the pinned empty-network-interface-list rejection, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Vsock invalid guest-CID rejection captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `PUT /vsock` rejects `guest_cid` values below the pinned minimum of `3`
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice adds explicit full-router contract proof for the existing invalid-`guest_cid` guard and expands the behavior matrix to `37` routes and `85` pinned cases
- Harness update: added `run/WOR-20/api/step71-upstream-vsock-invalid-cid-baseline.md`, added full-router invalid-`guest_cid` coverage in `tests/api_contract.rs`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json` with the new pinned `PUT /vsock` invalid-request case
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: the route-level `PUT /vsock` proof now includes the pinned invalid-`guest_cid` rejection, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Metrics duplicate-init guard captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `PUT /metrics` rejects duplicate initialization after a successful earlier configuration
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice adds explicit full-router contract proof for duplicate metrics initialization and expands the behavior matrix to `37` routes and `84` pinned cases
- Harness update: added `run/WOR-20/api/step70-upstream-metrics-reinit-baseline.md`, added full-router duplicate-init coverage in `tests/api_contract.rs`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json` with the new pinned `PUT /metrics` invalid-request case
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: the route-level proof for `PUT /metrics` now includes the pinned duplicate-initialization rejection, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Snapshot-load entropy guard captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `PUT /snapshot/load` rejects a valid request after preboot `PUT /entropy`
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice adds explicit unit plus full-router contract proof for the snapshot-load entropy guard and expands the behavior matrix to `37` routes and `83` pinned cases
- Harness update: added `run/WOR-20/api/step69-upstream-snapshot-load-entropy-baseline.md`, added snapshot-load entropy guard coverage in `src/api.rs` and `tests/api_contract.rs`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json` with the new pinned `PUT /snapshot/load` invalid-state case
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: the route-level snapshot-load guard evidence now includes preboot entropy configuration, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Snapshot-load MMDS guard captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `PUT /snapshot/load` rejects a valid request after preboot `PUT /mmds/config`
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice adds explicit unit plus full-router contract proof for the snapshot-load MMDS guard and expands the behavior matrix to `37` routes and `82` pinned cases
- Harness update: added `run/WOR-20/api/step68-upstream-snapshot-load-mmds-baseline.md`, added full-router snapshot-load MMDS guard coverage in `tests/api_contract.rs`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json` with the new pinned `PUT /snapshot/load` invalid-state case
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: the route-level snapshot-load guard evidence now includes preboot MMDS networking configuration, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Snapshot-load vsock guard captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `PUT /snapshot/load` rejects a valid request after preboot `PUT /vsock`
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice adds explicit unit plus full-router contract proof for the snapshot-load vsock guard and expands the behavior matrix to `37` routes and `81` pinned cases
- Harness update: added `run/WOR-20/api/step67-upstream-snapshot-load-vsock-baseline.md`, added snapshot-load vsock guard coverage in `src/api.rs` and `tests/api_contract.rs`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json` with the new pinned `PUT /snapshot/load` invalid-state case
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: the route-level snapshot-load guard evidence now includes preboot vsock configuration, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Snapshot-load cpu-config guard captured in the behavior matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed route-level proof that `PUT /snapshot/load` rejects a valid request after preboot `PUT /cpu-config`
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved with documented gaps; the slice adds explicit unit plus full-router contract proof for the snapshot-load cpu-config guard and expands the behavior matrix to `37` routes and `80` pinned cases
- Harness update: added `run/WOR-20/api/step66-upstream-snapshot-load-cpu-config-baseline.md`, added snapshot-load cpu-config guard coverage in `src/api.rs` and `tests/api_contract.rs`, and regenerated `docs/generated/firecracker-api-behavior-matrix.json` with the new pinned `PUT /snapshot/load` invalid-state case
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; remote push and CI are still pending from this local workspace; `src/api.rs` remains the long-standing architectural hotspot
- Truthful project state after this milestone: the route-level snapshot-load guard evidence now includes preboot cpu-config configuration, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Snapshot create/load placeholder responses replaced with pinned stub success semantics
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: return pinned `204` stub success semantics for `PUT /snapshot/create` and `PUT /snapshot/load` after the existing parser and state guards
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: pending local sequential council for this attempt; the slice removes the last explicit snapshot `501` placeholders, keeps the existing parser and invalid-state envelopes, and advances snapshot-load state to `Paused` by default or `Running` when `resume_vm=true`
- Harness update: removed the snapshot routes from `docs/specs/firecracker-api-baseline-captures.yaml`, regenerated `docs/generated/firecracker-api-surface-stub-matrix.json` to `37` `not_applicable` routes and `0` placeholder captures, kept `docs/generated/firecracker-api-behavior-matrix.json` at `37` concrete routes and `79` pinned cases, and fixed `scripts/harness/check_structure.py` so it recognizes the current explicit `/actions` dispatch
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; `python3 scripts/harness/check_structure.py --root .` still fails on the long-standing `src/api.rs` size budget breach (`8073` lines vs `3200` error budget); remote push and CI are still pending from this local workspace
- Truthful project state after this milestone: the Phase 0 API surface no longer serves snapshot create/load through placeholder `501` responses, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar, remote evidence, and human review are satisfied

### 2026-04-07 — WOR-20 — Drive backing-file parity aligned to pinned baseline
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: reject preboot `PUT /drives/{drive_id}` missing backing files with pinned Firecracker envelopes
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; `PUT /drives/{drive_id}` now records one success case plus one pinned preboot missing-backing-file failure case, and the route-level behavior matrix remains fully captured with `37` routes, `79` total pinned cases, and `0` operations left `pending_baseline_capture`
- Harness update: aligned `handle_put_drive_stub()` to reject missing backing files after the existing id/root-order guards, added unit plus contract proof for the pinned failure envelope and unchanged `/vm/config` export after rejected inserts, regenerated `docs/generated/firecracker-api-behavior-matrix.json`, and recorded the upstream source note in `run/WOR-20/api/step64-upstream-drive-backing-file-baseline.md`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; snapshot placeholder routes remain intentionally unimplemented after parser and state validation; remote push and CI are still pending from this local workspace
- Truthful project state after this milestone: the drive route family no longer under-models the pinned preboot backing-file validation surface for missing host files, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar is satisfied

### 2026-04-06 — WOR-20 — Pmem backing-file parity aligned to pinned baseline
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: reject preboot `PUT /pmem/{id}` missing and zero-sized backing files with pinned Firecracker envelopes
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; `PUT /pmem/{id}` now records one success case plus two pinned preboot backing-file failure cases, and the route-level behavior matrix remains fully captured with `37` routes, `78` total pinned cases, and `0` operations left `pending_baseline_capture`
- Harness update: aligned `handle_put_pmem_stub()` to reject missing and zero-sized backing files after the existing id/root-order guards, added unit plus contract proof for both failure envelopes and unchanged `/vm/config` export after rejected inserts, regenerated `docs/generated/firecracker-api-behavior-matrix.json`, and recorded the upstream source note in `run/WOR-20/api/step63-upstream-pmem-backing-file-baseline.md`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; snapshot placeholder routes remain intentionally unimplemented after parser and state validation; remote push and CI are still pending from this local workspace
- Truthful project state after this milestone: the pmem route family no longer under-models the pinned preboot backing-file validation surface for missing or zero-sized host files, but GH `#2` remains in `Verification` until the broader umbrella acceptance bar is satisfied

### 2026-04-06 — WOR-20 — Network-create conflict parity aligned to pinned baseline
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: reject preboot network-create guest-MAC and host tap conflicts with pinned Firecracker envelopes
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; `PUT /network-interfaces/{iface_id}` now records one success case plus two pinned preboot conflict cases, and the route-level behavior matrix remains fully captured with `37` routes, `76` total pinned cases, and `0` operations left `pending_baseline_capture`
- Harness update: aligned `handle_put_network_interface_stub()` to reject duplicate guest-MAC and host tap reuse across different interfaces, added explicit contract proof for both preboot failure envelopes plus unchanged `/vm/config` export after failed inserts, regenerated `docs/generated/firecracker-api-behavior-matrix.json`, and recorded the upstream source note in `run/WOR-20/api/step62-upstream-network-create-conflict-baseline.md`
- Remaining risks: GH `#2` still needs broader endpoint-level parity beyond the seeded route matrix; snapshot placeholder routes remain intentionally unimplemented after parser and state validation; remote GitHub/tracker updates, push, and CI are still pending from this local workspace
- Truthful project state after this milestone: the network-create route family no longer under-models the pinned preboot conflict surface for duplicate guest-MAC or host tap reuse, but GH `#2` remains in `Verification` until the broader issue acceptance bar is satisfied and remote evidence is updated

### 2026-04-06 — WOR-20 — Network-create behavior matrix seeded
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed the final pending preboot `PUT /network-interfaces/{iface_id}` success path in the behavior matrix
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the route-level behavior matrix now captures all currently seeded concrete-handler and matrix-backed placeholder operations with `37` captured routes, `74` total pinned cases, and `0` operations left `pending_baseline_capture`
- Harness update: added one pinned preboot network-create success case to `docs/specs/firecracker-api-behavior-captures.yaml`, added explicit full-router proof in `tests/api_contract.rs`, regenerated `docs/generated/firecracker-api-behavior-matrix.json`, and recorded the slice note in `run/WOR-20/api/step61-network-create-behavior-matrix-note.md`
- Remaining risks: the seeded route matrix still does not by itself prove full endpoint-level parity for every handler; fuller network-create MAC-conflict and tap-open parity remain open; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the generated route-level behavior matrix no longer has any `pending_baseline_capture` operations, but GH `#2` remains in `Verification` until remote evidence is updated and a human decides whether the now-complete capture set is sufficient for the broader issue acceptance bar

### 2026-04-06 — WOR-20 — Snapshot placeholder behavior matrix seeded
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed the snapshot placeholder parser, guard, and current `501` behavior in the behavior matrix
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the route-level behavior matrix now captures all `GET` routes plus `PUT /actions`, `PUT /balloon`, `PUT /boot-source`, `PUT /cpu-config`, `PUT /drives/{drive_id}`, `PUT /entropy`, `PUT /hotplug/memory`, `PUT /logger`, `PUT /machine-config`, `PUT /metrics`, `PUT /mmds`, `PUT /mmds/config`, `PUT /pmem/{id}`, `PUT /serial`, `PUT /snapshot/create`, `PUT /snapshot/load`, `PUT /vsock`, `PATCH /vm`, `PATCH /drives/{drive_id}`, `PATCH /network-interfaces/{iface_id}`, `PATCH /balloon`, `PATCH /balloon/statistics`, `PATCH /balloon/hinting/start`, `PATCH /balloon/hinting/stop`, `PATCH /machine-config`, `PATCH /mmds`, and `PATCH /hotplug/memory`; GH `#2` still needs one final route capture
- Harness update: added nine pinned snapshot-route cases to `docs/specs/firecracker-api-behavior-captures.yaml`, regenerated `docs/generated/firecracker-api-behavior-matrix.json` to `36` captured routes and `73` total cases, and recorded the slice note in `run/WOR-20/api/step60-snapshot-behavior-matrix-note.md`
- Remaining risks: only `1` operation remains `pending_baseline_capture` (`PUT /network-interfaces/{iface_id}`); full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the repo now records the snapshot placeholder parser, guard, and current `501` path in the generated behavior matrix, but GH `#2` remains in `Verification` until the final network-interface create gap is captured or otherwise truthfully resolved

### 2026-04-06 — WOR-20 — Drive and pmem write behavior matrix seeded
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed low-risk drive and pmem preboot write-path success cases in the behavior matrix
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the route-level behavior matrix now captures all `GET` routes plus `PUT /actions`, `PUT /balloon`, `PUT /boot-source`, `PUT /cpu-config`, `PUT /drives/{drive_id}`, `PUT /entropy`, `PUT /hotplug/memory`, `PUT /logger`, `PUT /machine-config`, `PUT /metrics`, `PUT /mmds`, `PUT /mmds/config`, `PUT /pmem/{id}`, `PUT /serial`, `PUT /vsock`, `PATCH /vm`, `PATCH /drives/{drive_id}`, `PATCH /network-interfaces/{iface_id}`, `PATCH /balloon`, `PATCH /balloon/statistics`, `PATCH /balloon/hinting/start`, `PATCH /balloon/hinting/stop`, `PATCH /machine-config`, `PATCH /mmds`, and `PATCH /hotplug/memory`; GH `#2` still needs broader route coverage and more endpoint-level capture work
- Harness update: added pinned drive and pmem preboot success cases to `docs/specs/firecracker-api-behavior-captures.yaml`, regenerated `docs/generated/firecracker-api-behavior-matrix.json` to `34` captured routes and `64` total cases, and recorded the slice note in `run/WOR-20/api/step59-drive-pmem-write-behavior-matrix-note.md`
- Remaining risks: only `3` operations remain `pending_baseline_capture` (`PUT /network-interfaces/{iface_id}`, `PUT /snapshot/create`, and `PUT /snapshot/load`); full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the repo now records the drive and pmem preboot success surface in the generated behavior matrix, but GH `#2` remains in `Verification` because broader endpoint-level status/error parity across the remaining network-interface and snapshot routes is still incomplete

### 2026-04-06 — WOR-20 — Singleton store write behavior matrix seeded
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed low-risk singleton store-route success cases in the behavior matrix
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the route-level behavior matrix now captures all `GET` routes plus `PUT /actions`, `PUT /balloon`, `PUT /boot-source`, `PUT /cpu-config`, `PUT /entropy`, `PUT /hotplug/memory`, `PUT /logger`, `PUT /machine-config`, `PUT /metrics`, `PUT /mmds`, `PUT /mmds/config`, `PUT /serial`, `PUT /vsock`, `PATCH /vm`, `PATCH /drives/{drive_id}`, `PATCH /network-interfaces/{iface_id}`, `PATCH /balloon`, `PATCH /balloon/statistics`, `PATCH /balloon/hinting/start`, `PATCH /balloon/hinting/stop`, `PATCH /machine-config`, `PATCH /mmds`, and `PATCH /hotplug/memory`; GH `#2` still needs broader route coverage and more endpoint-level capture work
- Harness update: added ten pinned singleton store-route success cases to `docs/specs/firecracker-api-behavior-captures.yaml`, regenerated `docs/generated/firecracker-api-behavior-matrix.json` to `32` captured routes and `62` total cases, and recorded the slice note in `run/WOR-20/api/step58-singleton-store-write-behavior-matrix-note.md`
- Remaining risks: only `5` operations remain `pending_baseline_capture` (`PUT /drives/{drive_id}`, `PUT /network-interfaces/{iface_id}`, `PUT /pmem/{id}`, `PUT /snapshot/create`, and `PUT /snapshot/load`); full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the repo now records the low-risk singleton write success surface in the generated behavior matrix, but GH `#2` remains in `Verification` because broader endpoint-level status/error parity across the remaining drive, network, pmem, and snapshot routes is still incomplete

### 2026-04-06 — WOR-20 — Config-store write behavior matrix seeded
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed low-risk machine-config and MMDS write-path success cases in the behavior matrix
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the route-level behavior matrix now captures `GET /`, `GET /balloon`, `GET /balloon/hinting/status`, `GET /balloon/statistics`, `GET /hotplug/memory`, `GET /machine-config`, `GET /mmds`, `GET /version`, `GET /vm/config`, `PUT /actions`, `PUT /machine-config`, `PUT /mmds`, `PUT /mmds/config`, `PATCH /vm`, `PATCH /drives/{drive_id}`, `PATCH /network-interfaces/{iface_id}`, `PATCH /balloon`, `PATCH /balloon/statistics`, `PATCH /balloon/hinting/start`, `PATCH /balloon/hinting/stop`, `PATCH /machine-config`, and `PATCH /mmds`, but GH `#2` still needs broader route coverage and more endpoint-level capture work
- Harness update: added five pinned write-path success cases across machine-config and MMDS routes to `docs/specs/firecracker-api-behavior-captures.yaml`, regenerated `docs/generated/firecracker-api-behavior-matrix.json` to `22` captured routes and `52` total cases, and recorded the slice note in `run/WOR-20/api/step57-config-store-write-behavior-matrix-note.md`
- Remaining risks: `15` operations still remain `pending_baseline_capture`; all `GET` routes are now represented in the behavior matrix, but the remaining open surface is still concentrated in the other write-path and runtime-sensitive routes; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the repo now has pinned behavior-matrix scenarios for the read surface plus another low-risk write-family tranche, but GH `#2` remains in `Verification` because broader endpoint-level status/error parity across the remaining write-path and runtime-sensitive routes is still incomplete

### 2026-04-06 — WOR-20 — Full VM config behavior matrix seeded
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed pinned `GET /vm/config` export cases from already-proven upstream-backed config families
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the route-level behavior matrix now captures `GET /`, `GET /balloon`, `GET /balloon/hinting/status`, `GET /balloon/statistics`, `GET /hotplug/memory`, `GET /machine-config`, `GET /mmds`, `GET /version`, `GET /vm/config`, `PUT /actions`, `PATCH /vm`, `PATCH /drives/{drive_id}`, `PATCH /network-interfaces/{iface_id}`, `PATCH /balloon`, `PATCH /balloon/statistics`, `PATCH /balloon/hinting/start`, and `PATCH /balloon/hinting/stop`, but GH `#2` still needs broader route coverage and more endpoint-level capture work
- Harness update: added three pinned `GET /vm/config` export-presence cases to `docs/specs/firecracker-api-behavior-captures.yaml`, regenerated `docs/generated/firecracker-api-behavior-matrix.json` to `17` captured routes and `47` total cases, and recorded the slice note in `run/WOR-20/api/step55-vm-config-behavior-matrix-note.md`
- Remaining risks: `20` operations still remain `pending_baseline_capture`; all `GET` routes are now represented in the behavior matrix, but the remaining open surface is still concentrated in write-path and runtime-sensitive routes; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the repo now has at least one pinned behavior-matrix scenario for every `GET` route on the Firecracker surface, but GH `#2` remains in `Verification` because broader endpoint-level status/error parity across the remaining write-path and runtime-sensitive routes is still incomplete

### 2026-04-06 — WOR-20 — MMDS read behavior matrix seeded
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: teach the behavior matrix to assert root documents and seed pinned `GET /mmds` read cases
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the route-level behavior matrix now captures `GET /`, `GET /balloon`, `GET /balloon/hinting/status`, `GET /balloon/statistics`, `GET /hotplug/memory`, `GET /machine-config`, `GET /mmds`, `GET /version`, `PUT /actions`, `PATCH /vm`, `PATCH /drives/{drive_id}`, `PATCH /network-interfaces/{iface_id}`, `PATCH /balloon`, `PATCH /balloon/statistics`, `PATCH /balloon/hinting/start`, and `PATCH /balloon/hinting/stop`, but GH `#2` still needs broader route coverage and more endpoint-level capture work
- Harness update: extended `src/api_behavior_matrix.rs` so success-body assertions can target the root document with an empty JSON pointer, added two pinned `GET /mmds` cases to `docs/specs/firecracker-api-behavior-captures.yaml`, regenerated `docs/generated/firecracker-api-behavior-matrix.json` to `16` captured routes and `44` total cases, and recorded the slice note in `run/WOR-20/api/step54-mmds-read-behavior-matrix-note.md`
- Remaining risks: `21` operations still remain `pending_baseline_capture`; the only uncaptured `GET` route is now `GET /vm/config`, whose broader nested export shape likely needs larger scoped captures or another matcher improvement; broader endpoint-level parity across the rest of the route set is still incomplete; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the repo now records the already-proven MMDS read surface in the generated behavior matrix and no longer needs a harness workaround for root-object success bodies, but GH `#2` remains in `Verification` because much broader endpoint-level status/error parity still needs capture and, in some areas, implementation work

### 2026-04-06 — WOR-20 — Memory hotplug read behavior matrix seeded
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed the generated behavior matrix with a pinned `GET /hotplug/memory` runtime status case
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the route-level behavior matrix now captures `GET /`, `GET /balloon`, `GET /balloon/hinting/status`, `GET /balloon/statistics`, `GET /hotplug/memory`, `GET /machine-config`, `GET /version`, `PUT /actions`, `PATCH /vm`, `PATCH /drives/{drive_id}`, `PATCH /network-interfaces/{iface_id}`, `PATCH /balloon`, `PATCH /balloon/statistics`, `PATCH /balloon/hinting/start`, and `PATCH /balloon/hinting/stop`, but GH `#2` still needs broader route coverage and more endpoint-level capture work
- Harness update: added one pinned `GET /hotplug/memory` runtime status case to `docs/specs/firecracker-api-behavior-captures.yaml`, regenerated `docs/generated/firecracker-api-behavior-matrix.json` to `15` captured routes and `42` total cases, and recorded the slice note in `run/WOR-20/api/step53-memory-hotplug-behavior-matrix-note.md`
- Remaining risks: `22` operations still remain `pending_baseline_capture`; broader endpoint-level parity across the rest of the route set is still incomplete; the remaining uncaptured read routes now skew toward richer nested bodies or thinner isolated upstream proof, especially `GET /mmds` and `GET /vm/config`; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the repo now records the already-proven initial hotplug-memory runtime status surface in the generated behavior matrix, but GH `#2` remains in `Verification` because much broader endpoint-level status/error parity still needs capture and, in some areas, implementation work

### 2026-04-06 — WOR-20 — Balloon read behavior matrix seeded
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed the generated behavior matrix with pinned `GET /balloon` preboot and runtime cases
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the route-level behavior matrix now captures `GET /`, `GET /balloon`, `GET /balloon/hinting/status`, `GET /balloon/statistics`, `GET /machine-config`, `GET /version`, `PUT /actions`, `PATCH /vm`, `PATCH /drives/{drive_id}`, `PATCH /network-interfaces/{iface_id}`, `PATCH /balloon`, `PATCH /balloon/statistics`, `PATCH /balloon/hinting/start`, and `PATCH /balloon/hinting/stop`, but GH `#2` still needs broader route coverage and more endpoint-level capture work
- Harness update: added four pinned `GET /balloon` cases to `docs/specs/firecracker-api-behavior-captures.yaml`, regenerated `docs/generated/firecracker-api-behavior-matrix.json` to `14` captured routes and `41` total cases, and recorded the slice note in `run/WOR-20/api/step52-balloon-read-behavior-matrix-note.md`
- Remaining risks: `23` operations still remain `pending_baseline_capture`; broader endpoint-level parity across the rest of the route set is still incomplete; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the repo now records the already-proven balloon read surface in the generated behavior matrix, but GH `#2` remains in `Verification` because much broader endpoint-level status/error parity still needs capture and, in some areas, implementation work

### 2026-04-06 — WOR-20 — Balloon statistics behavior matrix seeded
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed the generated behavior matrix with pinned `GET /balloon/statistics` runtime cases
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the route-level behavior matrix now captures `GET /`, `GET /balloon/hinting/status`, `GET /balloon/statistics`, `GET /machine-config`, `GET /version`, `PUT /actions`, `PATCH /vm`, `PATCH /drives/{drive_id}`, `PATCH /network-interfaces/{iface_id}`, `PATCH /balloon`, `PATCH /balloon/statistics`, `PATCH /balloon/hinting/start`, and `PATCH /balloon/hinting/stop`, but GH `#2` still needs broader route coverage and more endpoint-level capture work
- Harness update: added three pinned `GET /balloon/statistics` cases to `docs/specs/firecracker-api-behavior-captures.yaml`, regenerated `docs/generated/firecracker-api-behavior-matrix.json` to `13` captured routes and `37` total cases, and recorded the slice note in `run/WOR-20/api/step51-balloon-statistics-behavior-matrix-note.md`
- Remaining risks: `24` operations still remain `pending_baseline_capture`; broader endpoint-level parity across the rest of the route set is still incomplete; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the repo now records the already-proven balloon statistics read surface in the generated behavior matrix, but GH `#2` remains in `Verification` because much broader endpoint-level status/error parity still needs capture and, in some areas, implementation work

### 2026-04-06 — WOR-20 — Machine config behavior matrix seeded
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed the generated behavior matrix with a pinned `GET /machine-config` success-body case
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the route-level behavior matrix now captures `GET /`, `GET /balloon/hinting/status`, `GET /machine-config`, `GET /version`, `PUT /actions`, `PATCH /vm`, `PATCH /drives/{drive_id}`, `PATCH /network-interfaces/{iface_id}`, `PATCH /balloon`, `PATCH /balloon/statistics`, `PATCH /balloon/hinting/start`, and `PATCH /balloon/hinting/stop`, but GH `#2` still needs broader route coverage and more endpoint-level capture work
- Harness update: added one pinned `GET /machine-config` case to `docs/specs/firecracker-api-behavior-captures.yaml`, regenerated `docs/generated/firecracker-api-behavior-matrix.json` to `12` captured routes and `34` total cases, and recorded the slice note in `run/WOR-20/api/step50-machine-config-behavior-matrix-note.md`
- Remaining risks: `25` operations still remain `pending_baseline_capture`; broader endpoint-level parity across the rest of the route set is still incomplete; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the repo now records the already-proven machine-config read surface in the generated behavior matrix, but GH `#2` remains in `Verification` because much broader endpoint-level status/error parity still needs capture and, in some areas, implementation work

### 2026-04-06 — WOR-20 — Balloon hinting status behavior matrix seeded
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed the generated behavior matrix with pinned runtime `GET /balloon/hinting/status` cases
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the route-level behavior matrix now captures `GET /`, `GET /balloon/hinting/status`, `GET /version`, `PUT /actions`, `PATCH /vm`, `PATCH /drives/{drive_id}`, `PATCH /network-interfaces/{iface_id}`, `PATCH /balloon`, `PATCH /balloon/statistics`, `PATCH /balloon/hinting/start`, and `PATCH /balloon/hinting/stop`, but GH `#2` still needs broader route coverage and more endpoint-level capture work
- Harness update: added three pinned `GET /balloon/hinting/status` cases to `docs/specs/firecracker-api-behavior-captures.yaml`, regenerated `docs/generated/firecracker-api-behavior-matrix.json` to `11` captured routes and `33` total cases, and recorded the slice note in `run/WOR-20/api/step49-balloon-hinting-status-behavior-matrix-note.md`
- Remaining risks: `26` operations still remain `pending_baseline_capture`; broader endpoint-level parity across the rest of the route set is still incomplete; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the repo now records the already-proven balloon hinting status read surface in the generated behavior matrix, but GH `#2` remains in `Verification` because much broader endpoint-level status/error parity still needs capture and, in some areas, implementation work

### 2026-04-06 — WOR-20 — Success-body behavior matrix unlocked
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: extend the behavior-matrix harness to encode successful `200` response-body routes and seed identity/version captures
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the route-level behavior matrix now supports field-level JSON assertions for success bodies and captures `GET /` plus `GET /version`, but GH `#2` still needs broader route coverage and more endpoint-level capture work
- Harness update: added `success_body` outcome support plus exact/type JSON body assertions in `src/api_behavior_matrix.rs`, seeded `GET /` and `GET /version` in `docs/specs/firecracker-api-behavior-captures.yaml`, regenerated `docs/generated/firecracker-api-behavior-matrix.json` to `10` captured routes and `30` total cases, and recorded the slice note in `run/WOR-20/api/step48-success-body-harness-and-identity-version-note.md`
- Remaining risks: `27` routes still remain `pending_baseline_capture`; some remaining high-value routes may still need richer matchers for larger nested bodies or cross-field relations; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the repo is no longer blocked on a no-content-only behavior matrix, but GH `#2` remains in `Verification` because much broader endpoint-level status/error parity still needs capture and, in some areas, implementation work

### 2026-04-06 — WOR-20 — Balloon hinting control behavior matrix seeded
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: extend the generated behavior matrix with pinned `PATCH /balloon/hinting/start` and `PATCH /balloon/hinting/stop` runtime cases
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the route-level behavior matrix now captures `PUT /actions`, `PATCH /vm`, `PATCH /drives/{drive_id}`, `PATCH /network-interfaces/{iface_id}`, `PATCH /balloon`, `PATCH /balloon/statistics`, `PATCH /balloon/hinting/start`, and `PATCH /balloon/hinting/stop`, but GH `#2` still needs much broader route capture and implementation work
- Harness update: added hinting-control cases to `docs/specs/firecracker-api-behavior-captures.yaml`, regenerated `docs/generated/firecracker-api-behavior-matrix.json` to `8` captured routes and `28` total cases, and recorded the slice note in `run/WOR-20/api/step47-balloon-hinting-behavior-matrix-note.md`
- Remaining risks: the behavior matrix still captures only eight route families so far; many remaining high-value gaps are `GET` routes with `200` response bodies that the current matrix schema cannot yet describe well; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the repo now has a much broader route-level baseline behavior matrix for implemented handlers, but GH `#2` remains in `Verification` because broader endpoint-level status/error parity and the next harness upgrade are still incomplete

### 2026-04-06 — WOR-20 — Balloon runtime behavior matrix seeded
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: extend the generated behavior matrix with pinned `PATCH /balloon` runtime cases
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the route-level behavior matrix now captures `PUT /actions`, `PATCH /vm`, `PATCH /drives/{drive_id}`, `PATCH /network-interfaces/{iface_id}`, and `PATCH /balloon`, but GH `#2` still needs much broader route capture and implementation work
- Harness update: added `PATCH /balloon` cases to `docs/specs/firecracker-api-behavior-captures.yaml`, regenerated `docs/generated/firecracker-api-behavior-matrix.json` to `5` captured routes and `19` total cases, and recorded the slice note in `run/WOR-20/api/step45-balloon-behavior-matrix-note.md`
- Remaining risks: the behavior matrix still captures only five route families so far; most concrete-handler routes remain `pending_baseline_capture`; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the repo now has a growing route-level baseline behavior matrix for implemented handlers, but GH `#2` remains in `Verification` because broader endpoint-level status/error parity is still incomplete

### 2026-04-06 — WOR-20 — Network patch behavior matrix seeded
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: extend the generated behavior matrix with pinned `PATCH /network-interfaces/{iface_id}` runtime cases
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the route-level behavior matrix now captures `PUT /actions`, `PATCH /vm`, `PATCH /drives/{drive_id}`, and `PATCH /network-interfaces/{iface_id}`, but GH `#2` still needs much broader route capture and implementation work
- Harness update: added `PATCH /network-interfaces/{iface_id}` cases to `docs/specs/firecracker-api-behavior-captures.yaml`, regenerated `docs/generated/firecracker-api-behavior-matrix.json` to `4` captured routes and `16` total cases, and recorded the slice note in `run/WOR-20/api/step44-network-behavior-matrix-note.md`
- Remaining risks: the behavior matrix still captures only four route families so far; most concrete-handler routes remain `pending_baseline_capture`; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the repo now has a growing route-level baseline behavior matrix for implemented handlers, but GH `#2` remains in `Verification` because broader endpoint-level status/error parity is still incomplete

### 2026-04-06 — WOR-20 — Drive patch behavior matrix seeded
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: extend the generated behavior matrix with pinned `PATCH /drives/{drive_id}` runtime cases
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the route-level behavior matrix now captures `PUT /actions`, `PATCH /vm`, and `PATCH /drives/{drive_id}`, but GH `#2` still needs much broader route capture and implementation work
- Harness update: added `PATCH /drives/{drive_id}` cases to `docs/specs/firecracker-api-behavior-captures.yaml`, added a full-router runtime path/body mismatch test in `tests/api_contract.rs`, regenerated `docs/generated/firecracker-api-behavior-matrix.json` to `3` captured routes and `13` total cases, and recorded the slice note in `run/WOR-20/api/step43-drive-behavior-matrix-note.md`
- Remaining risks: the behavior matrix still captures only three route families so far; most concrete-handler routes remain `pending_baseline_capture`; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the repo now has a growing route-level baseline behavior matrix for implemented handlers, but GH `#2` remains in `Verification` because broader endpoint-level status/error parity is still incomplete

### 2026-04-06 — WOR-20 — VM state behavior matrix seeded
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: extend the generated behavior matrix with pinned `PATCH /vm` pause/resume cases
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the route-level behavior matrix now captures both `PUT /actions` and `PATCH /vm`, but GH `#2` still needs much broader route capture and implementation work
- Harness update: added `PATCH /vm` cases to `docs/specs/firecracker-api-behavior-captures.yaml`, regenerated `docs/generated/firecracker-api-behavior-matrix.json` to `2` captured routes and `10` total cases, and recorded the slice note in `run/WOR-20/api/step42-vm-behavior-matrix-note.md`
- Remaining risks: the behavior matrix still captures only two route families so far; most concrete-handler routes remain `pending_baseline_capture`; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the repo now has a growing route-level baseline behavior matrix for implemented handlers, but GH `#2` remains in `Verification` because broader endpoint-level status/error parity is still incomplete

### 2026-04-06 — WOR-20 — Actions behavior matrix seeded
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: seed a generated behavior matrix for concrete-handler routes with pinned `/actions` cases
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the repo now has a generated route-level behavior matrix seeded with seven pinned `/actions` cases, but GH `#2` still needs much broader route capture and implementation work
- Harness update: added `src/api_behavior_matrix.rs`, taught `baselinectl` and `Makefile` to generate and validate `docs/generated/firecracker-api-behavior-matrix.json` against `docs/specs/firecracker-api-behavior-captures.yaml`, and recorded the slice note in `run/WOR-20/api/step41-actions-behavior-matrix-note.md`
- Remaining risks: only `PUT /actions` is captured in the new behavior matrix so far; most concrete-handler routes still remain `pending_baseline_capture`; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the repo now has the first checked-in route-level baseline behavior matrix for implemented handlers, but GH `#2` remains in `Verification` because broader endpoint-level status/error parity is still incomplete

### 2026-04-06 — WOR-20 — Surface matrix applicability restored
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: make the generated surface-stub matrix mark concrete-handler routes as `not_applicable` instead of leaving them in blanket pending capture state
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the repo now truthfully records that only the two snapshot routes still use the matrix-backed placeholder path, but GH `#2` still needs broader endpoint-level behavior capture and implementation work
- Harness update: updated `src/api_matrix.rs` so only `PUT /snapshot/create` and `PUT /snapshot/load` participate in matrix-backed capture states, regenerated `docs/generated/firecracker-api-surface-stub-matrix.json`, and recorded the dispatch-basis note in `run/WOR-20/api/step40-matrix-applicability-note.md`
- Remaining risks: the surface-stub matrix is still not the full Firecracker status/error matrix requested by the issue; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the generated surface-stub matrix now marks `35` routes as `not_applicable` and the two snapshot placeholder routes as repo-backed captures, but GH `#2` remains in `Verification` because broader endpoint parity is still incomplete

### 2026-04-06 — WOR-20 — Surface matrix capture registry added
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: add a checked-in capture registry so the generated surface-stub matrix distinguishes repo-backed placeholder-route captures from blanket pending state
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the repo now truthfully records snapshot placeholder routes with pinned evidence in the generated matrix, but GH `#2` still needs broader endpoint-level behavior capture and implementation work
- Harness update: added `docs/specs/firecracker-api-baseline-captures.yaml`, taught `src/api_matrix.rs` and `baselinectl` to generate and validate the matrix against that registry, regenerated `docs/generated/firecracker-api-surface-stub-matrix.json`, and recorded the harness note in `run/WOR-20/api/step39-capture-registry-note.md`
- Remaining risks: the surface-stub matrix still only covers matrix-backed placeholder paths; most routes remain `pending_baseline_capture`; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the generated surface-stub matrix now distinguishes the two snapshot placeholder routes with repo-backed baseline evidence from the wider pending set, but GH `#2` remains in `Verification` because the full Firecracker status/error matrix and broader endpoint parity are still incomplete

### 2026-04-06 — WOR-20 — Snapshot-load network-interface guard proof captured
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: add upstream-backed full-router proof that network-interface configuration blocks `PUT /snapshot/load`
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the repo now has durable full-router proof for this low-risk snapshot-load network-interface guard, but broader endpoint-level parity work remains open
- Harness update: recorded the pinned snapshot-load network-interface guard facts in `run/WOR-20/api/step38-upstream-snapshot-load-network-interface-baseline.md` and added a full-router contract test proving that successful network-interface configuration makes `PUT /snapshot/load` return the invalid-state envelope
- Remaining risks: the wider route matrix still contains `pending_baseline_capture` placeholders or placeholder handlers; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; the parity doc still mixes pinned schema shape with observed baseline behavior in places; the broader snapshot boot-path matrix and broader snapshot create/load runtime behavior still need stronger upstream capture before more route changes would be safe; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the repo now has durable upstream-backed proof that network-interface configuration blocks `PUT /snapshot/load`, but GH `#2` still needs more endpoint-level parity work before the full OpenAPI surface stub can be considered complete

### 2026-04-06 — WOR-20 — Snapshot-load balloon guard proof captured
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: add upstream-backed full-router proof that balloon configuration blocks `PUT /snapshot/load`
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the repo now has durable full-router proof for this low-risk snapshot-load balloon guard, but broader endpoint-level parity work remains open
- Harness update: recorded the pinned snapshot-load balloon guard facts in `run/WOR-20/api/step37-upstream-snapshot-load-balloon-baseline.md` and added a full-router contract test proving that successful balloon configuration makes `PUT /snapshot/load` return the invalid-state envelope
- Remaining risks: the wider route matrix still contains `pending_baseline_capture` placeholders or placeholder handlers; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; the parity doc still mixes pinned schema shape with observed baseline behavior in places; the broader snapshot boot-path matrix and broader snapshot create/load runtime behavior still need stronger upstream capture before more route changes would be safe; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the repo now has durable upstream-backed proof that balloon configuration blocks `PUT /snapshot/load`, but GH `#2` still needs more endpoint-level parity work before the full OpenAPI surface stub can be considered complete

### 2026-04-06 — WOR-20 — Snapshot-load memory-hotplug guard proof captured
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: add upstream-backed full-router proof that memory-hotplug configuration blocks `PUT /snapshot/load`
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the repo now has durable full-router proof for this low-risk snapshot-load memory-hotplug guard, but broader endpoint-level parity work remains open
- Harness update: recorded the pinned snapshot-load memory-hotplug guard facts in `run/WOR-20/api/step36-upstream-snapshot-load-memory-hotplug-baseline.md` and added a full-router contract test proving that successful memory-hotplug configuration makes `PUT /snapshot/load` return the invalid-state envelope
- Remaining risks: the wider route matrix still contains `pending_baseline_capture` placeholders or placeholder handlers; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; the parity doc still mixes pinned schema shape with observed baseline behavior in places; the broader snapshot boot-path matrix and broader snapshot create/load runtime behavior still need stronger upstream capture before more route changes would be safe; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the repo now has durable upstream-backed proof that memory-hotplug configuration blocks `PUT /snapshot/load`, but GH `#2` still needs more endpoint-level parity work before the full OpenAPI surface stub can be considered complete

### 2026-04-06 — WOR-20 — Snapshot-load boot-source guard proof captured
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: add upstream-backed full-router proof that boot-source configuration blocks `PUT /snapshot/load`
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the repo now has durable full-router proof for this low-risk snapshot-load boot-source guard, but broader endpoint-level parity work remains open
- Harness update: recorded the pinned snapshot-load boot-source guard facts in `run/WOR-20/api/step35-upstream-snapshot-load-boot-source-baseline.md` and added a full-router contract test proving that successful boot-source configuration makes `PUT /snapshot/load` return the invalid-state envelope
- Remaining risks: the wider route matrix still contains `pending_baseline_capture` placeholders or placeholder handlers; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; the parity doc still mixes pinned schema shape with observed baseline behavior in places; the broader snapshot boot-path matrix and broader snapshot create/load runtime behavior still need stronger upstream capture before more route changes would be safe; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the repo now has durable upstream-backed proof that boot-source configuration blocks `PUT /snapshot/load`, but GH `#2` still needs more endpoint-level parity work before the full OpenAPI surface stub can be considered complete

### 2026-04-06 — WOR-20 — Snapshot-create paused-state proof captured
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: add upstream-backed full-router proof that `PUT /snapshot/create` requires a paused VM and still reaches the placeholder path once paused
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the repo now has durable full-router proof for this low-risk snapshot-create precondition, but broader endpoint-level parity work remains open
- Harness update: recorded the pinned snapshot-create postboot and paused-state facts in `run/WOR-20/api/step34-upstream-snapshot-create-paused-baseline.md` and added a full-router contract test proving that a running VM is rejected while a paused VM reaches the existing placeholder path
- Remaining risks: the wider route matrix still contains `pending_baseline_capture` placeholders or placeholder handlers; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; the parity doc still mixes pinned schema shape with observed baseline behavior in places; the broader snapshot boot-path matrix and broader snapshot create/load runtime behavior still need stronger upstream capture before more route changes would be safe; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the repo now has durable upstream-backed proof that `PUT /snapshot/create` is only valid once the VM is booted and paused, but GH `#2` still needs more endpoint-level parity work before the full OpenAPI surface stub can be considered complete

### 2026-04-06 — WOR-20 — Snapshot-load non-boot-config reachability proof captured
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: add upstream-backed full-router proof that logger, metrics, and serial configuration do not block `PUT /snapshot/load`
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the repo now has durable full-router proof for this low-risk non-boot-config snapshot-load reachability fact, but broader endpoint-level parity work remains open
- Harness update: recorded the pinned logger/metrics/serial snapshot-load boot-path facts in `run/WOR-20/api/step33-upstream-snapshot-load-nonboot-config-baseline.md` and added a full-router contract test proving that, after those non-boot configurations, `PUT /snapshot/load` still reaches the existing placeholder path
- Remaining risks: the wider route matrix still contains `pending_baseline_capture` placeholders or placeholder handlers; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; the parity doc still mixes pinned schema shape with observed baseline behavior in places; the broader snapshot boot-path matrix and broader snapshot create/load runtime behavior still need stronger upstream capture before more route changes would be safe; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the repo now has durable upstream-backed proof that logger, metrics, and serial configuration do not make `PUT /snapshot/load` preboot-invalid, but GH `#2` still needs more endpoint-level parity work before the full OpenAPI surface stub can be considered complete

### 2026-04-06 — WOR-20 — Snapshot-load serial reachability slice aligned to pinned baseline
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: allow `PUT /snapshot/load` after serial configuration and add full-router proof
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the repo now matches the pinned low-risk serial/snapshot-load reachability fact, but broader endpoint-level parity work remains open
- Harness update: recorded the pinned serial/load-snapshot boot-path facts in `run/WOR-20/api/step32-upstream-snapshot-load-serial-baseline.md`, removed the overbroad serial check from the local snapshot-load guard, and added contract coverage proving that the serial-configured route now reaches the placeholder snapshot path
- Remaining risks: the wider route matrix still contains `pending_baseline_capture` placeholders or placeholder handlers; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; the parity doc still mixes pinned schema shape with observed baseline behavior in places; the remaining open surface is increasingly runtime-sensitive, especially around broader snapshot guard/runtime behavior, the remaining snapshot boot-path matrix, and other endpoint families that still need more endpoint-level proof or concrete behavior work; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the repo no longer overblocks `PUT /snapshot/load` after serial configuration and now has durable full-router proof for that low-risk reachability fact, but GH `#2` still needs more endpoint-level parity work before the full OpenAPI surface stub can be considered complete

### 2026-04-06 — WOR-20 — VM pause/resume controller slice aligned to pinned baseline
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: align preboot `PATCH /vm` invalid-state behavior and add full-router pause/resume proof
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the repo now matches the pinned preboot `PATCH /vm` invalid-state message and has durable full-router proof for runtime pause/resume transitions, but broader endpoint-level parity work remains open
- Harness update: recorded the pinned `PATCH /vm` parser/controller facts in `run/WOR-20/api/step31-upstream-vm-state-baseline.md`, routed `PATCH /vm` through its dedicated handler before generic state-guard rejection, and added contract coverage proving preboot rejection plus runtime pause/resume state transitions
- Remaining risks: the wider route matrix still contains `pending_baseline_capture` placeholders or placeholder handlers; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; the parity doc still mixes pinned schema shape with observed baseline behavior in places; the remaining open surface is increasingly runtime-sensitive, especially around broader snapshot guard/runtime behavior, remaining VM lifecycle edge cases, and other endpoint families that still need more endpoint-level proof or concrete behavior work; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the repo now matches the pinned preboot `PATCH /vm` invalid-state surface and has durable full-router proof for runtime pause/resume transitions, but GH `#2` still needs more endpoint-level parity work before the full OpenAPI surface stub can be considered complete

### 2026-04-06 — WOR-20 — Snapshot-load parser fault slice aligned to pinned baseline
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: align `PUT /snapshot/load` memory-source parser faults to the pinned Firecracker deserialization surface
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the repo now matches the pinned snapshot-load parser fault strings for the covered missing and duplicate memory-source cases, but broader endpoint-level parity work remains open
- Harness update: recorded the pinned snapshot-load parser facts in `run/WOR-20/api/step30-upstream-snapshot-load-parser-baseline.md`, aligned the local missing and duplicate memory-source fault strings in `src/api.rs`, and added unit plus full-router contract coverage for both parser cases
- Remaining risks: the wider route matrix still contains `pending_baseline_capture` placeholders or placeholder handlers; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; the parity doc still mixes pinned schema shape with observed baseline behavior in places; the remaining open surface is increasingly runtime-sensitive, especially around broader snapshot guard/runtime behavior and other endpoint families that still need more endpoint-level proof or concrete behavior work; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the repo now matches the pinned snapshot-load deserialization fault surface for the covered memory-source parser cases, but GH `#2` still needs more endpoint-level parity work before the full OpenAPI surface stub can be considered complete

### 2026-04-06 — WOR-20 — Actions guard-surface proof slice anchored to pinned baseline
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: add upstream-backed full-router proof for `PUT /actions` state guards and fault surfaces
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the repo now has durable full-router proof for the existing `/actions` guard surface, but broader endpoint-level parity work remains open
- Harness update: recorded the pinned upstream `/actions` request/controller/error-display facts in `run/WOR-20/api/step29-upstream-actions-baseline.md` and added contract coverage proving missing-kernel `InstanceStart` rejection, preboot and postboot `FlushMetrics` behavior, postboot start rejection, and the architecture-specific `SendCtrlAltDel` guards
- Remaining risks: the wider route matrix still contains `pending_baseline_capture` placeholders or placeholder handlers; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; the parity doc still mixes pinned schema shape with observed baseline behavior in places; the remaining open surface is increasingly runtime-sensitive, especially around snapshots and other endpoint families that still need more endpoint-level proof or concrete behavior work; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the repo now has durable baseline-backed proof for the existing `/actions` guard surface on `InstanceStart`, `FlushMetrics`, and `SendCtrlAltDel`, but GH `#2` still needs more endpoint-level parity work before the full OpenAPI surface stub can be considered complete

### 2026-04-06 — WOR-20 — Identity/version preboot proof slice anchored to pinned baseline
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: add upstream-backed full-router proof for `GET /` and `GET /version`
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the repo now has durable full-router proof for the read-only preboot identity/version surface, but broader endpoint-level parity work remains open
- Harness update: recorded the pinned upstream identity/version request and response-conversion facts in `run/WOR-20/api/step28-upstream-identity-version-baseline.md` and added contract coverage proving the preboot `GET /` state string plus the `GET /version` field shape and version alignment
- Remaining risks: the wider route matrix still contains `pending_baseline_capture` placeholders or placeholder handlers; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; the parity doc still mixes pinned schema shape with observed baseline behavior in places; the remaining open surface is increasingly runtime-sensitive, especially around actions, snapshots, and other endpoint families that still need more endpoint-level proof or concrete behavior work; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the repo now has durable baseline-backed proof for the read-only preboot identity/version surface, but GH `#2` still needs more endpoint-level parity work before the full OpenAPI surface stub can be considered complete

### 2026-04-06 — WOR-20 — Logger accepted-state/omission proof slice anchored to pinned baseline
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: add upstream-backed full-router proof for the existing `PUT /logger` accepted-state behavior and `/vm/config` omission
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the repo now has durable full-router proof for the existing low-risk `/logger` accepted-state/omit path, but broader endpoint-level parity work remains open
- Harness update: recorded the pinned upstream logger request/export facts in `run/WOR-20/api/step27-upstream-logger-store-baseline.md` and added contract coverage proving that successful `PUT /logger` leaves logger configuration omitted from `/vm/config`
- Remaining risks: the wider route matrix still contains `pending_baseline_capture` placeholders or placeholder handlers; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; the parity doc still mixes pinned schema shape with observed baseline behavior in places; broader `/logger` host-path parity plus many other endpoint families still need more endpoint-level proof or concrete behavior work; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the repo now has durable baseline-backed proof for the existing `/logger` accepted-state/export path, but GH `#2` still needs more endpoint-level parity work before the full OpenAPI surface stub can be considered complete

### 2026-04-06 — WOR-20 — Metrics store/omission proof slice anchored to pinned baseline
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: add upstream-backed full-router proof for the existing `PUT /metrics` store behavior and `/vm/config` omission
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the repo now has durable full-router proof for the existing low-risk `/metrics` store/omit path, but broader endpoint-level parity work remains open
- Harness update: recorded the pinned upstream metrics request/export facts in `run/WOR-20/api/step26-upstream-metrics-store-baseline.md` and added contract coverage proving that successful preboot `PUT /metrics` leaves metrics configuration omitted from `/vm/config`
- Remaining risks: the wider route matrix still contains `pending_baseline_capture` placeholders or placeholder handlers; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; the parity doc still mixes pinned schema shape with observed baseline behavior in places; broader `/logger`, `/metrics`, `/serial`, and many other endpoint families still need more endpoint-level proof or concrete behavior work; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the repo now has durable baseline-backed proof for the existing `/metrics` store/export path, but GH `#2` still needs more endpoint-level parity work before the full OpenAPI surface stub can be considered complete

### 2026-04-06 — WOR-20 — Serial store/omission proof slice anchored to pinned baseline
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: add upstream-backed full-router proof for the existing `PUT /serial` store behavior and `/vm/config` omission
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the repo now has durable full-router proof for the existing low-risk `/serial` store/omit path, but broader endpoint-level parity work remains open
- Harness update: recorded the pinned upstream serial request/export facts in `run/WOR-20/api/step25-upstream-serial-store-baseline.md` and added contract coverage proving that successful preboot `PUT /serial` leaves serial configuration omitted from `/vm/config`
- Remaining risks: the wider route matrix still contains `pending_baseline_capture` placeholders or placeholder handlers; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; the parity doc still mixes pinned schema shape with observed baseline behavior in places; broader `/serial`, `/logger`, `/metrics`, and many other endpoint families still need more endpoint-level proof or concrete behavior work; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the repo now has durable baseline-backed proof for the existing `/serial` store/export path, but GH `#2` still needs more endpoint-level parity work before the full OpenAPI surface stub can be considered complete

### 2026-04-06 — WOR-20 — CPU-config export parity slice aligned to pinned baseline
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: fix preboot `PUT /cpu-config` parity so `/vm/config` omits `cpu-config`, matching pinned Firecracker export behavior
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the repo no longer overexports `cpu-config` in `/vm/config` after successful preboot `PUT /cpu-config`, but broader endpoint-level parity work remains open
- Harness update: recorded the pinned upstream CPU-config export facts in `run/WOR-20/api/step24-upstream-cpu-config-export-baseline.md`, removed the repo-local `cpu-config` export from `src/api.rs`, added unit plus contract coverage proving `/vm/config` omission after successful preboot `PUT /cpu-config`, and added a parity-doc caveat noting that schema and observed baseline behavior can diverge
- Remaining risks: the wider route matrix still contains `pending_baseline_capture` placeholders or placeholder handlers; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; the parity doc now carries a caveat, but the detailed field table still mirrors schema shape rather than observed baseline behavior for `cpu-config` under `/vm/config`; broader `/cpu-config` parity and many other endpoint families still need more endpoint-level proof or concrete behavior work; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the control-plane stub no longer exports `cpu-config` through `/vm/config` after preboot `PUT /cpu-config`, matching the pinned Firecracker full-config path, but GH `#2` still needs more endpoint-level parity work before the full OpenAPI surface stub can be considered complete

### 2026-04-06 — WOR-20 — Machine-config store/export proof slice anchored to pinned baseline
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: add upstream-backed full-router proof for `GET /machine-config`, preboot `PUT/PATCH /machine-config`, and `/vm/config` export behavior
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the repo now has durable full-router proof for the existing low-risk machine-config store/update/export path, but broader endpoint-level parity work remains open
- Harness update: added contract coverage for successful `GET /machine-config` plus preboot `PUT/PATCH /machine-config`, recorded the pinned upstream request/config facts in `run/WOR-20/api/step23-upstream-machine-config-store-baseline.md`, and proved that `/vm/config` preserves the stored machine configuration
- Remaining risks: the wider route matrix still contains `pending_baseline_capture` placeholders or placeholder handlers; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; broader `/machine-config` parity and many other endpoint families still need more endpoint-level proof or concrete behavior work; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the repo now has durable baseline-backed proof for the existing `/machine-config` store/update/export path, but GH `#2` still needs more endpoint-level parity work before the full OpenAPI surface stub can be considered complete

### 2026-04-06 — WOR-20 — Boot-source store/export proof slice anchored to pinned baseline
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: add upstream-backed full-router proof for the existing preboot `/boot-source` store/export behavior, including export in `/vm/config`
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the repo now has durable full-router proof for the existing low-risk `/boot-source` store/export path, but broader endpoint-level parity work remains open
- Harness update: added contract coverage for successful preboot `PUT /boot-source`, recorded the pinned upstream request/config facts in `run/WOR-20/api/step22-upstream-boot-source-store-baseline.md`, and proved that `/vm/config` preserves the stored boot source fields
- Remaining risks: the wider route matrix still contains `pending_baseline_capture` placeholders or placeholder handlers; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; broader `/boot-source` parity and many other endpoint families still need more endpoint-level proof or concrete behavior work; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the repo now has durable baseline-backed proof for the existing `/boot-source` store/export path, but GH `#2` still needs more endpoint-level parity work before the full OpenAPI surface stub can be considered complete

### 2026-04-06 — WOR-20 — Entropy store/export proof slice anchored to pinned baseline
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: add upstream-backed full-router proof for the existing preboot `/entropy` store/export behavior, including rate-limiter export in `/vm/config`
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the repo now has durable full-router proof for the existing low-risk `/entropy` store/export path, but broader endpoint-level parity work remains open
- Harness update: added contract coverage for successful preboot `PUT /entropy`, recorded the pinned upstream request/config facts in `run/WOR-20/api/step21-upstream-entropy-store-baseline.md`, and proved that `/vm/config` preserves the stored entropy rate-limiter shape
- Remaining risks: the wider route matrix still contains `pending_baseline_capture` placeholders or placeholder handlers; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; broader `/entropy` parity and many other endpoint families still need more endpoint-level proof or concrete behavior work; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the repo now has durable baseline-backed proof for the existing `/entropy` store/export path, but GH `#2` still needs more endpoint-level parity work before the full OpenAPI surface stub can be considered complete

### 2026-04-06 — WOR-20 — Vsock store/export proof slice anchored to pinned baseline
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: add upstream-backed full-router proof for the existing preboot `/vsock` store/export behavior, including deprecated-field omission in `/vm/config`
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the repo now has durable full-router proof for the existing low-risk `/vsock` store/export path, but broader endpoint-level parity work remains open
- Harness update: added contract coverage for successful preboot `PUT /vsock`, recorded the pinned upstream request/config facts in `run/WOR-20/api/step20-upstream-vsock-store-baseline.md`, and proved that `/vm/config` omits deprecated `vsock_id`
- Remaining risks: the wider route matrix still contains `pending_baseline_capture` placeholders or placeholder handlers; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; broader `/vsock` parity and many other endpoint families still need more endpoint-level proof or concrete behavior work; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the repo now has durable baseline-backed proof for the existing `/vsock` store/export path, but GH `#2` still needs more endpoint-level parity work before the full OpenAPI surface stub can be considered complete

### 2026-04-06 — WOR-20 — Drive root-first export slice aligned to pinned baseline
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: implement truthful `/vm/config` drive export ordering so the root block device is serialized first, matching the pinned Firecracker drive-list invariant
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the repo now matches the pinned root-first drive export invariant for the low-risk drive ordering path, but broader endpoint-level parity work remains open
- Harness update: reordered exported drives in `src/api.rs` so the root block device is serialized first, extended API plus contract coverage for the root-first export path, and recorded the pinned drive ordering evidence in `run/WOR-20/api/step19-upstream-drive-root-order-baseline.md`
- Remaining risks: the wider route matrix still contains `pending_baseline_capture` placeholders or placeholder handlers; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; fuller drive create backing-file validation, host-path failure, remaining non-root ordering, and io-engine parity are still open; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the drive family no longer exports non-root drives ahead of the root block device in `/vm/config`, but GH `#2` still needs more endpoint-level parity work before the full OpenAPI surface stub can be considered complete

### 2026-04-06 — WOR-20 — Drive cache default export slice aligned to pinned baseline
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: implement truthful preboot `PUT /drives/{drive_id}` stored-config defaulting so omitted `cache_type` exports as the pinned `Unsafe` default in `/vm/config`
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the repo now matches the pinned stored-config default shape for the low-risk drive cache-mode path, but broader endpoint-level parity work remains open
- Harness update: defaulted omitted drive `cache_type` to `Unsafe` in `src/api.rs`, extended API plus contract coverage for the normalized export path, and recorded the pinned drive config evidence in `run/WOR-20/api/step18-upstream-drive-cache-default-baseline.md`
- Remaining risks: the wider route matrix still contains `pending_baseline_capture` placeholders or placeholder handlers; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; fuller drive create backing-file validation, host-path failure, root ordering, and io-engine parity are still open; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the drive family no longer omits the pinned default cache mode from stored config export when callers leave it unspecified, but GH `#2` still needs more endpoint-level parity work before the full OpenAPI surface stub can be considered complete

### 2026-04-06 — WOR-20 — Drive create root-device conflict slice aligned to pinned baseline
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: implement truthful preboot `PUT /drives/{drive_id}` root-device conflict behavior, including pinned pmem-root and duplicate-block-root error envelopes while preserving same-id replacement
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the repo now matches the pinned controller behavior for the low-risk drive create root-conflict path, but broader endpoint-level parity work remains open
- Harness update: added upstream-backed root-device conflict handling in `src/api.rs`, extended API plus contract coverage for pmem-root conflict, duplicate-root conflict, and allowed same-id replacement, and recorded the pinned drive parser/controller evidence in `run/WOR-20/api/step17-upstream-drive-create-baseline.md`
- Remaining risks: the wider route matrix still contains `pending_baseline_capture` placeholders or placeholder handlers; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; fuller drive create backing-file validation and host-path failure parity is still open; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the drive family no longer accepts low-risk root-device conflicts that pinned Firecracker rejects, and same-id replacement remains truthful, but GH `#2` still needs more endpoint-level parity work before the full OpenAPI surface stub can be considered complete

### 2026-04-06 — WOR-20 — Pmem create preboot slice aligned to pinned baseline
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: implement truthful pmem id-mismatch handling and preboot `PUT /pmem/{id}` root-device conflict behavior, including pinned pmem error envelopes and defaulted config export booleans
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the repo now matches the pinned parser/controller behavior for the low-risk pmem create path, but broader endpoint-level parity work remains open
- Harness update: aligned pmem PUT id-mismatch handling in `src/api.rs` to the exact upstream parser string, enforced upstream-backed block-root and duplicate-pmem-root conflicts, normalized exported pmem defaults, and recorded the pinned pmem parser/controller evidence in `run/WOR-20/api/step16-upstream-pmem-create-baseline.md`
- Remaining risks: the wider route matrix still contains `pending_baseline_capture` placeholders or placeholder handlers; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; fuller pmem backing-file validation and ordering parity is still open; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the pmem family no longer uses repo-local id-mismatch wording, no longer misses the low-risk root-device conflict matrix, and now exports default boolean fields truthfully, but GH `#2` still needs more endpoint-level parity work before the full OpenAPI surface stub can be considered complete

### 2026-04-06 — WOR-20 — Network update runtime slice aligned to pinned baseline
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: implement truthful network-interface id-mismatch handling and postboot `PATCH /network-interfaces/{iface_id}` behavior, including the pinned missing-interface update envelope
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the repo now matches the pinned parser/controller behavior for the low-risk network update path, but broader endpoint-level parity work remains open
- Harness update: aligned network-interface PUT/PATCH id-mismatch handling in `src/api.rs` to the exact upstream parser format, extended API plus contract coverage for runtime missing-interface errors and successful postboot rate-limiter updates, and recorded the pinned network parser/controller evidence in `run/WOR-20/api/step15-upstream-network-update-baseline.md`
- Remaining risks: the wider route matrix still contains `pending_baseline_capture` placeholders or placeholder handlers; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; fuller network-create MAC-conflict and tap-open parity is still open; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the network-interface family no longer uses repo-local id-mismatch wording or a generic missing-interface update error for the low-risk runtime patch path, but GH `#2` still needs more endpoint-level parity work before the full OpenAPI surface stub can be considered complete

### 2026-04-06 — WOR-20 — Drive update runtime slice aligned to pinned baseline
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: implement truthful drive id-mismatch handling and postboot `PATCH /drives/{drive_id}` behavior, including the pinned missing-drive update envelope
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the repo now matches the pinned parser/controller behavior for the low-risk drive update path, but broader endpoint-level parity work remains open
- Harness update: aligned drive PUT/PATCH id-mismatch handling in `src/api.rs` to the exact upstream parser string, extended API plus contract coverage for runtime missing-drive errors and successful postboot path updates, and recorded the pinned drive parser/controller evidence in `run/WOR-20/api/step14-upstream-drive-update-baseline.md`
- Remaining risks: the wider route matrix still contains `pending_baseline_capture` placeholders or placeholder handlers; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; fuller drive-create/root-device conflict parity is still open; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the drive family no longer uses repo-local id-mismatch wording or a generic missing-drive update error for the low-risk runtime patch path, but GH `#2` still needs more endpoint-level parity work before the full OpenAPI surface stub can be considered complete

### 2026-04-06 — WOR-20 — Balloon hinting control runtime slice aligned to pinned baseline
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: implement truthful postboot `PATCH /balloon/hinting/start` and `PATCH /balloon/hinting/stop` behavior, including pinned missing-device and hinting-disabled fault strings plus the empty-body/defaulted start flow
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the repo now matches the pinned runtime controller behavior for the low-risk balloon hinting control path, but broader endpoint-level parity work remains open
- Harness update: added concrete balloon hinting start/stop handling in `src/api.rs`, extended API plus contract coverage for missing-device, hinting-disabled, empty-body start, and stop-with-ignored-body cases, and recorded the pinned parser/controller evidence in `run/WOR-20/api/step13-upstream-balloon-hinting-control-baseline.md`
- Remaining risks: the wider route matrix still contains `pending_baseline_capture` placeholders or placeholder handlers; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; the current control-path model is intentionally conservative and does not yet claim full guest-side hinting completion parity; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: balloon hinting control no longer falls through a generic runtime stub for the low-risk start/stop path after boot, but GH `#2` still needs more endpoint-level parity work before the full OpenAPI surface stub can be considered complete

### 2026-04-06 — WOR-20 — Balloon hinting-status runtime slice aligned to pinned baseline
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: implement truthful postboot `GET /balloon/hinting/status` behavior, including pinned missing-device and hinting-disabled fault strings plus the idle status shape
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the repo now matches the pinned runtime controller behavior for the low-risk balloon hinting-status path, but hinting start/stop and many other routes remain open
- Harness update: added concrete balloon hinting-status handling in `src/api.rs`, extended API plus contract coverage for missing-device, hinting-disabled, and idle enabled-hinting reads, and recorded the pinned controller evidence in `run/WOR-20/api/step12-upstream-balloon-hinting-status-baseline.md`
- Remaining risks: the wider route matrix still contains `pending_baseline_capture` placeholders or placeholder handlers; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; hinting start/stop sequencing is still not implemented; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the hinting-status read route no longer relies on a generic stub for the low-risk runtime idle path after boot, but GH `#2` still needs more endpoint-level parity work before the full OpenAPI surface stub can be considered complete

### 2026-04-06 — WOR-20 — Balloon statistics runtime slice aligned to pinned baseline
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: implement truthful postboot `GET /balloon/statistics` and `PATCH /balloon/statistics` behavior, including pinned missing-device, disabled-stats, and stats-state fault strings plus required response fields
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the repo now matches the pinned runtime controller behavior for the low-risk balloon statistics path, but balloon hinting and many other routes remain open
- Harness update: added concrete balloon statistics handlers in `src/api.rs`, extended API plus contract coverage for missing-device, disabled-stats, successful nonzero interval updates, and post-boot state-change rejection, and recorded the pinned controller evidence in `run/WOR-20/api/step11-upstream-balloon-stats-baseline.md`
- Remaining risks: the wider route matrix still contains `pending_baseline_capture` placeholders or placeholder handlers; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; the current stats response is a deterministic stub for required fields rather than full guest telemetry; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: balloon statistics routes no longer rely on generic route guards or placeholder handling for the low-risk runtime read/update path after boot, but GH `#2` still needs more endpoint-level parity work before the full OpenAPI surface stub can be considered complete

### 2026-04-06 — WOR-20 — Balloon runtime read/update slice aligned to pinned baseline
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: implement truthful postboot `GET /balloon` missing-device behavior and `PATCH /balloon` target updates, including the pinned missing-device and oversize fault strings
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the repo now matches the pinned runtime read/update behavior for the low-risk balloon target-size path, but balloon statistics and hinting remain open
- Harness update: added concrete postboot `PATCH /balloon` behavior in `src/api.rs`, aligned postboot missing-device `GET /balloon` reads with the upstream runtime error path, and extended API plus contract coverage for runtime missing-device, successful update, and oversize rejection cases
- Remaining risks: the wider route matrix still contains `pending_baseline_capture` placeholders or placeholder handlers; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the repo no longer underclaims or misreports the basic runtime balloon target update path after boot, but GH `#2` still needs more endpoint-level parity work before the full OpenAPI surface stub can be considered complete

### 2026-04-06 — WOR-20 — Balloon config-store slice aligned to pinned baseline
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: implement truthful `GET /balloon` and `PUT /balloon` behavior, `/vm/config` balloon export, snapshot-load blocking, and machine-config coupling for configured balloon state
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the repo now matches the pinned baseline for the low-risk balloon create/read config path and its dependent guards, but runtime balloon update/stats/hinting work is still open
- Harness update: added concrete balloon config state and handlers in `src/api.rs`, extended API plus contract coverage for defaulted payloads and exact fault strings, and corrected stale balloon runtime semantics rows in `docs/specs/firecracker-feature-parity.md`
- Remaining risks: the wider route matrix still contains `pending_baseline_capture` placeholders or placeholder handlers; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: the repo no longer treats balloon config creation/read as a generic placeholder path, but GH `#2` still needs more endpoint-level parity work before the full OpenAPI surface stub can be considered complete

### 2026-04-06 — WOR-20 — Balloon patch/stats/hinting guards corrected to pinned runtime-only semantics
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: correct overclaimed balloon `PATCH /balloon`, statistics, and hinting route guards from PB/PBRT semantics to pinned runtime-only behavior, regenerate the matrix, and refresh contract expectations
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the semantics correction with documented gaps; the repo no longer overstates preboot support for the balloon update/stats/hinting routes, but concrete balloon handler behavior is still future work
- Harness update: corrected `src/api_matrix.rs`, refreshed `docs/specs/firecracker-feature-parity.md`, regenerated `docs/generated/firecracker-api-surface-stub-matrix.json`, and updated contract route-guard expectations to match the pinned Firecracker request/controller flow
- Remaining risks: the wider route matrix still contains `pending_baseline_capture` placeholders or placeholder handlers; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: balloon patch/statistics/hinting routes no longer overclaim preboot support, but the broader GH `#2` parity bar remains incomplete

### 2026-04-06 — WOR-20 — MMDS datastore/config surface semantics aligned to pinned baseline
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: implement truthful `/mmds` and `/mmds/config` behavior, Firecracker-shaped validation/defaults, `/vm/config` export, runtime config guard, and contract coverage
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; MMDS control-plane semantics now match the pinned baseline for this endpoint family, but GH `#2` remains broader than this slice
- Harness update: added concrete MMDS handlers and state to `vesperd`, enforced JSON merge-patch and runtime gating tied to configured `mmds-config`, exported `mmds-config` via `/vm/config`, blocked snapshot load when MMDS config exists, and extended API plus contract coverage for default GET bodies, validation errors, and baseline revision envelopes
- Remaining risks: the wider route matrix still contains `pending_baseline_capture` placeholders; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: MMDS routes no longer overstate placeholder behavior or omit pinned config/export semantics, but GH `#2` still needs more endpoint-level baseline capture before the full OpenAPI surface stub can be considered complete

### 2026-04-06 — WOR-20 — Firecracker hotplug memory surface semantics aligned to pinned baseline
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: implement truthful `/hotplug/memory` PB/PO semantics, Firecracker-shaped validation, `/vm/config` export, and contract coverage for defaults plus error envelopes
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; the route family is now truthful to the pinned Firecracker API surface, but GH `#2` remains broader than this one endpoint family
- Harness update: added concrete `/hotplug/memory` handlers and state to `vesperd`, regenerated `docs/generated/firecracker-api-surface-stub-matrix.json` with `PO`/`PO`/`PB` semantics for the hotplug routes, and extended contract coverage to assert exported `memory-hotplug` config, runtime status defaults, and invalid-request baseline envelopes
- Remaining risks: the wider route matrix still contains `pending_baseline_capture` placeholders; full-repo `cargo test` retains the previously documented sandbox proxy permission failure outside this slice; issue/PR update, tracker-state update, push, and CI are still pending
- Truthful project state after this milestone: `/hotplug/memory` no longer overstates its guard semantics or payload shape, but GH `#2` still needs additional endpoint-level baseline capture before the full OpenAPI surface stub can be considered complete

### 2026-04-06 — HARNESS-BOOTSTRAP — harness engineering overlay synced into repo
- GitHub issue: pending mapping
- PR: pending
- Commit: pending local commit
- Phase: M0
- Parity unit: repo harness bootstrap
- Result: keep
- Verification bundle: local harness smoke test; issue-scoped bundle not created yet
- Council summary: not run for the bootstrap sync
- Harness update: added docs map, structure/docs/quality/janitor sensors, Symphony templates, and Linear-backed workflow config
- Remaining risks: first tracker-linked issue run and CI activation still need to be exercised end to end
- Truthful project state after this milestone: repo-local harness is installed locally; tracker and remote verification still need follow-through on the next ticket

### 2026-04-06 — WOR-20 — Firecracker route state-guard semantics aligned to parity matrix
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: align documented PB/PO/Any semantics, validate stub requests, persist low-risk config stubs, and tighten snapshot state guards for pinned Firecracker routes
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; GH #2 remains open because baseline-driven NotImplemented/status matrix work is still outstanding
- Harness update: `AppState` now requires a documented semantics mapping for every pinned route; drive/network/pmem stubs persist validated config and export via `/vm/config`; snapshot stubs validate Firecracker-shaped request bodies and now enforce paused/fresh-process state guards before falling through to placeholder results; contract tests assert PB/PO/Any behavior and representative invalid-request cases across the full OpenAPI surface
- Remaining risks: full-repo `cargo test` still hits the pre-existing sandbox permission failure in `proxy::tests::forwards_round_trip_traffic`; issue/PR update and push are still pending
- Truthful project state after this milestone: the route-surface scaffold no longer over- or under-guards documented Firecracker states, now rejects representative malformed stub requests correctly, now persists low-risk drive/network/pmem config stubs, and now enforces key snapshot state guards, but the broader GH #2 acceptance bar is not yet complete

### 2026-04-06 — WOR-20 — durable Firecracker surface-stub matrix generated and enforced
- GitHub issue: #2
- PR: none
- Commit: pending local commit
- Phase: 0
- Parity unit: replace the implicit blanket `NotImplemented` fallback with a generated, baseline-pinned surface-stub matrix artifact and enforce it at runtime
- Result: keep
- Verification bundle: `.symphony/runs/WOR-20/current/`
- Council summary: sequential local council approved the slice with documented gaps; GH #2 remains open because the generated matrix still marks routes as `pending_baseline_capture` until upstream behavior is captured
- Harness update: added `src/api_matrix.rs`, `baselinectl --mode generate-api-matrix|validate-api-matrix`, and `docs/generated/firecracker-api-surface-stub-matrix.json`; `vesperd` and contract tests now load the generated matrix instead of relying on an in-memory blanket default, and contract tests assert that a stubbed route response matches the generated artifact
- Remaining risks: full-repo `cargo test` still hits the pre-existing sandbox permission failure in `proxy::tests::forwards_round_trip_traffic`; `python3 scripts/harness/check_structure.py --root .` still fails because `src/api.rs` remains over the line-budget error threshold tracked in TD-001; upstream baseline capture for remaining placeholder routes is still missing; issue/PR update and push are still pending
- Truthful project state after this milestone: the repo now has a durable, validated, runtime-enforced surface-stub matrix for every pinned Firecracker route, but GH `#2` still needs upstream-captured baseline status/error values before parity can be claimed complete
