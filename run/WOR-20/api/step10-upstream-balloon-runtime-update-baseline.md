# Step 10 upstream balloon runtime update baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: runtime `GET /balloon` missing-device behavior and runtime `PATCH /balloon` target update behavior

## Source packet

- `src/vmm/src/rpc_interface.rs`
  - runtime `GetBalloonConfig` maps VMM lookup failures through `VmmActionError::InternalVmm`
  - runtime `UpdateBalloon` maps target updates through `VmmActionError::BalloonUpdate`
  - `VmmActionError::BalloonUpdate` formats as `Balloon update error: {0}`
  - `VmmActionError::InternalVmm` formats as `Internal VMM error: {0}`
- `src/vmm/src/lib.rs`
  - `balloon_config()` and `update_balloon_config()` both call `with_virtio_device(BALLOON_DEV_ID, ...)`
- `src/vmm/src/device_manager/mod.rs`
  - missing virtio device returns `FindDeviceError::DeviceNotFound`
  - that display string is `Device not found`
- `src/vmm/src/lib.rs`
  - `VmmError::FindDeviceError` formats as `Failed perform action on device: {0}`
- `src/vmm/src/devices/virtio/balloon/mod.rs`
  - oversize runtime update returns `BalloonError::TooMuchMemoryRequested(limit_mib)`
  - that display string is `Requested memory should be less than {limit_mib}MiB`

## Derived runtime fault strings

- runtime `GET /balloon` with no balloon device:
  - `Internal VMM error: Failed perform action on device: Device not found`
- runtime `PATCH /balloon` with no balloon device:
  - `Balloon update error: Failed perform action on device: Device not found`
- runtime `PATCH /balloon` with target larger than guest memory:
  - `Balloon update error: Balloon: Requested memory should be less than <guest_mem_mib>MiB`

## Baseline facts used for this slice

- `PATCH /balloon` is runtime-only.
- Successful runtime update returns an empty success response and changes the target reported by `GET /balloon`.
- Runtime lookup/update uses the live balloon device path, so missing-device reads and writes do not share the same error category as preboot config-store reads.

## Local parity decision

- This slice can truthfully implement the runtime balloon target update path in the stubbed control plane by:
  - preserving preboot `GET /balloon` behavior for the config-store path,
  - returning the upstream runtime missing-device read/update strings after boot when no balloon device was configured,
  - updating the stored balloon target after boot,
  - rejecting oversize runtime updates with the derived upstream fault string.
