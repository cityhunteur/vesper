# Baseline Governance

This document defines the required workflow for updating `docs/specs/design-v0.baselines.lock.yaml`.

## Required PR steps

1. Update `docs/specs/design-v0.baselines.lock.yaml` with new pinned revisions.
2. Regenerate parity metadata with:
   `cargo run --bin baselinectl -- --mode generate --backend <backend> --host-os <os> --host-cpu <cpu> --metadata artifacts/parity-metadata.json`
3. Validate metadata with:
   `cargo run --bin baselinectl -- --mode validate --metadata artifacts/parity-metadata.json`
4. Attach parity/regression evidence that references the exact baseline revision IDs.

## CI enforcement

The `Baseline Governance` GitHub Actions workflow enforces:

- lock file readability/validity
- required metadata key/value presence
- machine-readable parity metadata artifact publication

PRs that fail metadata validation are blocked.
