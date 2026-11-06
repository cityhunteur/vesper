---
status: verified
owner: cityhunteur
last_reviewed: 2026-11-06
---

# Vesper Quality Score

This file tracks the quality dimensions that matter most for agent-driven work on Vesper. It is updated from the
sensor output, not intuition. Run:

```sh
python3 scripts/harness/compute_quality_score.py --root . --markdown-out artifacts/quality-score.md --json-out artifacts/quality-score.json
```

then copy the stable numbers here, summarise the deltas, and link the artifact from the milestone log.

## Scoring model

| Dimension | Weight | Goal | How it is measured |
|---|---:|---|---|
| Knowledge base and docs map | 20 | Agents reliably find the right source of truth without oversized prompt files | Presence of the nine `CORE_DOCS` listed in the sensor |
| Harness sensors | 25 | Computational and inferential checks catch regressions and drift early | Presence of the four `harness-*` workflows, six harness scripts and four Symphony templates |
| Parity surface coverage | 25 | Route coverage and behavior proofs move toward full Firecracker parity | Golden routes with an explicit dispatch arm in `src/api.rs` |
| Architectural coherence | 15 | Boundaries stay clear and hotspots shrink over time | `src/api.rs` line count against the 1200 / 2500 / 3200 budgets |
| Truthful delivery process | 15 | Tracker state and milestones match the real code and evidence | Absence of the retired optimistic promotion workflows |

## Current signals (sensor run on 2026-11-06)

| Dimension | Score | Signal |
|---|---:|---|
| Knowledge base and docs map | 20 / 20 | 9 / 9 core docs present |
| Harness sensors | 25 / 25 | 14 / 14 workflows, scripts and templates present |
| Parity surface coverage | 25 / 25 | 37 / 37 golden routes dispatched explicitly (behaviour-level parity is still unproven against a real VM; the parity matrix has no ticked rows) |
| Architectural coherence | 8 / 15 | `src/api.rs` is 3153 lines, above the 2500 warning budget and below the 3200 error budget |
| Truthful delivery process | 15 / 15 | no optimistic promotion workflows remain |
| **Total** | **93 / 100 (A)** | |

The sensor measures route dispatch presence and file presence; it does not measure behaviour parity, runtime evidence
or CI coverage. Read it together with the [parity matrix](specs/firecracker-feature-parity.md) and the
[tech-debt tracker](exec-plans/tech-debt-tracker.md).

## Target state

- Knowledge base and docs map: A
- Harness sensors: A-
- Parity surface coverage: A, with behaviour proofs per route family rather than dispatch presence
- Architectural coherence: B+ or better (`src/api.rs` under 2500 lines)
- Truthful delivery process: A

## Current debt anchors

- Finish splitting `src/api.rs`: `errors` and `vm_config` families remain inline (TD-001).
- Expand behaviour-level parity tests beyond dispatch presence and make a regression to a stub fail CI (TD-003).
- Add runtime evidence paths for System VM and proxy work (TD-004, TD-005).
- Add structured logging and request logging so bundles carry API and proxy evidence (TD-007, TD-011).
- Bring `src/api/tests.rs` under a line budget (TD-015).
