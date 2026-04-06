# Vesper Docs Index

This `docs/` tree is the repository-local system of record for Vesper. Agents should start here and progressively disclose only the documents needed for the current task.

## Start here

- [`../AGENTS.md`](../AGENTS.md) — short repository map
- [`ARCHITECTURE.md`](ARCHITECTURE.md) — layer model, invariants, and refactor boundaries
- [`process/harness-engineering.md`](process/harness-engineering.md) — how Vesper uses harness engineering in practice

## Design and parity

- [`specs/design-v0.md`](specs/design-v0.md) — normative design, phases, verification plan
- [`specs/firecracker-feature-parity.md`](specs/firecracker-feature-parity.md) — current parity matrix and issue audit
- [`design/index.md`](design/index.md) — design document map and beliefs

## Quality, reliability, and security

- [`QUALITY_SCORE.md`](QUALITY_SCORE.md) — current quality dimensions, grades, and debt anchors
- [`RELIABILITY.md`](RELIABILITY.md) — artifact capture, runtime proofs, and legibility expectations
- [`SECURITY.md`](SECURITY.md) — trust boundaries and review rules for security-sensitive work

## Plans and progress

- [`PLANS.md`](PLANS.md) — how to write and maintain execution plans
- [`plans/active/README.md`](plans/active/README.md) — active plan template
- [`plans/completed/README.md`](plans/completed/README.md) — completed plan archive policy
- [`plans/tech-debt-tracker.md`](plans/tech-debt-tracker.md) — durable debt inventory
- [`progress/symphony-milestones.md`](progress/symphony-milestones.md) — accepted-issue milestone log

## Process docs already in the repo

- [`process/baseline-governance.md`](process/baseline-governance.md)
- [`process/compat-exceptions.md`](process/compat-exceptions.md)
- [`process/macos-packaging.md`](process/macos-packaging.md)
- [`process/systemvm-bringup.md`](process/systemvm-bringup.md)

## Generated and external references

- [`generated/README.md`](generated/README.md) — generated documents and ownership
- [`references/README.md`](references/README.md) — external references and notes

## Reading heuristics

- For API parity work, read `design-v0.md`, `firecracker-feature-parity.md`, `ARCHITECTURE.md`, and `RELIABILITY.md`.
- For system VM or proxy work, read `systemvm-bringup.md`, `RELIABILITY.md`, and `SECURITY.md`.
- For governance changes, read `baseline-governance.md`, `compat-exceptions.md`, `SECURITY.md`, and `QUALITY_SCORE.md`.
- For harness changes, read `process/harness-engineering.md`, `ARCHITECTURE.md`, and `PLANS.md`.
