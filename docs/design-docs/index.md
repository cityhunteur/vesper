---
status: verified
owner: cityhunteur
last_reviewed: 2026-11-05
---

# Design documents

This is the catalogue of engineering designs: what each design covers, whether it is still true, and where it is
implemented. Designs explain *how* something works or will work. [ADRs](adr/index.md) record *why* a single decision
was made. [ExecPlans](../exec-plans/index.md) record *how the work was carried out*. Documents are linked where
they live; the normative ones stay under `docs/specs/`.

## Catalogue

| Design | Scope | Status | Domain | Implementation | ADRs | Verification | Plans |
|---|---|---|---|---|---|---|---|
| [design-v0.md](../specs/design-v0.md) | Normative product and architecture design: requirements, backends, phases, verification plan | draft (normative, maintained by hand) | all | Phase 0 scaffolding in `src/`; later phases not started | none (candidates listed in the ADR index) | milestone reports reference it; not re-verified line by line | [milestone log](../progress/symphony-milestones.md) |
| [firecracker-feature-parity.md](../specs/firecracker-feature-parity.md) | Route-level and field-level parity matrix plus the issue audit | draft (normative, maintained by hand) | control plane | `src/api.rs`, `src/api/`, `docs/generated/*.json` | none | updated per accepted issue; checked against the behavior matrix by review | [milestone log](../progress/symphony-milestones.md) |
| [Core beliefs](core-beliefs.md) | Engineering principles for the whole repository | verified | all | harness sensors and docs | n/a | 2026-11-05 | n/a |
| [harness-engineering.md](../process/harness-engineering.md) | How guides, sensors, bundles and the tracker protocol fit together | verified | process | `scripts/harness/`, `harness/`, `.symphony/templates/`, `WORKFLOW.md` | none | 2026-11-05 | n/a |
| [systemvm-bringup.md](../process/systemvm-bringup.md) | System VM manifest, launch config, boot smoke and the Phase 0 vsock bridge | verified | runtime | `src/systemvm.rs`, `src/proxy.rs`, `scripts/systemvm/` | none | 2026-11-05 | n/a |
| [ADR index](adr/index.md) | Architecture decision records and candidates | verified | all | n/a | n/a | n/a | n/a |
| [Design doc template](TEMPLATE.md) | Starting point for new designs | n/a | n/a | n/a | n/a | n/a | n/a |

The two `docs/specs/` documents are the source of truth for parity claims
([CLAUDE.md](../../CLAUDE.md)); every parity claim must cite the revision in
[design-v0.baselines.lock.yaml](../specs/design-v0.baselines.lock.yaml).

## Adding a design

1. Copy [TEMPLATE.md](TEMPLATE.md) to `docs/design-docs/<short-name>.md`.
2. Add a row above with status `draft`.
3. When the design is implemented and checked against the code, set status `verified` and fill in Verification.
4. When code moves away from the design, either update the design or mark it `stale` and add a tech-debt entry.
