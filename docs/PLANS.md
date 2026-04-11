# Vesper Plans

Execution plans are first-class repository artifacts.

## When to write a plan

Write a plan in `docs/plans/active/` when:
- the task spans multiple commits or issue slices,
- runtime evidence is required,
- the task mixes code and harness changes,
- the task touches architecture boundaries,
- the task is likely to be resumed across sessions or agents.

A small one-file fix may rely on the issue description alone, but any parity or runtime-sensitive change should usually have a plan.

## Plan shape

Each active plan should include:

1. Goal
2. Scope and exclusions
3. Smallest verifiable slices
4. Required guides to consult
5. Required sensors to run
6. Progress log
7. Decision log
8. Exit criteria
9. Follow-up debt or split issues

## Lifecycle

- Draft in `docs/plans/active/`
- Update progress as work lands
- Move to `docs/plans/completed/` when the goal is either complete or superseded
- Link follow-up debt into `docs/plans/tech-debt-tracker.md`

## Why this matters

Agents are only as effective as the repo-local context they can discover. A versioned plan is better than a chat summary because it can be:
- read by future agent runs,
- reviewed in pull requests,
- cross-linked from docs and issues,
- validated by doc checks,
- archived alongside the code it shaped.
