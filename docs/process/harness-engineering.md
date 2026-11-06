---
status: verified
owner: cityhunteur
last_reviewed: 2026-11-05
---

# Harness Engineering for Vesper

This document adapts harness-engineering practices to Vesper’s repository and runtime constraints.

## What changes for Vesper

Vesper is not a generic CRUD app. It is a parity-sensitive runtime and control-plane project. That means:
- route reachability is not enough,
- runtime evidence matters,
- status drift is dangerous,
- merge-gate minimalism must be adapted carefully.

Tracker truth is split intentionally:
- Linear carries planning state for the project.
- GitHub issues and PRs carry the durable engineering evidence.
- When they disagree, trust the code and GitHub evidence, then repair the tracker state.

## Guides and sensors

### Feedforward guides

These tell the agent how to work before it writes code:
- `AGENTS.md`
- `docs/README.md`
- `ARCHITECTURE.md`
- `docs/PLANS.md`
- `docs/RELIABILITY.md`
- `docs/SECURITY.md`
- `docs/specs/design-v0.md`
- `docs/specs/firecracker-feature-parity.md`
- `WORKFLOW.md`
- `.symphony/templates/*`

### Computational sensors

These are deterministic or mostly deterministic checks:
- `make contract-check`
- `make contract-test`
- `make validate-entitlements`
- `cargo test`
- `cargo run --bin baselinectl -- --mode validate`
- `cargo run --bin compatexctl -- --registry docs/specs/compat-exceptions.yaml`
- `python3 scripts/harness/check_docs.py --root .`
- `python3 scripts/harness/check_structure.py --root .`
- `python3 scripts/harness/compute_quality_score.py --root .`
- `python3 scripts/harness/janitor_scan.py --root .`

### Inferential sensors

These are judgment-heavy:
- LLM council review
- human review
- milestone truth checks against the parity matrix

## Missing capability protocol

When a task fails, ask which of these is missing:
- a guide,
- a sensor,
- a durable artifact path,
- or a smaller issue slice.

Fix that gap first. Do not keep retrying the same underspecified task.

## Source of truth

Treat these as authoritative for parity-sensitive work:
- `docs/specs/design-v0.md`
- `docs/specs/firecracker-feature-parity.md`
- the mapped GitHub issue or PR discussion and artifacts
- `.symphony/runs/<ticket>/current/verification.yaml`

Do not mark parity complete from:
- tracker closure alone,
- old eval comments,
- route reachability scaffolding,
- or merged code without updated evidence.

## Progressive disclosure

Do not start with a giant instruction blob. Start from:
1. `AGENTS.md`
2. `docs/README.md`
3. only the specific docs needed for the task.

This keeps context focused and reduces stale or irrelevant guidance.

## Required per-slice workflow

1. Pick the smallest parity or harness unit that can be completed and verified independently.
2. Tie that unit to the relevant tracker ticket and mapped GitHub issue before changing status claims.
3. Implement the code and the regression tests or sensors together.
4. Run the narrowest meaningful local checks first, then one broader proof before push.
5. Commit each completed unit separately with an issue-linked message.
6. Push after each completed unit so GitHub Actions run against the real commit.
7. Monitor CI or required checks to completion; do not assume success from local green runs.
8. Update the mapped GitHub issue with what changed, the commit SHA, local verification commands, current CI state, and remaining gaps.
9. Advance the tracker only when the code, tests, artifacts, and GitHub evidence all agree.

## Architecture enforcement

Use docs and scripts together:
- `ARCHITECTURE.md` defines allowed boundaries,
- `scripts/harness/check_structure.py` checks them mechanically,
- `docs/QUALITY_SCORE.md` tracks whether coherence is improving.

## Runtime legibility

Vesper does not have a browser UI, so legibility must come from:
- API traces,
- error envelopes,
- proxy logs,
- System VM manifests and boot reports,
- durable command transcripts.

Use:
- `scripts/harness/init_issue_run.sh`
- `scripts/harness/collect_verification_bundle.sh`
- `docs/RELIABILITY.md`

Every issue attempt should leave behind:
- `.symphony/runs/<ticket>/current/verification.yaml`
- `.symphony/runs/<ticket>/current/notes.md`
- `.symphony/runs/<ticket>/results.tsv`
- `.symphony/runs/<ticket>/current/council/`
- copied runtime artifacts from `run/` when relevant

## Garbage collection

Run janitor tasks on a cadence:
- scan docs for broken links or missing maps,
- scan for missing explicit route handlers,
- scan for oversized hotspots,
- keep quality grades current,
- open or record targeted cleanup work.

Use:
- `scripts/harness/janitor_scan.py`
- `scripts/harness/compute_quality_score.py`
- `docs/exec-plans/tech-debt-tracker.md`

## Tracker and GitHub update protocol

- Every completed unit must leave a tracker artifact behind: tracker state change, GitHub comment, milestone entry, or all three.
- Post the tracker update only after push and after the first CI or required-check signal is available.
- Include both local verification and remote verification in the GitHub update.
- If CI is still running, say so explicitly and follow up with the final result.
- If CI fails, record the failing workflow name, failing SHA, and remediation plan before moving the unit forward.
- Do not mark checklist items or milestones complete from local testing alone if remote checks are still red or pending.

## Merge philosophy: adapted, not copied

OpenAI’s write-up notes that minimal blocking merge gates were a good tradeoff in their very high-throughput environment, and also says that would be irresponsible in a lower-throughput environment. Vesper should adopt the harness ideas, not the exact merge looseness.

For Vesper, keep blocking gates for:
- contract correctness,
- baseline and compat governance,
- docs integrity,
- architecture integrity,
- security-sensitive changes,
- runtime-sensitive evidence where applicable.

## Git rules

- Never commit unrelated user changes already present in the worktree.
- If the worktree is dirty, stage only the files for the current unit.
- Prefer commit messages that map back to the issue or ticket, for example:
  - `feat(api): implement machine config + readback parity (#2)`
  - `docs(harness): install Vesper harness engineering overlay`

## Verification checklist

Before push:
- Run the narrowest tests or sensors that cover the touched behavior.
- Run at least one broader regression command when the slice changes shared code or API semantics.
- Confirm `git status --short` contains only the intended files for the current unit.

After push:
- Check the GitHub Actions runs or required checks for the pushed SHA.
- Record the initial remote check state in GitHub.
- Follow the checks through completion and post the final result.
- Update the tracker or milestone state only after the evidence is current.

## Phase 0 expectations

- Every route in `third_party/firecracker/firecracker.yaml` must eventually be handled by `vesperd`.
- `501 Not Implemented` is not parity.
- State-dependent routes must enforce Firecracker-style preboot and runtime rules.
- Error responses must stay deterministic and include the pinned baseline revision.
- Contract tests must fail if a route regresses to reachability-only scaffolding.

## Success criteria

Harness engineering is working for Vesper when:
- issue state no longer outruns evidence,
- agents can discover the right design context from the repo,
- each accepted slice leaves a durable bundle,
- review feedback increasingly lands as docs, scripts, or templates,
- janitor work shrinks over time instead of growing.
