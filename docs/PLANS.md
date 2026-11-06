---
status: verified
owner: cityhunteur
last_reviewed: 2026-11-05
---

# Execution plans (ExecPlans)

Execution plans are first-class repository artifacts. An ExecPlan is a design document a coding agent can follow to
deliver a working change with only the current working tree and the plan file. The format is adapted from OpenAI's
[Using PLANS.md for multi-hour problem solving](https://developers.openai.com/cookbook/articles/codex_exec_plans).
`python3 harness/check_docs.py` enforces the required sections listed in `harness/config.toml`.

## When to write one

Write an ExecPlan in `docs/exec-plans/active/` when:

- the task spans multiple commits or issue slices,
- runtime evidence is required (proxy, System VM, boot smoke),
- the task mixes code and harness changes,
- the task touches architecture boundaries in [ARCHITECTURE.md](../ARCHITECTURE.md),
- the task is likely to be resumed across sessions or agents.

A small one-file fix may rely on the issue description alone, but any parity or runtime-sensitive change should
usually have a plan.

## Lifecycle in this repository

1. Create `docs/exec-plans/active/<YYYY-MM-DD>-<issue-id>-<short-name>.md` from the skeleton below and add a row to
   [exec-plans/active/index.md](exec-plans/active/index.md). Include the Linear ticket or GitHub issue in the name.
2. Keep the plan current while working. It must always be possible to restart from the plan alone.
3. When the outcome is reached and every Progress item is checked, write Outcomes & Retrospective, move the file to
   `docs/exec-plans/completed/`, and move its row to [exec-plans/completed/index.md](exec-plans/completed/index.md).
   Link the milestone entry in [progress/symphony-milestones.md](progress/symphony-milestones.md).
4. Record leftover work in the [tech-debt tracker](exec-plans/tech-debt-tracker.md) or a new plan.

Do not delete completed plans; their Decision Logs explain why the code looks the way it does.

## Requirements

- Every ExecPlan is fully self-contained: a reader with no prior knowledge of Vesper can implement it end to end.
- Every ExecPlan is a living document. Revise it as work progresses and record each change of course in the Decision Log.
- Every ExecPlan produces demonstrably working behaviour, with evidence, not just code that meets a definition.
- Every ExecPlan names the guides to consult and the sensors to run. For Vesper that means at minimum the relevant
  spec sections, the EGM contract README of each touched directory, and the commands in
  [TESTING.md](TESTING.md#definition-of-done).
- Define each term of art in plain language or avoid it. Use full repository-relative paths and exact symbol names.
- Anchor acceptance on observable outcomes: a request and its envelope, a test that fails before and passes after,
  an artifact under `run/` or `.symphony/runs/`.
- Steps are idempotent and safe; risky steps state the rollback.

## Formatting

Plain Markdown, no front matter, no surrounding code fence. Commands, transcripts and diffs as indented blocks.
Prose over lists, except in Progress, where a checklist is mandatory.

## Milestones

Milestones tell the story of the work: goal, work, result, proof. Each is independently verifiable and moves the goal
forward. Prototyping milestones are encouraged when they reduce risk; label them and say when they are promoted or
discarded. Progress tracks the granular work; a plan needs both.

## Skeleton

    # <Short, action-oriented description>

    This ExecPlan is a living document. The sections Progress, Surprises & Discoveries, Decision Log, and
    Outcomes & Retrospective must be kept up to date as work proceeds. It is maintained in accordance with
    docs/PLANS.md.

    ## Purpose / Big Picture

    What someone gains after this change and how they can see it working. Name the GitHub issue and parity unit.

    ## Progress

    - [x] (2026-01-01 13:00Z) Example completed step.
    - [ ] Example incomplete step.

    ## Surprises & Discoveries

    - Observation: …
      Evidence: …

    ## Decision Log

    - Decision: …
      Rationale: …
      Date/Author: …

    ## Outcomes & Retrospective

    Outcomes, gaps and lessons learned, compared against the original purpose. Follow-up debt or split issues.

    ## Context and Orientation

    The current state relevant to this task, as if the reader knows nothing. Key files by full path; terms defined.
    Guides to consult (spec sections, process docs, EGM contract READMEs).

    ## Plan of Work

    The sequence of edits in prose: which file, which function, what changes. Smallest verifiable slices.

    ## Concrete Steps

    Exact commands with working directory, and short expected transcripts.

    ## Validation and Acceptance

    Sensors to run (targeted tests, contract and matrix checks, docs checks) and what to observe; runtime artifacts
    that must exist. Exit criteria.

    ## Idempotence and Recovery

    Which steps are safe to repeat; how to retry or roll back risky ones.

    ## Artifacts and Notes

    The most important transcripts, diffs or snippets, as indented examples.

    ## Interfaces and Dependencies

    Modules and binaries to use, and the types and signatures that must exist at the end.

## Why this matters

Agents are only as effective as the repo-local context they can discover. A versioned plan can be read by future
agent runs, reviewed in pull requests, cross-linked from docs and issues, validated by doc checks, and archived
alongside the code it shaped.
