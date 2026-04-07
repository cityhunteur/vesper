# Step 22 upstream boot-source store baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: preboot `PUT /boot-source` stored-config and `/vm/config` export behavior

## Source packet

- `src/firecracker/src/api_server/request/boot_source.rs`
  - `parse_put_boot_source()` deserializes directly into `BootSourceConfig` and wraps `VmmAction::ConfigureBootSource`.
- `src/vmm/src/resources.rs`
  - `VmResources::build_boot_source()` stores the incoming `BootSourceConfig` as `boot_source.config`.
  - `impl From<&VmResources> for VmmConfig` re-exports `boot_source: resources.boot_source.config.clone()`.

## Derived baseline facts

- Preboot `PUT /boot-source` should accept a valid `BootSourceConfig` payload and store the configured kernel path, optional initrd path, and optional boot arguments.
- `/vm/config` should export the stored boot source through the `boot-source` section.

## Local parity decision

- This slice can truthfully tighten the harness by adding full-router proof for the existing boot-source store/export behavior without changing the implementation path.
