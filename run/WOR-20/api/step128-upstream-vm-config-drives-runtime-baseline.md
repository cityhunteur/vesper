# Step 128 upstream vm-config drives runtime baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: `GET /vm/config` drive export surface after a successful runtime drive patch

## Source packet

- `run/WOR-20/api/step14-upstream-drive-update-baseline.md`
  - `PATCH /drives/{drive_id}` is runtime-only.
  - accepted runtime patches update the live block device path and/or rate limiter for an existing drive.
- `run/WOR-20/api/step17-upstream-drive-create-baseline.md`
  - replacing an existing drive config by the same id is allowed and `/vm/config` reflects the replaced drive path.
- `src/api.rs`
  - full VM config export serializes stored drive state without a boot-state-specific branch.

## Derived baseline facts

- `GET /vm/config` should remain callable after boot after a successful runtime drive patch.
- The booted full-config export should preserve the stored drive identity while reflecting the updated runtime `path_on_host`.

## Local parity decision

- This slice is harness-first, not a handler change: Vesper already proves the runtime drive patch/export path through the full router, but the generated route-level behavior matrix still only recorded preboot drive export facts.
