---
status: verified
owner: cityhunteur
last_reviewed: 2026-11-11
---

# Decision log

This is the running record of decisions about what Vesper is for and how it is built. It is append-only: a
decision that changes gets a new entry that names the one it replaces. Newest entries are at the top.

Use it for decisions that are too small for an [ADR](adr/index.md) or that span several plans. A decision with
lasting architectural consequences still gets an ADR, and the entry here links to it. Decisions made while carrying
out one plan stay in that plan's Decision Log; add an entry here only when the decision outlives the plan.

Each entry gives the decision, who made it, why, and where it is carried out. "Maintainer agent" means the coding
agent working under the delegation in D-001.

| ID | Date | Decision | Decided by | Why | Carried out in |
|---|---|---|---|---|---|
| D-008 | 2026-11-11 | Next larger work is, in order: connect `vesperd` to Backend A; run Docker containers in microVMs; only then Phase 5 (native backend). | Maintainer agent | The design says each container runs in a microVM and that `vesperd` drives Firecracker. The code does neither: Docker containers run on runc and `vesperd` is an in-memory state machine. Phase 5 needs a working Backend A control path to be compared against. | Not started; drift report of 2026-11-10 (`run/egm/`, local), [design-v0.md](../specs/design-v0.md) |
| D-007 | 2026-11-11 | EGM review and drift reports are written to `run/egm/`, which git ignores. Their conclusions are carried into the repository through the documents they change and through this log. | Maintainer agent | The reports are working papers that go stale with the next commit; the checked-in documents are the record. | [CLAUDE.md](../../CLAUDE.md) |
| D-006 | 2026-11-11 | The share watcher never waits for the guest: writes to the sink are non-blocking and the exit report reads published counters. A pluggable sink connector lets a self-test stand in for the guest. | Maintainer agent | The council's runtime judge rejected GH #150: a guest that stopped reading port 2401 could hold up SIGTERM until the launcher was killed. | `launcher/Sources/vesper-vmlauncher/ShareWatcher.swift`, `scripts/macos/share-watch-selftest.sh` |
| D-005 | 2026-11-11 | The guest replay first sets the file's access time, then writes the modification time back; if the file size changed in between, it sets the modification time to now. The claim "host mtimes are never changed" is withdrawn and replaced by "the replay never leaves a host mtime older than the last host write, except for a same-size overwrite inside the two-step window". | Maintainer agent | A new suite step showed the original replay moved host mtimes backwards by about a second when one file was edited twice quickly: the guest read the mtime from its attribute cache. The reply to an access-time set carries current attributes, which makes the next read correct. A replay that changes host access times is acceptable; one that moves modification times backwards breaks build tools. | `systemvm/overlay/usr/local/bin/vesper-share`, `scripts/systemvm/binds-suite.sh` step `inotify_host_rapid_edits_mtime_preserved`, [SECURITY.md](../SECURITY.md) |
| D-004 | 2026-11-11 | Runtime-sensitive units get the four-judge council before their issue closes, and a `reject` is fixed before the issue moves. | cityhunteur (asked for the council on GH #150); maintainer agent (makes it the rule) | The GH #150 council found a shutdown hang and a modification-time rollback that CI and the suites had passed. Most earlier milestone entries record the council as skipped. | [WORKFLOW.md](../../WORKFLOW.md) Step 5, [milestone log](../progress/symphony-milestones.md) |
| D-003 | 2026-11-10 | GH #29 item 3 ("inotify/watch events propagate across mount boundary") is accepted as met with the residuals in TD-038: events are modify-style only, on watched read-write shares only. | cityhunteur | Development servers and file watchers react to the events that are delivered; native event fidelity is not available over virtio-fs. | [Tech debt TD-038](../exec-plans/tech-debt-tracker.md), [GH #150 plan](../exec-plans/completed/2026-11-10-GH150-host-edit-inotify.md) |
| D-002 | 2026-11-10 | Host edits reach containers through a host FSEvents watcher in the Swift launcher and a guest replay; on by default for read-write shares, `PATH:nowatch` opts out, read-only shares get no events. | cityhunteur | Recorded in full in the GH #150 plan's Decision Log. | [GH #150 plan](../exec-plans/completed/2026-11-10-GH150-host-edit-inotify.md) |
| D-001 | 2026-11-10 | The maintainer agent owns the codebase: what it is for and how it is built are its decisions, it improves the code when needed without asking, and it keeps this log. The "Ask a human first" list in [AGENTS.md](../../AGENTS.md) still applies. | cityhunteur | The owner delegated direction and day-to-day design so that work does not wait on review of each choice. | This log; [AGENTS.md](../../AGENTS.md) |
