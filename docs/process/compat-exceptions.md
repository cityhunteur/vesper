---
status: verified
owner: cityhunteur
last_reviewed: 2026-11-05
---

# Compat Exception Lifecycle

Compatibility exceptions are allowed only before parity-GA. They must be tracked in
`docs/specs/compat-exceptions.yaml`.

## Required fields per exception

- `id`
- `owner`
- `issue`
- `expiry_date` (YYYY-MM-DD)
- `target_release`
- `status` (`open` or `closed`)

## Policy checks

- New exceptions without required fields are rejected.
- Open exceptions past `expiry_date` fail CI.
- Parity-GA can enforce zero open exceptions with:
  `cargo run --bin compatexctl -- --registry docs/specs/compat-exceptions.yaml --require-zero-open`

## Usage

- Validate during development:
  `cargo run --bin compatexctl -- --registry docs/specs/compat-exceptions.yaml`
- Force parity-GA gate locally:
  `cargo run --bin compatexctl -- --registry docs/specs/compat-exceptions.yaml --require-zero-open`
