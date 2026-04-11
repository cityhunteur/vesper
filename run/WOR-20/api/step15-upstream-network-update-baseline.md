# Step 15 upstream network update baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: network-interface path-id validation and runtime `PATCH /network-interfaces/{iface_id}` update behavior

## Source packet

- `src/firecracker/src/api_server/request/net.rs`
  - `parse_put_net()` rejects path/body id mismatches with `The id from the path [{path}] does not match the id from the body [{body}]!`.
  - `parse_patch_net()` rejects path/body id mismatches with the same exact formatted string.
  - `parse_patch_net()` routes accepted requests to `UpdateNetworkInterface`.
- `src/vmm/src/rpc_interface.rs`
  - `UpdateNetworkInterface` is runtime-only.
  - runtime net updates surface through `VmmActionError::NetworkConfig`.
- `src/vmm/src/vmm_config/net.rs`
  - `NetworkInterfaceError::DeviceUpdate` displays as `Unable to update the net device: {0}`.
- `src/vmm/src/device_manager/mod.rs`
  - `FindDeviceError::DeviceNotFound` displays as `Device not found`.
- `src/vmm/src/lib.rs`
  - runtime net updates use the live device path via `update_net_rate_limiters()`.

## Derived runtime fault strings

- `PUT /network-interfaces/{iface_id}` or `PATCH /network-interfaces/{iface_id}` with a path/body id mismatch:
  - `The id from the path [{path}] does not match the id from the body [{body}]!`
- runtime `PATCH /network-interfaces/{iface_id}` with no configured interface:
  - `Network config error: Unable to update the net device: Failed perform action on device: Device not found`

## Baseline facts used for this slice

- `PUT /network-interfaces/{iface_id}` is preboot-only.
- `PATCH /network-interfaces/{iface_id}` is runtime-only.
- Runtime net patching updates the RX and TX rate limiter configuration for an existing interface.

## Local parity decision

- This slice can truthfully tighten the control-plane stub by:
  - reusing the exact upstream path/body id mismatch format for the network-interface endpoints,
  - rejecting runtime interface patches on missing interfaces with the pinned network-update envelope,
  - preserving the existing low-risk stored-config update path for successful runtime rate-limiter changes.
