---
tracker:
  kind: linear
  project_slug: "vesper-d61461c63277"
  auto_manage_queue: true
  backlog_state: Backlog
  todo_state: Todo
  in_progress_state: In Progress
  active_states:
    - Todo
    - In Progress
  terminal_states:
    - Done
    - Closed
    - Cancelled
    - Canceled
    - Duplicate
polling:
  interval_ms: 10000
workspace:
  root: ~/symphony/vesper-workspaces
hooks:
  after_create: |
    git clone --origin origin "${SYMPHONY_TARGET_REPO:?set SYMPHONY_TARGET_REPO}" .
    if [ -n "${SYMPHONY_SOURCE_WORKTREE:-}" ]; then
      rsync -a \
        --exclude '.git' \
        --exclude 'target' \
        --exclude 'build' \
        --exclude 'run' \
        --exclude '.symphony/runs' \
        --exclude '.symphony/council' \
        --exclude '.symphony/progress' \
        --exclude '.symphony/last-git-status.txt' \
        "${SYMPHONY_SOURCE_WORKTREE%/}/" .
    fi
    git fetch --all --tags --prune
    mkdir -p .symphony/runs .symphony/council .symphony/progress docs/progress run
    if command -v cargo >/dev/null 2>&1; then
      cargo fetch || true
    fi
  before_run: |
    mkdir -p .symphony/runs .symphony/council .symphony/progress docs/progress run
  after_run: |
    git status --short > .symphony/last-git-status.txt || true
agent:
  max_concurrent_agents: 4
  max_turns: 16
codex:
  command: codex app-server
  approval_policy: never
  thread_sandbox: workspace-write
  turn_sandbox_policy:
    type: workspaceWrite
observability:
  dashboard_enabled: true
  refresh_ms: 1000
server:
  port: 4010
  host: 127.0.0.1
---

You are working on a Vesper delivery ticket `{{ issue.identifier }}`.

{% if attempt %}
Continuation context:
- This is retry attempt #{{ attempt }} because the ticket is still active.
- Resume from existing docs, artifacts, and notes instead of restarting.
- Re-run only the sensors that are invalidated by the new diff or missing evidence.
{% endif %}

Ticket context:
Identifier: {{ issue.identifier }}
Title: {{ issue.title }}
Current status: {{ issue.state }}
Labels: {{ issue.labels }}
URL: {{ issue.url }}

Description:
{% if issue.description %}
{{ issue.description }}
{% else %}
No description provided.
{% endif %}

## Required reading order

1. `AGENTS.md`
2. `docs/index.md`
3. The smallest relevant docs for the task:
   - API / parity work:
     - `docs/specs/design-v0.md`
     - `docs/specs/firecracker-feature-parity.md`
     - `docs/ARCHITECTURE.md`
     - `docs/RELIABILITY.md`
   - governance / policy work:
     - `docs/process/baseline-governance.md`
     - `docs/process/compat-exceptions.md`
     - `docs/SECURITY.md`
   - system VM or proxy work:
     - `docs/process/systemvm-bringup.md`
     - `docs/RELIABILITY.md`
     - `docs/SECURITY.md`
   - planning / progress work:
     - `docs/PLANS.md`
     - `docs/progress/symphony-milestones.md`
4. The latest verification bundle for this issue, if it exists.

## Operating model

- The repository is the system of record.
- `AGENTS.md` is a map, not a manual.
- GitHub evidence outranks tracker optimism.
- If you cannot prove a change with the current harness, improve the harness first.
- Never mark parity complete from issue closure, route reachability, or tracker state alone.

## Required durable artifacts for every issue attempt

Store all run artifacts under `.symphony/runs/{{ issue.identifier }}/`.

Every attempt must leave behind:
1. `current/verification.yaml`
2. `current/notes.md`
3. `results.tsv` append-only ledger
4. `current/council/` verdict files
5. A GitHub issue or PR update with the same evidence
6. Any relevant runtime artifacts copied from `run/` into the issue bundle

The branch may advance only when `verification.yaml` ends with `status: keep`.

## Harness-first execution rules

When something fails, do not default to “prompt harder.”
Ask:
1. Is the issue underspecified?
2. Is the repo missing a guide?
3. Is the repo missing a sensor?
4. Is the repo missing a durable artifact path?
5. Is the issue too large and needs to be split?

If the answer to any of the first four is yes, add the missing harness piece before shipping more feature code.

## Tracker state mapping

- `Backlog`: parked work that is not yet selected for an execution run.
- `Todo`: eligible for execution, no verified implementation work started.
- `In Progress`: code or harness changes are in flight.
- `In Review`: non-executing checkpoint state. Use this when a committed `keep`
  exists but the slice still needs a pushed branch, PR, CI signal, or human
  review before more execution should continue.
- `Done`: merged and documented.
- `Canceled` / `Cancelled` / `Duplicate`: terminal states.

Local verification phases such as `Verification` and `Council` still belong in
`verification.yaml`, `notes.md`, and the GitHub update, but they are not
separate Linear workflow states for this project.

