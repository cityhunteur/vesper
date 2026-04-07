# Step 120 upstream vm-config drives preboot root-order capture baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: preboot `GET /vm/config` drive export ordering capture registration

## Source packet

- `run/WOR-20/api/step19-upstream-drive-root-order-baseline.md`
  - documents that Firecracker exports the root block device first in `/vm/config` even if a non-root drive was configured earlier
- `tests/api_contract.rs`
  - `drive_export_orders_root_device_first` already proves the full-router preboot store plus `/vm/config` export ordering path

## Derived baseline facts

- The repo already carries pinned upstream evidence plus a full-router proof for the preboot `/vm/config` drive export ordering surface.
- The remaining gap is only the missing generated behavior-matrix capture entry for that already-proven root-first `drives` array shape.

## Local parity decision

- Keep this slice harness-only: register the missing preboot drive export ordering case for `GET /vm/config`, regenerate the matrix, and preserve the evidence bundle without changing handler logic.
