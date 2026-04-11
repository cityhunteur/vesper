# Step 110 upstream vm-config runtime baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: `GET /vm/config` export surface after the microVM is booted

## Source packet

- `run/WOR-20/api/step23-upstream-machine-config-store-baseline.md`
  - `VmResources::update_machine_config()` stores the updated machine configuration.
  - `impl From<&VmResources> for VmmConfig` re-exports the stored machine configuration through the full VM config response.
- `third_party/firecracker/firecracker.yaml`
  - `GET /vm/config` is the `getExportVmConfig` operation and is documented as returning the full VM configuration rather than a lifecycle-gated preboot-only export.
- `src/firecracker/src/api_server/request/machine_configuration.rs`
  - the machine-config update parser path stores configuration without introducing a separate runtime-only export representation.

## Derived baseline facts

- `GET /vm/config` should remain callable after boot because the pinned full-configuration export reads stored resources rather than a preboot-only staging view.
- After preboot `PUT` then `PATCH` machine-config updates, the runtime full-config export should continue to serialize the stored `machine-config` fields.

## Local parity decision

- This slice is harness-first, not a handler change: Vesper already exports stored machine configuration through `GET /vm/config` after boot, but the generated route-level behavior matrix still only recorded preboot export cases for that route.
