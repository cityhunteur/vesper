# Step 123 upstream vm-config serial absent capture baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: preboot `GET /vm/config` serial omission capture registration

## Source packet

- `run/WOR-20/api/step25-upstream-serial-store-baseline.md`
  - documents that successful `PUT /serial` stores serial output configuration but `/vm/config` still omits the `serial` field
- `tests/api_contract.rs`
  - `serial_put_matches_pinned_store_behavior_without_vm_config_export` already proves the full-router preboot store plus `/vm/config` omission path

## Derived baseline facts

- The repo already carries pinned upstream evidence plus a full-router proof that `/vm/config` omits `serial` after successful preboot `PUT /serial`.
- With missing-field assertion support now present in the harness, the remaining gap is only the missing generated behavior-matrix capture entry for that already-proven omission.

## Local parity decision

- Keep this slice capture-only: register the preboot serial omission case for `GET /vm/config`, regenerate the matrix, and preserve the evidence bundle without changing handler logic.
