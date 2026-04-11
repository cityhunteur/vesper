# Step 131 upstream vm-config entropy runtime baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: `GET /vm/config` entropy export surface after the microVM is booted

## Source packet

- `run/WOR-20/api/step117-upstream-vm-config-entropy-preboot-capture-baseline.md`
  - pinned the preboot `/vm/config` entropy export surface after `PUT /entropy`.
- `run/WOR-20/api/step21-upstream-entropy-store-baseline.md`
  - the configured entropy state is serialized into `/vm/config` from the stored controller-side entropy configuration.
- `third_party/firecracker/firecracker.yaml`
  - `GET /vm/config` is the `getExportVmConfig` operation and is documented as returning the full VM configuration rather than a preboot-only staging view.
- `src/api.rs`
  - full VM config export serializes stored entropy configuration without a boot-state-specific branch.

## Derived baseline facts

- `GET /vm/config` should remain callable after boot after a successful preboot `PUT /entropy`.
- The booted full-config export should continue to serialize the stored entropy configuration fields unchanged until a runtime entropy update path exists.

## Local parity decision

- This slice is harness-first, not a handler change: Vesper already exports stored entropy configuration through `GET /vm/config` after boot, but the generated route-level behavior matrix still only recorded the preboot entropy export case.
