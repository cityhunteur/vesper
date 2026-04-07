# Step 126 upstream vm-config vsock_id omission baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: `GET /vm/config` vsock export omission of deprecated `vsock_id` before and after boot

## Source packet

- `run/WOR-20/api/step20-upstream-vsock-store-baseline.md`
  - Firecracker accepts deprecated `vsock_id` during `PUT /vsock` request parsing.
  - `VsockBuilder::config()` re-serializes stored config with `vsock_id: None`.
- `run/WOR-20/api/step111-upstream-vm-config-vsock-runtime-baseline.md`
  - The booted `GET /vm/config` path reuses the same stored vsock configuration export after boot.
- `tests/api_contract.rs`
  - `vsock_put_matches_pinned_store_behavior` and `vm_config_vsock_export_remains_available_after_boot_on_full_router` both assert that `/vm/config` exports `guest_cid` and `uds_path` while omitting deprecated `vsock_id`.

## Derived baseline facts

- `GET /vm/config` should omit deprecated `vsock_id` from the exported vsock block before boot.
- `GET /vm/config` should continue omitting deprecated `vsock_id` from the exported vsock block after boot.

## Local parity decision

- This slice does not change the handler path. It tightens the generated route-level behavior matrix so the existing preboot and runtime vsock export captures explicitly assert the pinned deprecated-field omission.
