---
status: verified
owner: cityhunteur
last_reviewed: 2026-11-06
---

# Product specifications

This section says where Vesper's requirements live and which source is authoritative for what. Vesper's product
surface is the Firecracker API itself, so the requirements are the pinned upstream contract plus the design-v0 hard
requirements. No business rules are inferred here; write a local spec only for knowledge agents cannot reach.

## Specifications

| Spec | Domain / journey | Status | Implementing code | Tests |
|---|---|---|---|---|
| [design-v0.md § Requirements](../specs/design-v0.md#requirements) | Hard requirements, platform constraints, phases 0 to 5 | draft (normative) | `src/` (Phase 0 only) | `tests/`, `src/api/tests.rs` |
| [firecracker-feature-parity.md](../specs/firecracker-feature-parity.md) | Per-route and per-field parity status, issue audit | draft (normative) | `src/api.rs`, `src/api/` | `tests/api_contract.rs`, generated matrices |
| [Spec template](TEMPLATE.md) | n/a | n/a | n/a | n/a |

## External sources of truth

| Source | Authoritative for | Access |
|---|---|---|
| `third_party/firecracker/firecracker.yaml` | The exact route set, request and response schemas Vesper must serve | in repo; pinned revision in [design-v0.baselines.lock.yaml](../specs/design-v0.baselines.lock.yaml) |
| Firecracker upstream at the pinned revision (`docs/design.md`, `SPECIFICATION.md`, snapshot docs) | Expected status codes and error messages captured in `docs/specs/firecracker-api-*-captures.yaml` | public GitHub, revision in the lock file |
| [docs/specs/compat-exceptions.yaml](../specs/compat-exceptions.yaml) | Every allowed deviation from upstream behaviour before GA | in repo |
| GitHub issues `cityhunteur/vesper` #1 to #41 | Issue-level acceptance and the eval audit in the parity matrix; the per-phase parity checklist is appended to #1 to #40 by `scripts/update-issue-checklists.sh` | `gh issue view <n>` |
| Linear project (tickets `WOR-*`) | Planning state only, never evidence ([harness-engineering.md](../process/harness-engineering.md)) | **Unknown**: access is outside the repository |
| containerd and Apple Containerization at pinned revisions | Future shim v2 and native backend contracts (phases 3 and 5) | revisions in the lock file; no code yet |
