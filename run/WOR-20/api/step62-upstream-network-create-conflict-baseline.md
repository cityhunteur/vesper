# Step 62 upstream network create conflict baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: preboot `PUT /network-interfaces/{iface_id}` guest-MAC conflict and host tap reuse failures

## Source packet

- `src/firecracker/src/api_server/request/net.rs`
  - `parse_put_net()` still accepts a syntactically valid create request after path/body id validation and forwards it to `InsertNetworkDevice`.
- `src/vmm/src/rpc_interface.rs`
  - `InsertNetworkDevice` is handled on the preboot controller path.
  - `VmmActionError::NetworkConfig` renders as `Network config error: {0}`.
- `src/vmm/src/vmm_config/net.rs`
  - `NetworkInterfaceError::GuestMacAddressInUse` renders as `The MAC address is already in use: {0}`.
  - `NetBuilder::build()` rejects a guest-MAC reuse when another interface already owns that MAC.
  - `NetBuilder::build()` does not pre-validate `host_dev_name` reuse; it relies on tap creation to fail when another interface already owns the same TAP.
  - `NetworkInterfaceError::CreateNetworkDevice` renders as `Could not create the network device: {0}`.
- `src/vmm/src/devices/virtio/net/mod.rs`
  - `NetError::TapOpen` renders as `Open tap device failed: {0}`.
- `src/vmm/src/devices/virtio/net/tap.rs`
  - `TapError::IfreqExecuteError` renders as `Error while creating ifreq structure: {0}. Invalid TUN/TAP Backend provided by {1}. Check our documentation on setting up the network devices.`
  - The upstream `test_tap_exclusive_open` path proves a second open on the same TAP fails.

## Derived baseline facts

- Preboot `PUT /network-interfaces/{iface_id}` with a duplicate guest MAC on a different interface should fail with:
  - `Network config error: The MAC address is already in use: 06:00:ac:10:00:02`
- Preboot `PUT /network-interfaces/{iface_id}` with a duplicate host tap name on a different interface should fail with:
  - `Network config error: Could not create the network device: Open tap device failed: Error while creating ifreq structure: Device or resource busy (os error 16). Invalid TUN/TAP Backend provided by tap0. Check our documentation on setting up the network devices.`

## Local parity decision

- This slice can truthfully tighten the preboot network create stub by:
  - rejecting duplicate guest-MAC reuse across different configured interfaces with the pinned Firecracker envelope,
  - rejecting duplicate host tap reuse across different configured interfaces with the pinned tap-open envelope,
  - preserving same-`iface_id` replacement behavior and the existing successful create plus `/vm/config` export path.
