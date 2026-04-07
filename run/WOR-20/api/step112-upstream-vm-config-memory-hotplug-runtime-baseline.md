# Step 112 upstream vm-config memory-hotplug runtime baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: `GET /vm/config` memory-hotplug export surface after the microVM is booted

## Source packet

- `run/WOR-20/api/step6-upstream-hotplug-baseline.md`
  - `MemoryHotplugConfig` defaults are `block_size_mib = 2` and `slot_size_mib = 128`.
  - `/vm/config` includes `memory-hotplug` in the upstream full configuration export.
- `third_party/firecracker/firecracker.yaml`
  - `GET /vm/config` is the `getExportVmConfig` operation and is documented as returning the full VM configuration rather than a lifecycle-gated preboot-only export.
- `src/api.rs`
  - `full_vm_config_response()` clones the stored `memory_hotplug` config into the full VM configuration response without a boot-state-specific branch.

## Derived baseline facts

- `GET /vm/config` should remain callable after boot because the pinned full-configuration export reads stored resources rather than a preboot-only staging view.
- After preboot `PUT /hotplug/memory`, the runtime full-config export should continue to serialize the configured `memory-hotplug` block with Firecracker defaults applied.

## Local parity decision

- This slice is harness-first, not a handler change: Vesper already exports stored memory-hotplug configuration through `GET /vm/config` after boot, but the generated route-level behavior matrix still only recorded the preboot export fact for that config family.
