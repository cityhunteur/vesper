# Design Docs Index

This directory holds agent-facing design context and beliefs that sit above individual issue slices.

## Documents

- [`core-beliefs.md`](core-beliefs.md) — the agent-first principles for Vesper
- [`../specs/design-v0.md`](../specs/design-v0.md) — normative product and architecture design
- [`../specs/firecracker-feature-parity.md`](../specs/firecracker-feature-parity.md) — parity matrix and issue audit
- [`../process/harness-engineering.md`](../process/harness-engineering.md) — harness implementation guide

## Verification status

| Document | Role | Verification status |
| --- | --- | --- |
| `core-beliefs.md` | Stable guidance | Checked by `scripts/harness/check_docs.py` |
| `../specs/design-v0.md` | Normative | Maintained manually; referenced by milestone reports |
| `../specs/firecracker-feature-parity.md` | Normative parity matrix | Maintained manually; should align with milestone reports |
| `../process/harness-engineering.md` | Operational design | Checked by `scripts/harness/check_docs.py` |
