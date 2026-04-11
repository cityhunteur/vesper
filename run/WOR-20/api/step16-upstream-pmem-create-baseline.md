# Step 16 upstream pmem create baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: preboot `PUT /pmem/{id}` id validation, default booleans, and root-device conflict behavior

## Source packet

- `src/firecracker/src/api_server/request/pmem.rs`
  - `parse_put_pmem()` rejects path/body id mismatches with `The id from the path does not match the id from the body!`.
  - accepted requests route to `InsertPmemDevice`.
- `src/vmm/src/rpc_interface.rs`
  - `InsertPmemDevice` is preboot-only.
  - pmem configuration failures surface through `VmmActionError::PmemDevice`, which formats as `Pmem device error: {0}`.
- `src/vmm/src/vmm_config/pmem.rs`
  - `PmemConfig.root_device` defaults to `false`.
  - `PmemConfig.read_only` defaults to `false`.
  - `PmemConfigError::AddingSecondRootDevice` displays as `Attempt to add pmem as a root device while the root device defined as a block device`.
  - `PmemConfigError::RootPmemDeviceAlreadyExist` displays as `A root pmem device already exist`.
  - updates to the same pmem id are allowed.

## Derived preboot fault strings

- preboot `PUT /pmem/{id}` with a path/body id mismatch:
  - `The id from the path does not match the id from the body!`
- preboot `PUT /pmem/{id}` with `root_device=true` while a block root already exists:
  - `Pmem device error: Attempt to add pmem as a root device while the root device defined as a block device`
- preboot `PUT /pmem/{id}` with `root_device=true` while another pmem root already exists:
  - `Pmem device error: A root pmem device already exist`

## Baseline facts used for this slice

- `PUT /pmem/{id}` is preboot-only.
- Missing `root_device` and `read_only` default to `false`.
- Replacing an existing pmem config by id is allowed.

## Local parity decision

- This slice can truthfully tighten the control-plane stub by:
  - reusing the exact upstream path/body id mismatch string,
  - rejecting block-root and duplicate-pmem-root conflicts with the pinned pmem error envelopes,
  - normalizing stored pmem booleans so `/vm/config` exports the same defaulted shape that Firecracker stores.
