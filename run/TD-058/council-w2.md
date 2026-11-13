## Code Review Council Report

**✅ Ready to merge** · 0 blockers
🔴 0 Critical · 🟠 0 High · 🟡 2 Medium · 🔵 7 Low · ▲ 0 coverage gaps
`src/control/suites/egress.rs` +2 more (11 files, +298 −17) · POSIX sh, Rust
Agents: Codex · Claude · Antigravity · Judge

To answer the three questions: the namespace step proves the cap, but the root-alone step does not. The root-alone step passes when the cap is broken in several ways, and nothing has yet shown that either step fails when the `connlimit` rule is removed. The `flows` helper change is safe: it is read-only and does not affect existing `net` callers. Its readout fails open to zero, but no assertion depends on it.

### Next steps
1. **Fix #1** before W2 counts as done → on a System VM, set `--connlimit-above 1` at `vesper-vesperd:355`, run `make systemvm-egress-suite` and confirm `flow_cap_drops_excess_in_root_alone` now fails
2. **Fix #2**: commit failing reports with each `connlimit` rule removed, or untick W2 → `git ls-files run/TD-058`
3. **Follow up** after merge: #3, #4, #5, #6, #7, #8, #9

### 🟡 Medium

**#1 · The root-alone flood step passes when the cap is broken**
`src/control/suites/egress.rs:429-432` · Correctness, Testing, Security · Codex F1, Claude F1, Antigravity F1 · ✓ verified

The step checks only that root drops are at least `(1400 − 1024) / 2 = 188`. It has no upper bound and no count of admitted flows. Also, `rtd` (`vesper-vesperd:1039-1040`) adds up every root DROP row, not just the connlimit rule. So the step still passes with `--connlimit-above 1`, with a cap near 1,190, with `--connlimit-mask 0`, or with the UDP `RETURN` at line 360 deleted, which sends all ~1,400 packets to the final DROP. The `--connlimit-mask 0` case passes because step 8 leaves ~1,028 root conntrack entries alive (`udp_s=30`), so with one shared budget the new lease gets no flows at all. Antigravity rated this High; the judge lowered it to Medium because it is a gap in test strength. Codex's 1,200-flow example would actually fail, because only ~1,379 distinct flows arrive; a cap of ~1,190 would pass. Codex's `review-pr188-repro.py` is not in the tree.
- **Verify:** `python3 -c 'FLOOD=1400; ok=lambda b,a,d: not (a != b or d < (FLOOD-1024)//2); assert ok(0,0,1380) and ok(0,0,1400) and ok(0,0,188)'` shows the predicate accepts all three broken cases. After the fix, the System VM run in Next steps 1 must fail.
- **Fix:** Bound `dropped` on both sides, for example `excess - 64 <= dropped && dropped <= excess` (the runs show 354–357 against an excess of 376). Better still, have `net` print the connlimit rule's own packet counter and judge that number.

**#2 · No artefact shows the acceptance criterion "fails if the `connlimit` rule is removed"**
`docs/exec-plans/active/2026-11-13-outstanding-work-programme.md:141` · Testing · Claude F2 · ✓ verified

The plan marks W2 done, but `run/TD-058` holds only two passing 12/12 runs. No run shows either step failing with the `connlimit` rule removed. Under CLAUDE.md, a checklist item needs evidence before it is ticked.
- **Verify:** `git ls-files run/TD-058` and `grep -n "fails if" docs/exec-plans/active/2026-11-13-outstanding-work-programme.md`
- **Fix:** Run the suite once with each connlimit rule removed and commit the failing reports next to the passing ones, or untick W2 until those reports exist.

### 🔵 Low

