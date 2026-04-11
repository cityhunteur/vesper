# Step 106 upstream machine-config runtime read baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: `GET /machine-config` response surface after the microVM is booted

## Source packet

- `run/WOR-20/api/step23-upstream-machine-config-store-baseline.md`
  - `parse_get_machine_config()` maps directly to `VmmAction::GetVmMachineConfig`.
  - `VmResources::update_machine_config()` stores the updated machine configuration.
  - `GET /machine-config` should reflect the stored machine configuration.
- `src/firecracker/src/api_server/request/machine_configuration.rs`
  - the `GET /machine-config` parser path does not introduce a microVM lifecycle-state guard.

## Derived baseline facts

- `GET /machine-config` should remain callable after boot because the pinned read path is state-independent.
- After preboot `PUT` then `PATCH` updates, the runtime read should continue to serialize the stored machine configuration fields.

## Local parity decision

- This slice is harness-first, not a handler change: Vesper already persists machine configuration updates and exposes them through `GET /machine-config`, but the generated route-level behavior matrix still only recorded the preboot read surface.
