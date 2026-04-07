# Baseline Governance

This document defines the required workflow for updating `docs/specs/design-v0.baselines.lock.yaml`.

## Required PR steps

1. Update `docs/specs/design-v0.baselines.lock.yaml` with new pinned revisions.
2. Regenerate parity metadata with:
   `cargo run --bin baselinectl -- --mode generate --backend <backend> --host-os <os> --host-cpu <cpu> --metadata artifacts/parity-metadata.json`
3. Validate metadata with:
   `cargo run --bin baselinectl -- --mode validate --metadata artifacts/parity-metadata.json`
4. Update `docs/specs/firecracker-api-baseline-captures.yaml` for any route whose current matrix-backed placeholder behavior has newly pinned upstream evidence.
   Routes handled by concrete code should remain `not_applicable` in the generated matrix instead of being added to the capture registry.
5. Regenerate the pinned API surface stub matrix with:
   `cargo run --bin baselinectl -- --mode generate-api-matrix --matrix docs/generated/firecracker-api-surface-stub-matrix.json --captures docs/specs/firecracker-api-baseline-captures.yaml`
6. Validate the API surface stub matrix with:
   `cargo run --bin baselinectl -- --mode validate-api-matrix --matrix docs/generated/firecracker-api-surface-stub-matrix.json --captures docs/specs/firecracker-api-baseline-captures.yaml`
7. Update `docs/specs/firecracker-api-behavior-captures.yaml` for any concrete-handler or matrix-backed placeholder route whose current status/error behavior now has pinned upstream evidence.
8. Regenerate the route-level API behavior matrix with:
   `cargo run --bin baselinectl -- --mode generate-api-behavior-matrix --behavior-matrix docs/generated/firecracker-api-behavior-matrix.json --behavior-captures docs/specs/firecracker-api-behavior-captures.yaml`
9. Validate the route-level API behavior matrix with:
   `cargo run --bin baselinectl -- --mode validate-api-behavior-matrix --behavior-matrix docs/generated/firecracker-api-behavior-matrix.json --behavior-captures docs/specs/firecracker-api-behavior-captures.yaml`
10. Attach parity/regression evidence that references the exact baseline revision IDs.

## CI enforcement

The `Baseline Governance` GitHub Actions workflow enforces:

- lock file readability/validity
- required metadata key/value presence
- generated API surface stub matrix coverage/consistency against the pinned Firecracker route set, placeholder-path applicability, and checked-in capture registry
- generated API behavior matrix coverage/consistency against the pinned Firecracker route set and checked-in route-behavior capture registry
- machine-readable parity metadata artifact publication

PRs that fail metadata validation are blocked.
