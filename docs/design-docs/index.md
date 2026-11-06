---
status: verified
owner: cityhunteur
last_reviewed: 2026-11-06
---

# Design documents

This is the catalogue of engineering designs: what each design covers, whether it is still true, and where it is
implemented. Designs explain *how* something works or will work. [ADRs](adr/index.md) record *why* a single decision
was made. [ExecPlans](../exec-plans/index.md) record *how the work was carried out*. Documents are linked where
they live; the normative ones stay under `docs/specs/`.

## Catalogue

| Design | Scope | Status | Domain | Implementation | ADRs | Verification | Plans |
|---|---|---|---|---|---|---|---|
| [design-v0.md](../specs/design-v0.md) | Normative product and architecture design: requirements, backends, phases, verification plan | draft (normative, maintained by hand) | all | Phase 0 in `src/`; Phase 1 image and boot pipeline (`src/systemvm.rs`, `launcher/`), no guest workload yet (TD-005); phases 2 to 5 not started | none (candidates listed in the ADR index) | milestone reports reference it; not re-verified line by line | [milestone log](../progress/symphony-milestones.md) |
| [firecracker-feature-parity.md](../specs/firecracker-feature-parity.md) | Route-level and field-level parity matrix plus the issue audit | draft (normative, maintained by hand) | control plane | `src/api.rs`, `src/api/`, `docs/generated/*.json` | none | updated per accepted issue; checked against the behavior matrix by review | [milestone log](../progress/symphony-milestones.md) |
| [Core beliefs](core-beliefs.md) | Engineering principles for the whole repository | verified | all | harness sensors and docs | n/a | 2026-11-05 | n/a |
| [harness-engineering.md](../process/harness-engineering.md) | How guides, sensors, bundles and the tracker protocol fit together | verified | process | `scripts/harness/`, `harness/`, `.symphony/templates/`, `WORKFLOW.md` | none | 2026-11-05 | n/a |
| [systemvm-bringup.md](../process/systemvm-bringup.md) | Pinned System VM image, overlay and packages, launch config, launcher, boot smoke and the Phase 0 vsock bridge | verified | runtime | `src/systemvm.rs`, `src/systemvm/`, `launcher/`, `systemvm/overlay/`, `src/proxy.rs`, `scripts/systemvm/`, `scripts/macos/build-vmlauncher.sh` | [0001](adr/0001-systemvm-launcher-swift-helper.md), [0002](adr/0002-systemvm-overlay-via-initramfs.md) | 2026-11-06 | [WOR-27 plan](../exec-plans/completed/2026-11-06-WOR-27-systemvm-image-pipeline.md), [WOR-25 plan](../exec-plans/active/2026-11-06-WOR-25-systemvm-control-channel.md) |
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
