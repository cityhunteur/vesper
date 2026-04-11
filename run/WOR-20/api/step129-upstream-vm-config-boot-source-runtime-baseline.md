# Step 129 upstream vm-config boot-source runtime baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: `GET /vm/config` boot-source export surface after the microVM is booted

## Source packet

- `run/WOR-20/api/step118-upstream-vm-config-boot-source-preboot-capture-baseline.md`
  - pinned the preboot `/vm/config` boot-source export surface after `PUT /boot-source`.
- `third_party/firecracker/firecracker.yaml`
  - `GET /vm/config` is the `getExportVmConfig` operation and is documented as returning the full VM configuration rather than a preboot-only staging view.
- `src/api.rs`
  - full VM config export serializes stored boot-source configuration without a boot-state-specific branch.

## Derived baseline facts

- `GET /vm/config` should remain callable after boot after a successful preboot `PUT /boot-source`.
- The booted full-config export should continue to serialize the stored `kernel_image_path`, `initrd_path`, and `boot_args`.

## Local parity decision

- This slice is harness-first, not a handler change: Vesper already exports stored boot-source configuration through `GET /vm/config` after boot, but the generated route-level behavior matrix still only recorded the preboot boot-source export case.
