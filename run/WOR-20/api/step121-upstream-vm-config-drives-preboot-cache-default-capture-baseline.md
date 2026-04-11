# Step 121 upstream vm-config drives preboot cache-default capture baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: preboot `GET /vm/config` drive default-cache export capture registration

## Source packet

- `run/WOR-20/api/step18-upstream-drive-cache-default-baseline.md`
  - documents that omitting `cache_type` on preboot `PUT /drives/{drive_id}` should still store and re-export `cache_type = "Unsafe"`
- `tests/api_contract.rs`
  - `drive_put_defaults_cache_type_to_unsafe` already proves the full-router preboot store plus `/vm/config` export path for the defaulted cache type

## Derived baseline facts

- The repo already carries pinned upstream evidence plus a full-router proof for the preboot `/vm/config` drive default-cache export surface.
- The remaining gap is only the missing generated behavior-matrix capture entry for that already-proven `cache_type = "Unsafe"` export shape.

## Local parity decision

- Keep this slice harness-only: register the missing preboot drive default-cache case for `GET /vm/config`, regenerate the matrix, and preserve the evidence bundle without changing handler logic.
