# Step 113 upstream vm-config MMDS runtime baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: `GET /vm/config` MMDS networking export surface after the microVM is booted

## Source packet

- `run/WOR-20/api/step7-upstream-mmds-baseline.md`
  - Firecracker exports `mmds_config` through full VM config.
  - `MmdsConfig` defaults to version `V1`, default `imds_compat = false`, and default IPv4 address `169.254.169.254`.
  - MMDS config requires pre-existing network interface IDs and is stored independently from the MMDS datastore itself.
- `third_party/firecracker/firecracker.yaml`
  - `GET /vm/config` is the `getExportVmConfig` operation and is documented as returning the full VM configuration rather than a lifecycle-gated preboot-only export.
- `src/api.rs`
  - `full_vm_config_response()` clones the stored `mmds_config` into the full VM config response.

## Derived baseline facts

- `GET /vm/config` should remain callable after boot because the pinned full-configuration export reads stored resources rather than a preboot-only staging view.
- After preboot `PUT /network-interfaces/{iface_id}` and `PUT /mmds/config`, the runtime full-config export should continue to serialize the stored `mmds-config` block with Firecracker defaults.

## Local parity decision

- This slice is harness-first, not a handler change: Vesper already exports stored MMDS networking configuration through `GET /vm/config` after boot, but the generated route-level behavior matrix still only recorded a preboot export case for that block.
