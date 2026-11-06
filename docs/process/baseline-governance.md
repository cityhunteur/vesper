---
status: verified
owner: cityhunteur
last_reviewed: 2026-11-06
---

# Baseline Governance

This document defines the required workflow for updating `docs/specs/design-v0.baselines.lock.yaml`.

## Required PR steps

1. Update `docs/specs/design-v0.baselines.lock.yaml` with new pinned revisions.
2. Regenerate parity metadata with:
   `cargo run --bin baselinectl -- --mode generate --backend <backend> --host-os <os> --host-cpu <cpu> --metadata artifacts/parity-metadata.json`
3. Validate metadata with:
   `cargo run --bin baselinectl -- --mode validate --metadata artifacts/parity-metadata.json`
4. Update `docs/specs/firecracker-api-baseline-captures.yaml` for any route whose current matrix-backed placeholder behavior has newly pinned upstream evidence.
   Routes handled by concrete code should remain `not_applicable` in the generated matrix instead of being added to the capture registry; capture concrete route-family endpoint packets in issue-local `run/<ISSUE>/...` notes and keep `docs/specs/firecracker-api-behavior-captures.yaml` plus milestone references aligned instead.
5. Regenerate the pinned API surface stub matrix with `make surface-matrix-generate`, which runs:
   `cargo run --bin baselinectl -- --mode generate-api-matrix` (defaults: `--matrix docs/generated/firecracker-api-surface-stub-matrix.json --captures docs/specs/firecracker-api-baseline-captures.yaml --swagger third_party/firecracker/firecracker.yaml --lock docs/specs/design-v0.baselines.lock.yaml`)
6. Validate the API surface stub matrix with `make surface-matrix-check` (`--mode validate-api-matrix`, same defaults)
7. Update `docs/specs/firecracker-api-behavior-captures.yaml` for any concrete-handler or matrix-backed placeholder route whose current status/error behavior now has pinned upstream evidence.
8. Regenerate the route-level API behavior matrix with `make behavior-matrix-generate` (`--mode generate-api-behavior-matrix`; defaults `--behavior-matrix docs/generated/firecracker-api-behavior-matrix.json --behavior-captures docs/specs/firecracker-api-behavior-captures.yaml`)
9. Validate the route-level API behavior matrix with `make behavior-matrix-check` (`--mode validate-api-behavior-matrix`, same defaults)
10. Attach parity/regression evidence that references the exact baseline revision IDs.

## CI enforcement

The `Baseline Governance` workflow (`.github/workflows/baseline-governance.yml`, triggered by the lock file,
`src/baseline.rs`, `src/bin/baselinectl.rs` and `Cargo.toml`) runs `cargo test`, then `baselinectl --mode generate`
and `--mode validate`, and uploads `artifacts/parity-metadata.json`. It enforces lock file validity, required metadata
key presence, and metadata artifact publication.

The `Rust Tests` workflow (`.github/workflows/rust-tests.yml`, triggered by `src/**`, `tests/**`, `third_party/**`,
`docs/generated/**` and `docs/specs/*.yaml`) runs `cargo test`, `make surface-matrix-check` and
`make behavior-matrix-check`. It enforces generated surface stub matrix and behavior matrix coverage and consistency
against the pinned route set and the checked-in capture registries. The `validates_repo_generated_*` unit tests
inside `cargo test` check the same thing.

PRs that fail metadata validation or carry a stale matrix are blocked.
