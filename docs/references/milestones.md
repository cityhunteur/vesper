---
status: deprecated
owner: cityhunteur
last_reviewed: 2026-11-05
---

> Historical note from the 2026-03 harness-engineering bundle. Paths such as `docs/index.md`, `docs/design/` and `docs/plans/` refer to the layout that was replaced on 2026-11-05; see [index.md](index.md).

# Recommended Vesper milestones after harness-engineering adoption

## M0 — Truth reset and harness bootstrap

Purpose:
- restore issue truth,
- install the repo-local docs map,
- turn on structure/docs/quality/janitor sensors,
- start milestone logging.

Exit criteria:
- `AGENTS.md` becomes a short map,
- `docs/index.md`, `docs/ARCHITECTURE.md`, `docs/QUALITY_SCORE.md`, `docs/RELIABILITY.md`, `docs/SECURITY.md`, and `docs/PLANS.md` exist,
- `.symphony/templates/*` exist,
- new harness workflows run,
- optimistic status workflows are scheduled for retirement.

## M1 — Phase 0 behavior parity slices

Purpose:
- turn partial API support into behavior-proven slices rather than route reachability.

Exit criteria:
- each accepted issue leaves a verification bundle,
- route behavior proofs expand,
- parity matrix status matches milestone entries.

## M2 — Runtime legibility for proxy and System VM

Purpose:
- make runtime-sensitive work reproducible and inspectable.

Exit criteria:
- proxy logs and System VM artifacts are captured for relevant issues,
- the council can review runtime changes from durable artifacts.

## M3 — Backend A lifecycle truth

Purpose:
- use the harness to make Backend A claims evidence-backed.

Exit criteria:
- lifecycle and transport slices are backed by actual runtime evidence,
- compat exceptions are explicit where parity is still open.

## M4 — Shim v2 and runtime orchestration

Purpose:
- connect the runtime contract to real, reviewable evidence and smaller slices.

Exit criteria:
- accepted issues map to durable runtime and parity proofs,
- issue status no longer advances from merged code alone.

## M5 — Continuous garbage collection

Purpose:
- keep the repo coherent as more agent-driven work lands.

Exit criteria:
- janitor reports are reviewed on a cadence,
- quality score trend improves,
- hotspot and docs drift shrink instead of compounding.
