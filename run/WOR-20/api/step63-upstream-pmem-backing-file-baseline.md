# Step 63 upstream pmem backing-file baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: preboot `PUT /pmem/{id}` backing-file access and zero-size failures

## Source packet

- `src/firecracker/src/api_server/request/pmem.rs`
  - `parse_put_pmem()` forwards syntactically valid requests to `InsertPmemDevice` after path/body id validation.
- `src/vmm/src/rpc_interface.rs`
  - `InsertPmemDevice` is handled on the preboot controller path.
  - `VmmActionError::PmemDevice` renders as `Pmem device error: {0}`.
- `src/vmm/src/vmm_config/pmem.rs`
  - root-device conflicts are checked before device creation, so backing-file failures matter only when the request survives earlier validation.
  - `PmemConfigError::CreateDevice` renders as `Unable to create the virtio-pmem device: {0}`.
- `src/vmm/src/devices/virtio/pmem/device.rs`
  - `PmemError::BackingFile` renders as `Error accessing backing file: {0}`.
  - `Pmem::new()` opens the host backing file and rejects filesystem access errors.
  - `PmemError::BackingFileZeroSize` renders as `Error backing file size is 0`.
  - `Pmem::new()` rejects a zero-length backing file after reading metadata.

## Derived preboot fault strings

- preboot `PUT /pmem/{id}` with a missing backing file:
  - `Pmem device error: Unable to create the virtio-pmem device: Error accessing backing file: No such file or directory (os error 2)`
- preboot `PUT /pmem/{id}` with a zero-sized backing file:
  - `Pmem device error: Unable to create the virtio-pmem device: Error backing file size is 0`

## Local parity decision

- This slice can truthfully tighten the preboot pmem stub by:
  - validating backing-file accessibility only after the existing path/body id and root-device conflict checks,
  - reusing the pinned Firecracker-shaped missing-file and zero-sized-file envelopes,
  - preserving the successful store path and unchanged `/vm/config` export after rejected inserts.

