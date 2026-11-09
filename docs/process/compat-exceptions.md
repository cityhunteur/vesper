---
status: verified
owner: cityhunteur
last_reviewed: 2026-11-08
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

Optional: `description`, and the approval pair `approved_by` (a human, per AGENTS.md "Ask a human first") and
`approved_on` (YYYY-MM-DD, not in the future). The pair must be set together; `compatexctl` reports how many open
exceptions carry it as `approved=`. An exception without the pair is registered but not yet accepted as a pre-GA
deviation, so an issue whose acceptance criteria need "approved pre-GA compat exceptions" stays open. See
`src/compat.rs`.

## Registered exceptions

Three open exceptions back the GH #21 parity baseline (WOR-26, 2026-11-06), all owned by cityhunteur with expiry
2027-03-31 and target release `parity-ga`: CE-0001 (the upstream Firecracker pytest suite is not run; an API
conformance subset stands in), CE-0002 (the guest runs the v1.16.0 release, not a build of the pinned commit;
TD-029) and CE-0003 (performance is captured under nested virtualization with no Linux baseline on the same host
class). All three were approved by cityhunteur on 2026-11-08 (`approved_by`/`approved_on`), which satisfied the
GH #21 criterion "tracked as approved pre-GA compat exceptions" and closed WOR-26 and the Phase 2 epic WOR-14.
They are summarised in `docs/specs/backend-a-parity-baseline.json`; the `Parity Baseline` workflow fails if the
capture and the registry disagree about which ids are open.

A fourth open exception, CE-0004 (WOR-32 / GH #23, owned by cityhunteur, expiry 2027-03-31, target `parity-ga`),
records that guest exec deliberately overrides a workload's requested in-guest security posture: it always grants
the full capability set, forces no-new-privileges off and drops AppArmor/SELinux labels, because the microVM is
the isolation boundary (ADR 0006). Unlike CE-0001..CE-0003 it is not yet approved (`approved_by`/`approved_on`
are unset), so it needs a human's acceptance before GA. It is also listed in the parity baseline capture so the
`Parity Baseline` workflow's registry cross-check stays green.

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
