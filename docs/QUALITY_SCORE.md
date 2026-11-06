---
status: verified
owner: cityhunteur
last_reviewed: 2026-11-05
---

# Vesper Quality Score

This file tracks the quality dimensions that matter most for agent-driven work on Vesper. It should be updated from durable reports, not intuition alone.

## Scoring model

| Dimension | Weight | Goal |
| --- | ---: | --- |
| Knowledge base and docs map | 20 | Agents can reliably find the right source of truth without oversized prompt files. |
| Harness sensors | 25 | Computational and inferential checks catch regressions and drift early. |
| Parity surface coverage | 25 | Route coverage and behavior proofs move toward full Firecracker parity. |
| Architectural coherence | 15 | Boundaries stay clear and hotspots shrink over time. |
| Truthful delivery process | 15 | Tracker state and milestones match the real code and evidence. |

## Seed baseline for the current Vesper snapshot

This bundle was tailored to the current repository shape and starts with the following seed observations:

| Dimension | Current signal | Notes |
| --- | --- | --- |
| Knowledge base and docs map | Partial | Strong design specs exist, but the repo lacks a short map plus structured docs index. |
| Harness sensors | Mixed | Baseline, compat, and contract checks exist; structure, docs, quality, and janitor sensors are missing. |
| Parity surface coverage | Partial | Static scan shows 15 / 37 Firecracker routes with explicit behavior handling; many runtime-heavy endpoints remain incomplete. |
| Architectural coherence | Fair with hotspot | `src/api.rs` is ~2847 lines in the current snapshot and should be decomposed. |
| Truthful delivery process | Weak | Existing issue-promotion workflows can mark progress ahead of durable evidence. |

## Target state

- Knowledge base and docs map: A
- Harness sensors: A-
- Parity surface coverage: A
- Architectural coherence: B+ or better
- Truthful delivery process: A

## How to update this file

Use `python3 scripts/harness/compute_quality_score.py --root . --markdown-out artifacts/quality-score.md --json-out artifacts/quality-score.json`.

Then:
1. copy the stable metrics you want to preserve here,
2. summarize major deltas,
3. link the artifact in the milestone log.

## Current debt anchors

- Split `src/api.rs`.
- Replace optimistic issue-promotion automation with evidence-driven orchestration.
- Expand behavior-level parity tests beyond reachability.
- Add runtime evidence paths for System VM and proxy work.
- Close missing route behavior gaps from the parity matrix.
