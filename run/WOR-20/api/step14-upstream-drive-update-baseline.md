# Step 14 upstream drive update baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: preboot `PUT /drives/{drive_id}` path-id validation and runtime `PATCH /drives/{drive_id}` update behavior

## Source packet

- `src/firecracker/src/api_server/request/drive.rs`
  - `parse_put_drive()` rejects path/body id mismatches with `The id from the path does not match the id from the body!`.
  - `parse_patch_drive()` rejects path/body id mismatches with the same exact string.
  - `parse_patch_drive()` accepts `drive_id`-only bodies and supports updates for `path_on_host` and `rate_limiter`.
- `src/vmm/src/rpc_interface.rs`
  - `UpdateBlockDevice` is runtime-only.
  - runtime drive updates surface through `VmmActionError::DriveConfig`.
- `src/vmm/src/vmm_config/drive.rs`
  - `DriveError::DeviceUpdate` displays as `Unable to patch the block device: {0} Please verify the request arguments.`
- `src/vmm/src/device_manager/mod.rs`
  - `FindDeviceError::DeviceNotFound` displays as `Device not found`.
- `src/vmm/src/lib.rs`
  - runtime drive updates use the live block device path via `update_block_device()`.
  - path updates and rate-limiter updates both fail through the same device-update chain when the drive is missing.

## Derived runtime fault strings

- preboot `PUT /drives/{drive_id}` or runtime `PATCH /drives/{drive_id}` with a path/body id mismatch:
  - `The id from the path does not match the id from the body!`
- runtime `PATCH /drives/{drive_id}` with no configured drive:
  - `Drive config error: Unable to patch the block device: Failed perform action on device: Device not found Please verify the request arguments.`

## Baseline facts used for this slice

- `PUT /drives/{drive_id}` is preboot-only.
- `PATCH /drives/{drive_id}` is runtime-only.
- Runtime drive patching updates the live block device path and/or rate limiter for an existing drive.
- `PATCH /drives/{drive_id}` with only `drive_id` is a valid request shape upstream.

## Local parity decision

- This slice can truthfully tighten the control-plane stub by:
  - reusing the exact upstream path/body id mismatch string for the drive endpoints,
  - rejecting runtime drive patches on missing drives with the pinned device-update envelope,
  - preserving the existing low-risk stored-config update path for successful runtime path changes.
