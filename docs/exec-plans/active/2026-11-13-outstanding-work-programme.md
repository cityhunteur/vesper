# Outstanding work programme: code complete, hardening, then the Linux reference run

This plan sequences every outstanding item agreed on 2026-11-13 ("proceed with all outstanding items; keep Linux run
last; ask for review first"). It is a programme plan: each workstream below ships as its own pull request tied to its
issue or tech-debt row, and the larger ones (#172, #160) get their own ExecPlan when they start. The plan is in
review. The owner approved it on 2026-11-13 (D-029).

## Purpose / Big Picture

After this programme, Vesper reaches "code complete" for Backend A. ADR 0007 is done, so the remaining piece is #172:
the upstream Firecracker integration suite runs through a committed script, and a comparator classifies every
result. The known runtime gaps a user can hit are also closed or bounded:

- the console logs (TD-055);
- the shared memory budget (TD-056);
- the snapshot store (TD-053);
- the per-microVM flow cap (TD-058);
- host shares inside microVMs (#160).

Only then does the CE-0003 Linux reference run happen, once, against the finished code. That is decision D-024, and
it is why the run comes last.

## Progress

- [x] Owner reviews and approves this plan, including the decisions marked "owner" in the Decision Log (2026-11-13, D-029)
- [x] W1 Housekeeping: remove the merged worktrees; split `src/systemvm.rs` (TD-043) (2026-11-13, #187)
- [x] W2 TD-058 follow-up: flood test of the 1,024-flow cap; conntrack count after teardown (positive and negative-control runs 2026-11-13, `run/TD-058/egress-flood-*.json`)
- [ ] W3 #172: `make systemvm-upstream-suite` and the comparator (own ExecPlan)
- [x] W4 Runtime bounds: console logs (TD-055), memory budget (TD-056), snapshot store quota (TD-053) (PR #191;
  runtime evidence `run/W4/20261113T170614Z`: 7 suites, 121 of 121 steps, and a microVM that outlives its console bounder)
- [ ] W5 #160: host shares into microVMs over NFS from `unfs3` (own ExecPlan; dependency approved in D-021)
- [ ] W6 ctr-conformance hang: reproduce, then fix or record it
- [x] W7 Repository settings: the owner applies them (D-029); nothing for the agent to do
- [ ] W8 CE-0003 Linux reference run (gated: quote and quota approval)
- [ ] Outcomes & Retrospective written; plan moved to completed

## Surprises & Discoveries

- W1: one `cargo test` run failed one test (300 passed, 1 failed) on the first run after the split. It did not come
  back in 8 later runs on the branch or on `main`, and the failing test's name was not captured. It is part of W6.
- W2: the 1,024-flow cap holds in each layer: the namespace held 1,024 of about 1,380 distinct flows (a few
  datagrams of the 1,400 share a flow). A stopped lease leaves its flows in the System VM's table: 1,028 before
  teardown and 1,028 after. They age out on the kernel's timers, which are 30 s for UDP and 5 days for established
  TCP (read, not measured).
- W2 council review (`run/TD-058/council-w2.md`): the first root-alone check passed with the cap broken in several
  ways, because it summed every root drop and had no upper bound. Both flood steps now judge the cap rules' own
  counters, with bounds on both sides, and the installed cap values. Only a connlimit rule whose target is DROP
  counts. The first run of that version failed: `iptables -nvxL` prints the rule as `#conn src/0 > 1024`, not
  "connlimit", so the counter readout never matched. With that fixed the suite passes 12/12 (namespace rule dropped
  351, root rule alone 349; `egress-flood-positive.json`), and each negative control fails both flood steps or the
  one it targets: no namespace cap rule (`-no-ns-cap`), no root cap rule (`-no-root-cap`), and the root rule with
  `--connlimit-mask 0` (`-root-cap-mask0`), whose installed cap still reads 1,024 but whose rule dropped all 1,400.
  Not covered (council #8, #9): a flood split across two destinations, TCP and DNS probes while the cap is
  saturated, and the cause of the ~20 flows fewer than datagrams sent.

## Decision Log

- 2026-11-13, owner review (D-029): order approved as written; W2 counts conntrack entries only (option a); W4 refuses a launch over the shared memory budget; W7 is left to the owner.
- 2026-11-13: order fixed as W1 to W8. W3 comes before W4 and W5 so that code complete for parity lands first. W4 and
  W5 change runtime behaviour, so W3's expected-results file is regenerated after them, before W8.
- 2026-11-13: W4 deviates from the plan for TD-053. The plan said to prune unreferenced store entries by age and
  total size. The store holds the user's own snapshots, which `vesperd` cannot tell from abandoned ones, and a
  snapshot that no lease references is still the user's only copy. So the store gets a quota (25% of the data disk)
  and a copy that would pass it is refused with `store quota: …`; nothing is removed automatically. `vesperd store
  list` reports the usage and the quota. Automatic pruning would need an owner decision on a retention rule.
- 2026-11-13: W8 compares against an expected-results file kept in the repo until the Linux list exists (#172
  acceptance criterion 3). The Linux list replaces that file in W8.
- **Owner**: W2 has two options for conntrack. Option a, the default, counts entries with
  `/proc/sys/net/netfilter/nf_conntrack_count` and adds no package. Option b adds `conntrack-tools` to the System VM
  package lock, so teardown can flush a lease's entries. Option b is a new runtime dependency.
- **Owner**: W4's memory budget (TD-056) adds a shared admission check: the shim and `vesperd` both refuse a launch
  that would commit more than the System VM's memory less a reserve for dockerd and containerd. This makes a new
  error that clients can see.
- **Owner**: W7 changes repository settings: private vulnerability reporting, Dependabot alerts and security updates,
  and protection of `main` (require the CI checks; no force-push). Protecting `main` also binds the agent's own
  merges.
- **Owner**: W8 needs the quote and the quota increase from 32 to 96 CPUs in
  `2026-11-12-CE0003-linux-reference-run.md`. It is not started without both.
- Out of scope, unless the owner adds them: the native backend epics (#56, #82 to #87, #91); TD-054 (a custom System
  VM kernel for userfaultfd); TD-057 (reattach); TD-051 (naming a security reviewer); the egress proxy and VPN gap
  from TD-058; #96, #103, #59.

## Outcomes & Retrospective

Not yet written.

## Context and Orientation

- Backend A runs Firecracker microVMs in the Alpine System VM. `vesperd --backend systemvm` (ADR 0007) and the
  containerd shim in `shim/` both launch microVMs through the helper `systemvm/overlay/usr/local/bin/vesper-vesperd`.
- The GH #166 trial ran the upstream v1.16.0 suite unmodified: 755 of 980 selected tests passed, and every failure
  was classified. Its scripts exist only in `run/GH-166/` and are not tracked.
- The suites are run by `systemvmctl control <suite>`, under `src/control/suites/`, and wrapped by `make systemvm-*`
  targets. TESTING.md lists them.
- The tech-debt rows are in `docs/exec-plans/tech-debt-tracker.md`, and decisions are in
  `docs/design-docs/decision-log.md`.
- The worktrees `../vesper-ce0003` (branch `docs/ce0003-linux-reference-plan`, PR #178) and `../vesper-public`
  (branch `chore/public-release-prep`, PR #180) are clean, and both PRs are squash-merged.

## Plan of Work

**W1.** Remove both worktrees and their local and remote branches. Their content is on `main` through squash merges.
Move share parsing (`SharedDirectory`, `shared_directories_from_specs`) from `src/systemvm.rs` into
`src/systemvm/shares.rs`, with no change in behaviour, so the structure sensor stops warning.

**W2.** Done as two steps, one per layer, so the egress suite has 12 steps. Add an egress-suite step. A `nat` guest opens more than 1,024 TCP flows to a public listener, and the step
asserts that the excess is dropped, using the `DROPS` counters and the guest probe results. The same step stops the
lease and records the System VM's conntrack count before and after. If option b is approved, teardown also deletes
the lease's entries. Update ADR 0008 and TD-058 with the measured result.

**W3.** Promote the trial scripts into the tree under `scripts/systemvm/upstream-suite/` and a
`src/control/suites/upstream.rs` driver. Pin the artefact prefix in the baseline lock. Add a working per-test timeout:
pytest-timeout, plus an outer watchdog that kills the test's Firecracker. Raise the System VM to 4 vCPUs and 8 GiB
for the run and restore the defaults on every exit path. Wipe `/srv` between runs. The comparator is a Rust binary
mode or a test-backed module. It reads `{nodeid: outcome}` for Backend A and either the Linux list or the in-repo
expected file. It matches every test that fails on Backend A but passes on the reference against a recorded
exception class (a node-id pattern plus an error signature), and exits non-zero on any unmatched test. The full
detail goes in its own ExecPlan.

**W4.** Make three bounds:
- **TD-055:** cap each console log (rotate at a fixed size and keep the last N KiB) and delete it at teardown after
  `vesperd log` has had its chance. A test floods the console and asserts the bound.
- **TD-056:** add one shared admission ledger under the helper's lock that both owners use (see the owner decision).
  A test fills the budget from both sides and asserts the refusal.
- **TD-053:** bound the store by a quota that refuses a copy past it, and report it in `vesperd store list`
  (changed from pruning; see the Decision Log).

**W5.** Follow #160's checklist in its own ExecPlan:
1. pin `unfs3`, bind it to the microVM-facing addresses only, and prove that a container cannot connect;
2. give the NFS client modules to the microVM initramfs;
3. map OCI binds in the shim;
4. carry the host-edit event hop;
5. run a `binds-suite` variant.

**W6.** Run `ctr-conformance` in a loop until the hang reproduces, capture the stacks, then fix it or record it as a
tech-debt row with the evidence.

**W7.** Apply the approved settings with `gh api`, or leave them as a checklist for the owner.

**W8.** Follow `2026-11-12-CE0003-linux-reference-run.md`, then swap the comparator's reference to the Linux list,
classify, and record the result in the parity matrix.

## Concrete Steps

Each workstream follows the repository workflow:
1. Create a branch, then make code and tests together.
2. Run the narrowest suite, then `cargo test`, `python3 harness/check_docs.py` and
   `python3 scripts/harness/check_structure.py --root .`. Runtime workstreams also run their System VM suite on a
   freshly booted System VM, with evidence under `run/<id>/`.
3. Run an llm-council implementation review for W2 to W5, with its report under `run/<id>/`.
4. Open the PR, wait for green CI, squash-merge, and post an issue comment with the SHA and the local and remote
   verification.
5. Add a decision-log line for each decision made.

## Validation and Acceptance

- **W1:** the structure sensor prints no TD-043 warning, and `git worktree list` shows only the main checkout.
- **W2:** the egress suite has 12 steps, all passing twice in one boot, and the flood step fails if the `connlimit`
  rule is removed.
- **W3:** `make systemvm-upstream-suite` completes with no hang, and the comparator exits 0 on the current results
  and non-zero on an injected unmatched failure. This meets #172's acceptance criteria.
- **W4:** each bound has a suite step that fails without the change.
- **W5:** #160's acceptance criteria are met.
- **W6:** the hang is fixed with a regression loop, or recorded with stacks.
- **W7:** the settings are visible in `gh api repos/cityhunteur/vesper`.
- **W8:** CE-0003's plan is complete and the comparator passes against the Linux list.

## Idempotence and Recovery

Every workstream is a separate PR and can be reverted alone. The System VM changes are hot-patched for testing, then
rebuilt with `make systemvm-overlay` and `make systemvm-up-snapshot`. A bad overlay is recovered by rebuilding from
`main`. W3's resize of the System VM restores the defaults in a trap. If a run dies, `make systemvm-up-snapshot`
restores them. W1's branch deletion is safe because both PRs are merged: their commits are on `main` as squash
commits, and the branch heads stay recoverable from the PR pages.
