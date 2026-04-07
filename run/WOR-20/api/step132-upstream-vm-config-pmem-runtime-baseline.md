# Step 132 upstream vm-config pmem runtime baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: `GET /vm/config` pmem export surface after the microVM is booted

## Source packet

- `run/WOR-20/api/step119-upstream-vm-config-pmem-preboot-capture-baseline.md`
  - pinned the preboot `/vm/config` pmem export surface after `PUT /pmem/{id}`.
- `run/WOR-20/api/step16-upstream-pmem-create-baseline.md`
  - the configured pmem state is serialized into `/vm/config` from the stored controller-side pmem configuration, including defaulted booleans.
- `third_party/firecracker/firecracker.yaml`
  - `GET /vm/config` is the `getExportVmConfig` operation and is documented as returning the full VM configuration rather than a preboot-only staging view.
- `src/api.rs`
  - full VM config export serializes stored pmem configuration without a boot-state-specific branch.

## Derived baseline facts

- `GET /vm/config` should remain callable after boot after a successful preboot `PUT /pmem/{id}`.
- The booted full-config export should continue to serialize the stored pmem default booleans unchanged until an explicit pmem update path exists.

## Local parity decision

- This slice is harness-first, not a handler change: Vesper already exports stored pmem configuration through `GET /vm/config` after boot, but the generated route-level behavior matrix still only recorded the preboot pmem export case.