- **#3** `vesper-vesperd:1078-1082` · Observability · Codex F2, Antigravity F4 · ✓ verified (judge lowered both from Medium) — `flows` turns unreadable counters into `0` and exits 0. `root_flows` ignores `rc`, and `parse_flows` uses `unwrap_or(0)`. No assertion depends on these values today → return `ERR` with a non-zero exit, check `rc`, and parse the fields as `Option<u64>`.
- **#4** `src/control/suites/egress.rs:396-398,426` · Correctness, Reliability · Claude F5, Antigravity F5 · ✓ verified (Antigravity rated it Medium) — `root_flows(..)?` returns before `stop`, the second call hides the `ran` and `stopped` errors, and step 9 has `let _ = stop(..)` → keep the results as `Result`s and apply `?` after `ran?; stopped?;`. The `boot()` leak predates this diff.
- **#5** `vesper-vesperd:1045` · Design and maintainability · Claude F3 · ✓ verified — `cap=` prints `$NAT_FLOWS` back, not the cap in the installed rule, and the root step hard-codes `1024` → add one `const FLOW_CAP`, and drop `cap=` or read it from `iptables -S`.
- **#6** `docs/design-docs/adr/0008-vesperd-microvm-egress.md:132-135` · Design and maintainability · Claude F4 · ✓ verified — "exactly 1,024", the "measured" 5-day TCP timeout, "N sockets, so N flows" and the W2 paragraph (plan lines 89-92) all claim more than the code shows → say "up to N", "holds the cap (1,024 observed)" and "read, not measured", and rewrite W2.
- **#7** `src/control/suites/egress.rs:137` · Readability and idioms · Claude F6 · ✓ verified — the doc comment for `policy_field` now sits above `struct Flows` → move it back above `fn policy_field`.
- **#8** `src/control/suites/egress.rs:31` · Testing · Antigravity F2 · ? unverified (judge lowered it from High) — all flood traffic goes to one destination, and no TCP or DNS probe runs while the cap is saturated, so a `--connlimit-daddr` or DNS-ordering regression would not be caught → split the flood across two destinations and add `tcp:`/`dns:` probes that must come back `closed`.
- **#9** `vesper-microvm-init:40-44` · Concurrency · Antigravity F3 · ? unverified (judge lowered it from Medium) — 1,378–1,381 distinct flows arrive instead of 1,400, and each step takes ~14 s against a 30 s UDP timeout. The port-reuse cause is not proven → consider fixed source ports through `socat`.

### Coverage
Nobody ran a live negative control, so it is still unknown whether either step fails with `connlimit` removed (#2).

| Lens | Status | Evidence |
| --- | --- | --- |
| Security | covered | #1; Codex traced dispatch and counters |
| Privacy | covered | Prints only aggregate counters |
| Supply chain | covered | No new dependencies |
| Correctness | covered | #1, #4; flood parse traced |
| API and compatibility | covered | Existing `net` callers unaffected |
| Rollout and migration | n/a | Test plus read-only helper verb |
| Reliability | covered | #4 |
| Scalability and performance | covered | #9, mostly speculative |
| Concurrency | covered | #1 (leftover conntrack), #9 |
| Observability | covered | #3 |
| Testing | covered | #1, #2, #8 |
| Design and maintainability | covered | #5, #6 |
| Readability and idioms | covered | #7 |

### Consensus
| # | Codex | Claude | Antigravity | Judge |
| --- | --- | --- | --- | --- |
| 1 | ● | ● | ● | ✓ |
| 2 | ○ | ● | ○ | ✓ |
| 3 | ● | ○ | ● | ✓ |
| 4 | ○ | ● | ● | ✓ |
| 5 | ○ | ● | ○ | ✓ |
| 6 | ○ | ● | ○ | ✓ |
| 7 | ○ | ● | ○ | ✓ |
| 8 | ○ | ○ | ● | ? |
| 9 | ○ | ○ | ● | ? |

Dropped as false: none.
● flagged · ○ not flagged · ✓ verified · ? unverified · ✗ false

### Strengths
- The namespace step pins the cap: `held.ns` must fall in 960..1040, so removing the rule would fail it.
- The new `FLOWS` line prints only in the `nat` branch, and every existing `net` caller still parses correctly.
- `STEPS` = 12 matches the step calls, `src/README.md` and `docs/TESTING.md`.