## Linear queue hygiene

Use `.codex/skills/linear/SKILL.md` for tracker writes and queue decisions.

When `tracker.auto_manage_queue` is enabled, Symphony's orchestrator should own
the routine execution queue:
- park blocked queued work from `Todo -> Backlog`,
- promote the next ready dependency slice from `Backlog -> Todo`, and
- move dispatched work from `Todo -> In Progress`.

The agent still owns truthfulness:
- verify those transitions happened,
- repair them when the tracker drifted or a checkpoint changed the right state,
- and explain any promotion or parking decision in the tracker update.

If the orchestrator is unavailable, misconfigured, or leaves the project idle
while ready backlog work exists, the planner becomes the fallback queue
manager:
- identify the next ready backlog slice by dependency order first, then
  priority, then age,
- promote only the single truthful next slice to `Todo`,
- and avoid waking blocked or speculative future work.

Before ending a run, parking an issue in `In Review`, or marking an issue
`Done`, reconcile the next ready work in the same Linear project:

1. Query same-project issues in `Backlog`, `Todo`, `In Progress`, and `In Review`.
2. Identify ready `Backlog` issues:
   - all explicit blockers are terminal,
   - the issue is the next dependency slice unblocked by the current work, and
   - no higher-priority ready issue should go first.
3. Promote the next ready issue from `Backlog -> Todo`.
4. Only promote multiple issues when they are independently ready and the
   workflow intentionally supports parallel execution.
5. If a `Todo` issue is found to be blocked again before active work starts,
   move it back to `Backlog`.
6. Mention any queue promotion or parking decision in the tracker update.

## Step 0: Resolve ticket identity and restore truth

1. Determine the mapped GitHub issue from the tracker ticket.
2. Read the mapped issue, linked PR if any, and the latest verification artifact.
3. Check whether the issue state overclaims implementation reality.
4. If tracker state and code disagree:
   - repair the tracker state,
   - reopen or replace the GitHub issue if needed,
   - record the mismatch in the issue notes,
   - proceed only after truth is restored.

## Step 1: Initialize the issue run and capture baseline

1. Record:
   - branch,
   - `git status --short`,
   - `git rev-parse --short HEAD`.
2. Run `scripts/harness/init_issue_run.sh {{ issue.identifier }}` if available.
3. Create `.symphony/runs/{{ issue.identifier }}/current/` if it does not exist.
4. Write the baseline verification manifest before code changes.
5. Define exactly one smallest verifiable parity unit for this attempt.
6. Capture the baseline signal:
   - failing or missing behavior,
   - missing route parity,
   - missing transport or system VM evidence,
   - missing doc or structure sensor when the issue is a harness issue.
7. If the orchestrator has not already done it, repair `Todo -> In Progress`
   once baseline capture confirms active work has started.

## Step 2: Implement one verifiable unit

1. Change only the code or harness needed for the current unit.
2. Update tests or sensors together with the implementation.
3. Keep diffs narrow and behavior-oriented.
4. Prefer legible, inspectable solutions over opaque or clever ones.
5. Use the smallest relevant sensor set first:
   - API control plane:
     - `make contract-check`
     - targeted `cargo test` for the changed API behavior
     - `make contract-test` if route semantics changed
   - governance:
     - `cargo run --bin baselinectl -- --mode validate`
     - `cargo run --bin compatexctl -- --registry docs/specs/compat-exceptions.yaml`
   - proxy and transport:
     - targeted `cargo test proxy`
     - direct forwarding proof for unix, tcp, or vsock
   - system VM:
     - targeted `cargo test systemvm`
     - `bash scripts/systemvm/generate-launch-config.sh`
     - `bash scripts/systemvm/boot-smoke.sh` when the environment supports it
   - docs or harness changes:
     - `python3 scripts/harness/check_docs.py --root .`
     - `python3 scripts/harness/check_structure.py --root .`
     - `python3 scripts/harness/compute_quality_score.py --root .`
6. Do not mark the slice ready for checkpoint or review until the narrow sensor
   set for the changed unit is green.

## Step 3: Collect computational sensors and artifacts

1. Populate `verification.yaml` using `.symphony/templates/verification-manifest-template.yaml`.
2. Run the candidate sensor bundle:
   - targeted checks,
   - one broader regression set,
   - parity or runtime proof when the issue is parity-sensitive,
   - docs/structure/quality sensors when the harness changed.
3. Capture durable artifacts:
   - command transcripts,
   - API traces or error envelopes when relevant,
   - `build/systemvm/*.json` and `run/systemvm/*` when relevant,
   - proxy or socket-forwarding logs when relevant,
   - docs or janitor reports when relevant.
4. Run `scripts/harness/collect_verification_bundle.sh {{ issue.identifier }}` if available.

## Step 4: Autoresearch-style keep/discard verification

Treat the attempt like an experiment.

1. Compare candidate evidence to baseline evidence.
2. Classify the result:
   - `keep`
     - acceptance criteria met,
     - narrow sensors pass,
     - broad sensors pass,
     - no untracked regression,
     - evidence is durable.
   - `discard`
     - code changed, but parity value is ambiguous, under-proven, or not actually better than baseline.
   - `crash`
     - build, tests, runtime proof, or harness integrity failed.
