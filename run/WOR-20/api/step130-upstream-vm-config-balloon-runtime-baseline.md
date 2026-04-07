# Step 130 upstream vm-config balloon runtime baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: `GET /vm/config` balloon export surface after the microVM is booted

## Source packet

- `run/WOR-20/api/step116-upstream-vm-config-balloon-preboot-capture-baseline.md`
  - pinned the preboot `/vm/config` balloon export surface after `PUT /balloon`.
- `run/WOR-20/api/step9-upstream-balloon-config-baseline.md`
  - the configured balloon state is serialized into `/vm/config` from the stored controller-side balloon configuration.
- `third_party/firecracker/firecracker.yaml`
  - `GET /vm/config` is the `getExportVmConfig` operation and is documented as returning the full VM configuration rather than a preboot-only staging view.
- `src/api.rs`
  - full VM config export serializes stored balloon configuration without a boot-state-specific branch.

## Derived baseline facts

- `GET /vm/config` should remain callable after boot after a successful preboot `PUT /balloon`.
- The booted full-config export should continue to serialize the stored balloon configuration fields unchanged until a runtime balloon update occurs.

## Local parity decision

- This slice is harness-first, not a handler change: Vesper already exports stored balloon configuration through `GET /vm/config` after boot, but the generated route-level behavior matrix still only recorded the preboot balloon export case.
