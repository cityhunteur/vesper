# Step 12 upstream balloon hinting status baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: runtime `GET /balloon/hinting/status`

## Source packet

- `src/vmm/src/rpc_interface.rs`
  - `GetFreePageHintingStatus` is runtime-only.
  - runtime hinting status reads map through `VmmActionError::BalloonUpdate`, which formats as `Balloon update error: {0}`.
- `src/vmm/src/lib.rs`
  - runtime hinting status uses the live balloon device path via `get_balloon_hinting_status()`.
  - missing-device lookup failures therefore retain the `Failed perform action on device: Device not found` chain used by other balloon runtime update paths.
- `src/vmm/src/devices/virtio/balloon/mod.rs`
  - `FREE_PAGE_HINT_STOP` is `0`.
  - `FREE_PAGE_HINT_DONE` is `1`.
  - `BalloonError::HintingNotEnabled` displays as `Attempting to use hinting when not enabled`.
- `src/vmm/src/devices/virtio/balloon/device.rs`
  - `HintingState` defaults `host_cmd` to `FREE_PAGE_HINT_STOP` and `guest_cmd` to `None`.
  - `HintingStatus` serializes `host_cmd` and `guest_cmd`, with `guest_cmd` represented as an optional field rather than a skipped one.
  - `test_cannot_update_inactive_device` proves hinting status is unavailable on an inactive device.
- `tests/integration_tests/functional/test_balloon.py`
  - hinting flows are only exercised when the balloon was configured preboot with `free_page_hinting=true`.

## Derived runtime fault strings

- runtime `GET /balloon/hinting/status` with no balloon device:
  - `Balloon update error: Failed perform action on device: Device not found`
- runtime `GET /balloon/hinting/status` with hinting disabled:
  - `Balloon update error: Balloon: Attempting to use hinting when not enabled`

## Baseline facts used for this slice

- `GET /balloon/hinting/status` is runtime-only.
- Hinting status reads use the balloon update error category rather than the generic internal-VMM read category.
- The idle hinting status before any run is the default state: `host_cmd = 0` and `guest_cmd = null`.

## Local parity decision

- This slice can truthfully implement the low-risk hinting-status read path in the control-plane stub by:
  - reusing the proven balloon update missing-device envelope,
  - rejecting reads when hinting was not enabled preboot with the upstream hinting-disabled string,
  - returning the idle status shape from the upstream default state without claiming any start/stop command sequencing yet.
