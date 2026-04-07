# Step 23 upstream machine-config store baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: `GET /machine-config`, preboot `PUT/PATCH /machine-config`, and `/vm/config` export behavior

## Source packet

- `src/firecracker/src/api_server/request/machine_configuration.rs`
  - `parse_get_machine_config()` maps directly to `VmmAction::GetVmMachineConfig`.
  - `parse_put_machine_config()` deserializes `MachineConfig`, converts it to `MachineConfigUpdate`, and wraps `VmmAction::UpdateMachineConfiguration`.
  - `parse_patch_machine_config()` deserializes `MachineConfigUpdate`, rejects an empty patch body, and wraps `VmmAction::UpdateMachineConfiguration`.
- `src/vmm/src/vmm_config/machine_config.rs`
  - `MachineConfigUpdate::from(MachineConfig)` applies Firecracker defaults for unspecified PUT fields.
  - `MachineConfig::update()` merges patch fields into the stored configuration.
- `src/vmm/src/resources.rs`
  - `VmResources::update_machine_config()` stores the updated machine configuration.
  - `impl From<&VmResources> for VmmConfig` re-exports `machine_config: Some(resources.machine_config.clone())`.

## Derived baseline facts

- Preboot `PUT /machine-config` should accept a valid `MachineConfig` payload and store the resulting machine configuration.
- Preboot `PATCH /machine-config` should merge fields into the stored machine configuration and reject an empty patch body.
- `GET /machine-config` and `/vm/config` should reflect the stored machine configuration.

## Local parity decision

- This slice can truthfully tighten the harness by adding full-router proof for the existing machine-config store/update/export behavior without changing the implementation path.
