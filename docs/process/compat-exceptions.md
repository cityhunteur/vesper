---
status: verified
owner: cityhunteur
last_reviewed: 2026-11-06
---

# Compat Exception Lifecycle

Compatibility exceptions are allowed only before parity-GA. They must be tracked in
`docs/specs/compat-exceptions.yaml`.

## Registry shape

Top level: `version` (non-zero integer) and `exceptions`. Required fields per exception (all non-blank):

- `id`
- `owner`
- `issue` (positive integer)
- `expiry_date` (YYYY-MM-DD)
- `target_release`
- `status` (`open` or `closed`, matched case-insensitively after trimming)

Optional: `description`. See `src/compat.rs`.

## Policy checks

- New exceptions without required fields are rejected.
- Open exceptions past `expiry_date` fail CI. The `Compat Exceptions Policy` workflow runs only when the registry,
  `src/compat.rs`, `src/bin/compatexctl.rs` or `Cargo.toml` change; there is no schedule, so an exception that expires
  by calendar time is caught on the next such change (TD-016).
- Parity-GA can enforce zero open exceptions with:
  `cargo run --bin compatexctl -- --registry docs/specs/compat-exceptions.yaml --require-zero-open`

## Usage

- Validate during development:
  `cargo run --bin compatexctl -- --registry docs/specs/compat-exceptions.yaml`
- Reproduce the expiry check for a given date:
  `cargo run --bin compatexctl -- --registry docs/specs/compat-exceptions.yaml --today 2026-12-31`
- In CI the zero-open gate is a `workflow_dispatch` input (`require_zero_open`) on the same workflow.
- Force parity-GA gate locally:
  `cargo run --bin compatexctl -- --registry docs/specs/compat-exceptions.yaml --require-zero-open`