3. Append the attempt to `.symphony/runs/{{ issue.identifier }}/results.tsv`.
4. If `discard` or `crash`:
   - revert or keep the issue in `In Progress` with a failure note,
   - explain the failure in `notes.md`,
   - do not advance the branch.
5. If `keep`:
   - preserve the verification bundle,
   - continue to council review.

## Step 5: LLM council and checkpoint gate

Run a multi-agent council after `keep` and before `In Review`.

Minimum roles:
1. `parity-judge`
2. `runtime-judge`
3. `regression-judge`
4. `release-judge`

Council input packet:
- issue description,
- mapped GitHub issue or PR,
- diff or commit range,
- `verification.yaml`,
- `notes.md`,
- relevant design docs,
- relevant parity matrix section,
- exact command list and artifacts.

Council gate:
- Any `reject` blocks promotion.
- `approve-with-gaps` is acceptable only when the gaps are documented and do not invalidate acceptance criteria.
- Persist all verdicts under `.symphony/runs/{{ issue.identifier }}/current/council/`.
- If true parallel council execution is unavailable, run the same rubric sequentially and record `council_mode: sequential`.

Before any checkpoint or review-style tracker transition:

1. Create at least one issue-linked commit for the accepted slice.
2. Record the resulting short SHA in `notes.md`, `verification.yaml`, and the
   GitHub update draft.
3. A local-only dirty workspace with `status: keep` is not a completed
   checkpoint.

After council, decide whether the run may continue:

1. If the issue still maps to an umbrella GitHub tracker, or the accepted slice
   still lives only as a local commit without a pushed branch or PR, or there is
   no issue-specific branch or PR yet:
   - write a checkpoint request in `notes.md` and the GitHub update draft,
   - move the Linear issue to `In Review`,
   - promote the next ready dependency slice from `Backlog` to `Todo` when it
     is now the truthful next item,
   - stop after the current slice instead of starting another parity unit in
     the same run.
2. Only continue past the current slice when the accepted work already has a
   clean issue-specific branch/commit/PR path and the next step is still within
   the same bounded issue.

## Step 6: GitHub update and milestone logging

1. Update the mapped GitHub issue or PR with:
   - parity unit or harness unit completed,
   - files changed,
   - short SHA,
   - sensors run,
   - keep/discard result,
   - council summary,
   - remaining risks,
   - truthful next state.
2. Update `docs/progress/symphony-milestones.md` for any accepted issue or truth-changing milestone.
3. Never post “done” without:
   - the verification bundle path,
   - exact commands run,
   - current or pending CI signal.
4. For checkpoint stops, the update must explicitly say:
   - the workspace is intentionally parked,
   - the accepted slice is already committed,
   - the branch / PR / CI step is now required,
   - the tracker was moved to `In Review`,
   - whether the next ready issue was promoted to `Todo`,
   - no further parity slices should start from this issue until the checkpoint
     is resolved.

## Step 7: Feed human taste back into the repo

If this attempt uncovered recurring review comments, stale guidance, or repeated mistakes:
- update `AGENTS.md` only if the map itself is wrong,
- otherwise update the deeper doc in `docs/`,
- promote recurring review taste into a script, workflow, or template when possible,
- record janitor follow-up work in `docs/plans/tech-debt-tracker.md`.

## Promotion rules

Move to `In Review` and stop the run when all are true:
- attempt status is `keep`,
- the council has no rejects,
- the accepted slice already exists as an issue-linked commit,
- and any of these are still true:
  - the issue is an umbrella tracker,
  - the workspace is dirty after the accepted slice,
  - the branch is not pushed,
  - there is no PR yet,
  - CI has not produced its first truthful signal.

Do not begin another parity slice from the same issue after a checkpoint stop.

Move to `Done` only when all are true:
- attempt status is `keep`,
- required sensors are green,
- runtime proof exists for runtime-sensitive changes,
- the council has no rejects,
- GitHub issue or PR is updated,
- the branch is pushed,
- CI is green or clearly pending with no known local failure,
- tracker state matches reality,
- the next ready backlog issue is promoted to `Todo`, or the tracker update
  explains why no follow-up issue is ready yet,
- the bounded issue has actually been merged or otherwise fully landed.

If approved by a human:
- move to `Done`,
- leave the final milestone entry in the repo and GitHub issue.

## Vesper-specific focus rules

- Start from Phase 0 truth restoration, not from optimistic roadmap state.
- Prefer behavior slices over large epics.
- A reachable route is not parity.
- For backend, proxy, or system VM work, runtime evidence is mandatory.
- `src/api.rs` is an active hotspot: avoid making it the dumping ground for new behavior when a modular extraction is practical.
- Replace optimistic issue promotion with evidence-driven progression; do not keep both philosophies active at once.

## Final response contract for each run

Report only:
- what changed,
- sensor result,
- council result,
- current state,
- blockers if any.
