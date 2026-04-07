# Step 17 upstream drive create baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: preboot `PUT /drives/{drive_id}` root-device conflict behavior

## Source packet

- `src/firecracker/src/api_server/request/drive.rs`
  - `parse_put_drive()` rejects path/body id mismatches with `The id from the path does not match the id from the body!`.
  - accepted requests route to `InsertBlockDevice`.
- `src/vmm/src/rpc_interface.rs`
  - `InsertBlockDevice` is preboot-only.
  - drive configuration failures surface through `VmmActionError::DriveConfig`, which formats as `Drive config error: {0}`.
- `src/vmm/src/vmm_config/drive.rs`
  - `DriveError::AddingSecondRootDevice` displays as `Attempt to add block as a root device while the root device defined as a pmem device`.
  - `DriveError::RootBlockDeviceAlreadyAdded` displays as `A root block device already exists!`.
  - replacing an existing drive by id is allowed.

## Derived preboot fault strings

- preboot `PUT /drives/{drive_id}` with `is_root_device=true` while a pmem root exists:
  - `Drive config error: Attempt to add block as a root device while the root device defined as a pmem device`
- preboot `PUT /drives/{drive_id}` with `is_root_device=true` while another block root exists:
  - `Drive config error: A root block device already exists!`

## Baseline facts used for this slice

- `PUT /drives/{drive_id}` is preboot-only.
- Replacing an existing drive config by the same id is allowed.

## Local parity decision

- This slice can truthfully tighten the control-plane stub by:
  - reusing the exact upstream drive-root conflict strings,
  - rejecting block-root and pmem-root conflicts before storing new config,
  - preserving allowed same-id updates so `/vm/config` reflects the replaced drive path.
