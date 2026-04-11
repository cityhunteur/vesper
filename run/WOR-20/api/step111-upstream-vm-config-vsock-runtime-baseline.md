# Step 111 upstream vm-config vsock runtime baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: `GET /vm/config` vsock export surface after the microVM is booted

## Source packet

- `run/WOR-20/api/step20-upstream-vsock-store-baseline.md`
  - `VsockDeviceConfig` stores `guest_cid` and `uds_path`.
  - `VsockBuilder::config()` re-serializes stored config back into `VsockDeviceConfig` with deprecated `vsock_id` omitted.
- `third_party/firecracker/firecracker.yaml`
  - `GET /vm/config` is the `getExportVmConfig` operation and is documented as returning the full VM configuration rather than a lifecycle-gated preboot-only export.
- `src/api.rs`
  - `full_vm_config_response()` clones the stored `vsock_config` into the full VM configuration response without a boot-state-specific branch.

## Derived baseline facts

- `GET /vm/config` should remain callable after boot because the pinned full-configuration export reads stored resources rather than a preboot-only staging view.
- After preboot `PUT /vsock`, the runtime full-config export should continue to serialize `guest_cid` and `uds_path` while omitting deprecated `vsock_id`.

## Local parity decision

- This slice is harness-first, not a handler change: Vesper already exports stored vsock configuration through `GET /vm/config` after boot, but the generated route-level behavior matrix still only recorded preboot export facts for that config family.
