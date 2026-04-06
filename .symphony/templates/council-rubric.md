# LLM Council Rubric

Use this rubric after an issue has a `keep` verification result.

## Shared input packet

Every council member receives:
- the issue description,
- the mapped GitHub issue and PR,
- the diff or commit range,
- `verification.yaml`,
- `notes.md`,
- the relevant Vesper design sections,
- the relevant parity matrix section,
- the exact commands and evidence files.

Council members review read-only. They do not change code.

## Roles

### 1. parity-judge

Focus:
- observable Firecracker contract,
- state-machine correctness,
- parity matrix impact,
- whether the issue is truly closable.

Reject when:
- behavior still differs materially from the intended parity contract,
- evidence proves reachability but not parity,
- checklist claims exceed the code and evidence.

### 2. runtime-judge

Focus:
- process lifecycle,
- transport correctness,
- system VM safety,
- file or socket ownership,
- deterministic failure modes,
- cleanup and resource lifetime.

Reject when:
- the change can deadlock, leak, misroute, or leave inconsistent state,
- runtime evidence is missing for a runtime-sensitive change.

### 3. regression-judge

Focus:
- test precision,
- whether the same bug would be caught again,
- whether broad regression coverage is appropriate,
- whether docs/structure/quality sensors also needed updates.

Reject when:
- no targeted regression exists,
- broad regressions are missing for shared-path change,
- the harness can still pass while the bug survives.

### 4. release-judge

Focus:
- truthful project state,
- whether the issue should advance, split, reopen, or stay blocked,
- whether milestone logging is reviewer-ready.

Reject when:
- tracker state would outrun evidence,
- the slice is too large or mixed,
- milestone or verification reporting is too weak for handoff.

## Output schema

Each member writes a markdown file:

```md
# Council Verdict
- role: parity-judge
- verdict: approve | approve-with-gaps | reject
- evidence reviewed:
  - docs/specs/design-v0.md
  - docs/specs/firecracker-feature-parity.md
  - .symphony/runs/VSP-123/current/verification.yaml
- blocking findings:
  - none
- non-blocking gaps:
  - add a stronger regression around X
- suggested next state: Council | Human Review | Rework
```

## Council gate

Promote the issue only when:
- no member rejects,
- every blocking finding is resolved or the issue moves to `Rework`,
- the suggested next state converges on `Human Review` or a documented equivalent.

When verdicts disagree, prefer the most conservative state and update the issue with the disagreement summary.
