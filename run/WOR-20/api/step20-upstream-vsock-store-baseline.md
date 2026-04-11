# Step 20 upstream vsock store baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: preboot `PUT /vsock` stored-config and `/vm/config` export behavior

## Source packet

- `src/firecracker/src/api_server/request/vsock.rs`
  - `parse_put_vsock()` deserializes directly into `VsockDeviceConfig`.
  - A deprecated `vsock_id` field is accepted during request parsing and recorded only as a deprecation message.
- `src/vmm/src/vmm_config/vsock.rs`
  - `VsockDeviceConfig` stores `guest_cid` and `uds_path`, with `vsock_id` declared as an optional deprecated field.
  - `VsockBuilder::config()` re-serializes stored config back into `VsockDeviceConfig` with `vsock_id: None`.

## Derived baseline facts

- Preboot `PUT /vsock` should accept a request that includes the deprecated `vsock_id` field.
- `/vm/config` should export stored vsock config with `guest_cid` and `uds_path` while omitting deprecated `vsock_id`.

## Local parity decision

- This slice can truthfully tighten the harness by adding full-router proof for the existing vsock store/export behavior without changing the implementation path.
