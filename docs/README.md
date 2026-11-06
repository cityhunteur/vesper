---
status: verified
owner: cityhunteur
last_reviewed: 2026-11-05
---

# Documentation

This directory is the repository's system of record: anything an agent or a new engineer needs that the code alone
does not say. [AGENTS.md](../AGENTS.md) is the entry point. This page is the full index.

Vesper is a Rust crate that provides a Firecracker-compatible control plane on macOS. Its domains are the API state
machine and deterministic error contract, parity governance against pinned upstream baselines, socket proxying, and
System VM bring-up. It runs today as a local macOS dev loop; packaged deployment is designed but not implemented.

## Map

| Area | Document | Use it for |
|---|---|---|
| Architecture | [ARCHITECTURE.md](../ARCHITECTURE.md) | Layers, dependency rules, flows, invariants |
| Specs | [specs/](specs/index.md) | Normative design, parity matrix, baseline lock, compat registry, captures |
| Design | [design-docs/](design-docs/index.md) | Catalogue of designs, with their status and verification |
| Design | [core-beliefs.md](design-docs/core-beliefs.md) | Engineering principles the repository follows |
| Decisions | [design-docs/adr/](design-docs/adr/index.md) | Architecture decision records and candidates |
| Product | [product-specs/](product-specs/index.md) | Where requirements live and which source is authoritative |
| Process | [process/](process/index.md) | Harness engineering, baseline governance, compat lifecycle, packaging, System VM |
| Planning | [PLANS.md](PLANS.md) | How to write and maintain an ExecPlan |
| Planning | [exec-plans/](exec-plans/index.md) | Active and completed plans, tech-debt tracker |
| Progress | [progress/](progress/index.md) | Symphony milestone log of accepted issues |
| History | [devlog.md](devlog.md) | Dated development log (Rust rewrite, eval orchestration) |
| Development | [DEVELOPMENT.md](DEVELOPMENT.md) | Toolchain, build, run, code generation |
| Testing | [TESTING.md](TESTING.md) | Test inventory, commands, definition of done |
| Operations | [OBSERVABILITY.md](OBSERVABILITY.md) | What the binaries emit and the agent diagnosis loop |
| Operations | [RELIABILITY.md](RELIABILITY.md) | Required runtime artifacts and stage-gated expectations |
| Security | [SECURITY.md](SECURITY.md) | Trust boundaries, entitlements, review rules |
| Quality | [QUALITY_SCORE.md](QUALITY_SCORE.md) | Scoring model and current signals |
| Reference | [references/](references/index.md) | Historical bundle notes and third-party pointers |
| Generated | [generated/](generated/README.md) | Machine-generated matrices; never edit by hand |

## Ownership

Every subject has exactly one authoritative document. Other documents link to it instead of repeating it.
When code changes the behaviour a document describes, update that document in the same change.

| Subject | Authoritative document |
|---|---|
| Component boundaries, layering, dependency direction | [ARCHITECTURE.md](../ARCHITECTURE.md) |
| Product requirements, phases, verification plan | [specs/design-v0.md](specs/design-v0.md) |
| Per-route and per-field parity status | [specs/firecracker-feature-parity.md](specs/firecracker-feature-parity.md) |
| Pinned upstream revisions | [specs/design-v0.baselines.lock.yaml](specs/design-v0.baselines.lock.yaml) |
| Allowed behaviour deviations before GA | [specs/compat-exceptions.yaml](specs/compat-exceptions.yaml) |
| The pinned Firecracker API surface | `third_party/firecracker/firecracker.yaml` |
| Per-directory code contracts (interface, invariants, side effects) | The EGM `README.md` in each directory, rolled up in [../README.md](../README.md) |
| Why a decision was made | The relevant [ADR](design-docs/adr/index.md) |
| Commands and environment | [DEVELOPMENT.md](DEVELOPMENT.md) |
| Issue execution and tracker protocol | [process/harness-engineering.md](process/harness-engineering.md), [WORKFLOW.md](../WORKFLOW.md), [CLAUDE.md](../CLAUDE.md) |
| Known gaps and debt | [exec-plans/tech-debt-tracker.md](exec-plans/tech-debt-tracker.md) |

## Reading heuristics

- API parity work: `specs/design-v0.md`, `specs/firecracker-feature-parity.md`, `ARCHITECTURE.md`, `RELIABILITY.md`.
- System VM or proxy work: `process/systemvm-bringup.md`, `RELIABILITY.md`, `SECURITY.md`.
- Governance changes: `process/baseline-governance.md`, `process/compat-exceptions.md`, `SECURITY.md`, `QUALITY_SCORE.md`.
- Harness changes: `process/harness-engineering.md`, `ARCHITECTURE.md`, `PLANS.md`.

## Conventions

### Front matter

Each document under `docs/`, and `ARCHITECTURE.md`, starts with:

```yaml
---
status: draft          # template | draft | verified | stale | deprecated (ADRs: proposed | accepted | rejected | superseded | deprecated)
owner: cityhunteur     # who answers questions about it
last_reviewed: 2026-11-05
---
```

`verified` means someone checked every claim against the code on `last_reviewed`. The checker warns when a review is
older than 90 days. Exempt from front matter: ExecPlans, which stay pure plan documents, and `generated/`.

### Evidence labels

Documents separate what is true from what is hoped for:

- **Verified**: backed by code, configuration or tests. Cite the path.
- **Intended**: the agreed design, but not yet fully reflected in code.
- **Proposed**: a recommendation. It is not policy.
- **Unknown**: a gap. Say what is missing and how to resolve it.

### Template markers

`TODO(harness)` marks content still to be written for this repository. `python3 harness/check_docs.py --strict`
fails while any marker remains outside a `TEMPLATE.md` file.

### Indexes and links

- Every directory has an `index.md` (or `README.md`) that links every document and subdirectory in it.
- Every document is reachable from `AGENTS.md`. Relative links and `#anchors` must resolve.
- Link to code by repository-relative path, so the checker catches references to code that has moved or been deleted.
- Prefer a short document per subject over one long document.

## Keeping docs current

- **In every change:** update the owning document. Two checkers run in CI: `harness/check_docs.py`
  (`.github/workflows/docs.yml`) and the older `scripts/harness/check_docs.py` (`.github/workflows/harness-docs.yml`).
- **Recurring:** an agent runs [doc-gardening](../harness/prompts/doc-gardening.md) to find documents that no longer match
  the code, and [entropy sweep](../harness/prompts/entropy-sweep.md) to find code that no longer matches the documented
  principles. `scripts/harness/janitor_scan.py` covers broken links, missing route handlers and oversized files.
- **When an agent struggles:** treat it as a missing doc, tool or guardrail. Add it, and prefer a check over prose.
