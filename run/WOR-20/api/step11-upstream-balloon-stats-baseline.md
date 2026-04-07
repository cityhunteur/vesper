# Step 11 upstream balloon statistics baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: runtime `GET /balloon/statistics` and `PATCH /balloon/statistics`

## Source packet

- `src/vmm/src/devices/virtio/balloon/mod.rs`
  - `BalloonError::StatisticsDisabled` displays as `Received stats query when stats are disabled.`
  - `BalloonError::StatisticsStateChange` displays as `Statistics cannot be enabled/disabled after activation.`
- `src/vmm/src/lib.rs`
  - balloon stats reads and updates use the live virtio-balloon device lookup path
  - missing-device lookup failures format as `Failed perform action on device: Device not found`
- `src/vmm/src/rpc_interface.rs`
  - stats reads surface through `VmmActionError::InternalVmm`, which formats as `Internal VMM error: {0}`
  - stats interval updates surface through `VmmActionError::BalloonUpdate`, which formats as `Balloon update error: {0}`
- `src/vmm/src/devices/virtio/balloon/device.rs`
  - `test_update_stats_interval` proves the post-activation state-machine rule:
    - `0 -> nonzero` is rejected
    - `nonzero -> 0` is rejected
    - `nonzero -> nonzero` is accepted
    - `0 -> 0` is accepted
- `tests/integration_tests/functional/test_balloon.py`
  - runtime stats polling interval updates are exercised through the dedicated `/balloon/statistics` path
  - the stats path is only meaningful when the balloon was configured preboot with statistics enabled

## Derived runtime fault strings

- runtime `GET /balloon/statistics` with no balloon device:
  - `Internal VMM error: Failed perform action on device: Device not found`
- runtime `GET /balloon/statistics` with stats disabled:
  - `Internal VMM error: Balloon: Received stats query when stats are disabled.`
- runtime `PATCH /balloon/statistics` with no balloon device:
  - `Balloon update error: Failed perform action on device: Device not found`
- runtime `PATCH /balloon/statistics` when toggling stats enabled/disabled after activation:
  - `Balloon update error: Balloon: Statistics cannot be enabled/disabled after activation.`

## Baseline facts used for this slice

- `GET /balloon/statistics` and `PATCH /balloon/statistics` are runtime-only.
- The post-boot state machine allows changing the polling interval only within the already-enabled or already-disabled mode.
- The stats response always includes the required balloon target/actual fields, while the extended counters remain optional.

## Local parity decision

- This slice can truthfully implement a deterministic control-plane stub by:
  - reusing the proven live-device missing-device envelopes after boot,
  - rejecting post-boot stats mode toggles with the upstream state-change string,
  - allowing post-boot interval updates only when statistics were already enabled preboot,
  - returning the required statistics fields with a stable target/actual view derived from the configured balloon size.
