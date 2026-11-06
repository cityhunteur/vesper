# Entropy-sweep prompt: keep the code true to its principles

Run this on a schedule. Agents copy the patterns they find, including bad ones. Small, frequent cleanups stop drift
before it compounds. This prompt is the code half of continuous garbage collection. The documentation half is
[doc-gardening.md](doc-gardening.md).

---

You are paying down technical debt in this repository, a little at a time. Read `AGENTS.md`, `ARCHITECTURE.md`
(especially Invariants), `docs/design-docs/core-beliefs.md`, `docs/QUALITY_SCORE.md` and
`docs/exec-plans/tech-debt-tracker.md`.

1. Scan for deviations from the documented invariants and from the core beliefs marked **Verified**. Examples:
   dependencies that point the wrong way, hand-rolled helpers that duplicate a shared utility, unvalidated data at
   boundaries, unstructured logging, missing timeouts.
2. For each deviation, choose one of three actions:
   - **Small and mechanical:** fix it in a focused PR with its tests. Each PR should take a reviewer under a minute.
   - **Large:** add or update a tech-debt entry with evidence. If it is P0 or P1, propose an ExecPlan.
   - **Recurring:** propose a lint rule or structural test that would prevent it, and record it as a debt entry.
     Write the lint's error message as a fix instruction.
3. Update `docs/QUALITY_SCORE.md`: change grades only where you have new evidence, and add a History row.
4. Close tech-debt entries that are now resolved, citing the PR.

Don't change behaviour, public APIs or security-sensitive code in a sweep. Escalate those as debt entries. Keep each
PR to one kind of fix.
