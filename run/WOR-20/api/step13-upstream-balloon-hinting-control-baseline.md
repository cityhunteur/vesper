# Step 13 upstream balloon hinting control baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: runtime `PATCH /balloon/hinting/start` and `PATCH /balloon/hinting/stop`

## Source packet

- `src/firecracker/src/api_server/request/balloon.rs`
  - `parse_patch_hinting()` routes `/balloon/hinting/start` and `/balloon/hinting/stop` as runtime-only hinting actions.
  - start accepts an empty body and defaults to `StartHintingCmd::default()`.
  - start accepts an optional `acknowledge_on_stop` JSON field.
  - stop accepts an empty body and ignores the request body contents.
- `src/vmm/src/rpc_interface.rs`
  - `StartBalloonHinting` and `StopBalloonHinting` both surface through `VmmActionError::BalloonUpdate`, which formats as `Balloon update error: {0}`.
- `src/vmm/src/lib.rs`
  - runtime hinting start and stop both use the live balloon device path.
  - missing-device lookup failures therefore retain the `Failed perform action on device: Device not found` chain used by other balloon runtime update paths.
- `src/vmm/src/devices/virtio/balloon/mod.rs`
  - `FREE_PAGE_HINT_STOP` is `0`.
  - `FREE_PAGE_HINT_DONE` is `1`.
  - `BalloonError::HintingNotEnabled` displays as `Attempting to use hinting when not enabled`.
- `src/vmm/src/devices/virtio/balloon/device.rs`
  - `HintingState` defaults `host_cmd` to `0`, `guest_cmd` to `None`, `last_cmd_id` to `0`, and `acknowledge_on_finish` to `true`.
  - device tests prove a new start bumps the command id and that stop/finish paths can drive the host state to the done command.
- `docs/ballooning.md`
  - hinting is controlled through `/balloon/hinting/start`, `/balloon/hinting/stop`, and `/balloon/hinting/status`.
  - the simple start flow defaults `acknowledge_on_stop=true`.
  - status reports the last command sent by Firecracker and the last update received from the guest.

## Derived runtime fault strings

- runtime `PATCH /balloon/hinting/start` with no balloon device:
  - `Balloon update error: Failed perform action on device: Device not found`
- runtime `PATCH /balloon/hinting/stop` with no balloon device:
  - `Balloon update error: Failed perform action on device: Device not found`
- runtime `PATCH /balloon/hinting/start` with hinting disabled:
  - `Balloon update error: Balloon: Attempting to use hinting when not enabled`
- runtime `PATCH /balloon/hinting/stop` with hinting disabled:
  - `Balloon update error: Balloon: Attempting to use hinting when not enabled`

## Baseline facts used for this slice

- `PATCH /balloon/hinting/start` and `PATCH /balloon/hinting/stop` are runtime-only.
- Start defaults `acknowledge_on_stop` to `true` when the body is omitted or empty.
- Stop accepts an empty body and does not require JSON parsing for success.
- Idle hinting status is `host_cmd = 0` and `guest_cmd = null`.

## Local parity decision

- This slice can truthfully implement a low-risk control-plane stub by:
  - reusing the proven balloon-update missing-device and hinting-disabled envelopes,
  - accepting empty-body start requests and defaulting `acknowledge_on_stop` to `true`,
  - allowing stop requests without parsing the request body,
  - modeling the host-side control progression conservatively as:
    - first start emits command `2`,
    - later starts bump the command id,
    - stop moves the host state to the done command `1`,
  - leaving `guest_cmd` unset in the local stub until guest-side progress is explicitly modeled in a future slice.

- The command progression above is an inference from the reserved command ids, the documented hinting flow, and the upstream device tests. This slice claims only low-risk host-issued control behavior and does not claim full guest-progress parity.
