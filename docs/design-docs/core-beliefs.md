---
status: verified
owner: cityhunteur
last_reviewed: 2026-11-05
---

# Core beliefs

These are the engineering principles this repository follows. Agents use them to break ties when no spec, design or
ADR covers a decision. Each belief says how it is enforced; a belief nothing enforces will drift. They translate
agent-first harness engineering into Vesper's parity-sensitive runtime environment.

### Humans steer, agents execute

**Verified.** Humans decide goals, priorities and acceptance criteria; the harness exists so agents can execute safely
and repeatably. *Enforced by:* the human-review and council stages in [WORKFLOW.md](../../WORKFLOW.md) and the
"ask a human first" list in [AGENTS.md](../../AGENTS.md).

### The repository is the system of record

**Verified.** If an agent cannot find it in the repo, it effectively does not exist. *Enforced by:*
`harness/check_docs.py` and `scripts/harness/check_docs.py` in CI; verification bundles under `.symphony/runs/`.

### `AGENTS.md` is a table of contents

**Verified.** Keep it short; put deep context in linked docs. *Enforced by:* the 100/150-line budget in
`harness/config.toml` and the 160-line warning in `scripts/harness/check_docs.py`.

### A small proven slice beats a large optimistic claim

**Verified.** Each issue maps to one smallest verifiable parity or harness unit, committed and pushed on its own.
*Enforced by:* the required workflow in [CLAUDE.md](../../CLAUDE.md) and
[harness-engineering.md](../process/harness-engineering.md); council rubric in `.symphony/templates/council-rubric.md`.

### A reachable route is not parity

**Verified** as policy; **Proposed** as a mechanical check. Route presence without correct state behaviour and error
semantics is incomplete, and `501 Not Implemented` is never parity. *Enforced by:* review against the
[parity matrix](../specs/firecracker-feature-parity.md); `scripts/harness/janitor_scan.py` only warns on routes
without explicit handlers (tech debt TD-003).

### Runtime claims need runtime evidence

**Verified** as policy. Proxy and System VM work must leave logs, reports or other durable artifacts.
*Enforced by:* [RELIABILITY.md](../RELIABILITY.md) artifact classes, `scripts/harness/collect_verification_bundle.sh`,
and the verification manifest template.

### Enforce invariants, not implementations

**Verified.** Dependency direction and size budgets are checked mechanically; how each module is written is not
prescribed. *Enforced by:* the `RULES` allowlist and budgets in `scripts/harness/check_structure.py`
([ARCHITECTURE.md](../../ARCHITECTURE.md#invariants)).

### Parse at the boundary

**Verified** per module. Request bodies, the baseline lock, compat registry, proxy targets and launch config are
parsed into typed shapes at the edge (`serde` structs, `proxy::parse_target`, `baseline::load`, `compat::load`).
*Enforced by:* unit tests in each module; no cross-cutting lint (tech debt TD-008).

### Prefer boring, inspectable dependencies

**Verified.** The crate depends on axum, clap, serde, tokio and a few utility crates; harness tooling is Python
standard library only. A tiny repo-local abstraction is preferred over an opaque helper package.
*Enforced by:* dependency review per [SECURITY.md](../SECURITY.md); new runtime dependencies need a human.

### When the agent struggles, upgrade the harness

**Verified.** Missing docs, scripts, structure checks or artifact capture are fixed directly rather than retried.
*Enforced by:* the missing-capability protocol in [harness-engineering.md](../process/harness-engineering.md) and the
[tech-debt tracker](../exec-plans/tech-debt-tracker.md).

### Human taste should compound

**Verified.** Repeated review comments belong in docs, templates, scripts or workflows. *Enforced by:* the council
rubric and the "promote repeated feedback" rule in [ARCHITECTURE.md](../../ARCHITECTURE.md).

### Continuous garbage collection beats debt clean-up marathons

**Verified.** Drift is scanned on a cadence and small fix-ups land continuously. *Enforced by:*
`.github/workflows/harness-garbage-collection.yml` (janitor scan), `harness-quality-score.yml`, the
[entropy sweep](../../harness/prompts/entropy-sweep.md) and [doc-gardening](../../harness/prompts/doc-gardening.md) prompts.
