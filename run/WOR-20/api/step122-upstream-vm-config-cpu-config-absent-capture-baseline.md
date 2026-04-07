# Step 122 upstream vm-config cpu-config absent capture baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: preboot `GET /vm/config` cpu-config omission capture registration

## Source packet

- `run/WOR-20/api/step24-upstream-cpu-config-export-baseline.md`
  - documents that successful preboot `PUT /cpu-config` stores CPU configuration for later use but `/vm/config` still omits the `cpu-config` field
- `tests/api_contract.rs`
  - `cpu_config_put_matches_pinned_store_behavior_without_vm_config_export` already proves the full-router preboot store plus `/vm/config` omission path

## Derived baseline facts

- The repo already carries pinned upstream evidence plus a full-router proof that `/vm/config` omits `cpu-config` after successful preboot `PUT /cpu-config`.
- The remaining harness gap is that the behavior-matrix registry could not encode field absence, so this pinned omission case could not be represented directly.

## Local parity decision

- Keep this slice harness-first: add explicit missing-field assertion support to the behavior matrix, register the preboot cpu-config omission case for `GET /vm/config`, regenerate the matrix, and preserve the evidence bundle without changing handler logic.
