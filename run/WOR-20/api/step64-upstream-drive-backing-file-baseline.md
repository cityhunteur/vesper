# Step 64 upstream drive backing-file baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: preboot `PUT /drives/{drive_id}` missing backing-file failures

## Source packet

- `src/vmm/src/vmm_config/drive.rs`
  - `DriveError::CreateBlockDevice` displays as `Unable to create the virtio block device: {0}`.
  - `BlockBuilder::insert()` builds the replacement `Block` before mutating the stored drive list, so failed inserts do not overwrite an existing config.
- `src/vmm/src/devices/virtio/block/mod.rs`
  - `BlockError::VirtioBackend` displays as `Virtio backend error: {0}`.
- `src/vmm/src/devices/virtio/block/virtio/mod.rs`
  - `VirtioBlockError::BackingFile(std::io::Error, String)` displays as `Error manipulating the backing file: {0} {1}`.
- `src/vmm/src/devices/virtio/block/virtio/device.rs`
  - `DiskProperties::open_file()` opens the backing file with read access and optional write access, and maps `open()` failures into `VirtioBlockError::BackingFile`.

## Derived preboot fault string

- preboot `PUT /drives/{drive_id}` with a missing `path_on_host` backing file:
  - `Drive config error: Unable to create the virtio block device: Virtio backend error: Error manipulating the backing file: No such file or directory (os error 2) /definitely/missing-drive.ext4`

## Baseline facts used for this slice

- Drive backing-file access failures happen after path/body id validation and root-device conflict checks.
- Failed drive creation should not pollute `/vm/config`, even when the request targets an existing drive id.

## Local parity decision

- This slice can truthfully tighten the control-plane stub by validating the drive backing file after the existing id/root-order guards and before storing the config.
