# Step 19 upstream drive root-order baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: exported drive ordering in `/vm/config` for preboot `PUT /drives/{drive_id}`

## Source packet

- `src/vmm/src/vmm_config/drive.rs`
  - `BlockBuilder.devices` documents that the root block device is stored first in the list.
  - `BlockBuilder::insert()` pushes new root devices to the front and swaps updated roots into index `0`.
  - Upstream tests `test_add_root_block_device_first()` and `test_root_block_device_add_last()` explicitly assert that the root block device stays at position `0` regardless of insertion order.
  - `BlockBuilder::configs()` re-serializes that ordered device list into exported config.

## Derived baseline fact

- `/vm/config` should export the root block device first in the `drives` array even when a non-root drive was configured earlier.

## Local parity decision

- This slice can truthfully tighten the control-plane stub by reordering exported drives so the root block device is serialized first, without overclaiming full upstream insertion-order behavior for the remaining non-root drives.
