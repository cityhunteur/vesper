# Step 9 upstream balloon config baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: `GET /balloon`, `PUT /balloon`, `/vm/config` balloon export, machine-config interaction, and snapshot-load blocking implications for configured balloon state

## Source packet

- `src/firecracker/src/api_server/request/balloon.rs`
  - maps `GET /balloon` to `GetBalloonConfig`
  - maps `PUT /balloon` to `SetBalloonDevice`
- `src/vmm/src/vmm_config/balloon.rs`
  - defines `BalloonDeviceConfig`
  - required fields: `amount_mib`, `deflate_on_oom`
  - defaulted fields: `stats_polling_interval_s = 0`, `free_page_hinting = false`, `free_page_reporting = false`
  - denies unknown fields
- `src/vmm/src/resources.rs`
  - rejects balloon targets larger than guest memory during `set_balloon_device`
  - rejects machine memory shrink below an already configured balloon target during machine-config updates
  - exports optional `balloon` config in the full VM configuration
- `src/vmm/src/rpc_interface.rs`
  - returns `200` with serialized `BalloonDeviceConfig` for successful `GetBalloonConfig`
  - turns VMM action errors into `400` responses carrying the controller error string
- `tests/integration_tests/functional/test_api.py`
  - covers preboot create/read balloon flow, oversize balloon rejection, invalid preboot `PATCH /balloon`, and runtime update behavior after boot

## Baseline facts used for this slice

- `GET /balloon` returns the configured balloon descriptor when a device exists.
- `GET /balloon` returns `400` when no balloon device is configured.
- `PUT /balloon` is a preboot operation and stores Firecracker defaults for omitted optional fields.
- `PUT /balloon` returns `400` when `amount_mib` exceeds guest memory.
- `/vm/config` includes `balloon` when a balloon device has been configured.
- Shrinking `machine-config.mem_size_mib` below the configured balloon target returns `400`.

## Exact fault strings pinned for this slice

- `Balloon config error: No balloon device found.`
- `Balloon config error: Amount of pages requested is too large.`
- `The memory size (MiB) is smaller than the previously set balloon device target size.`

## Local parity decision

- This slice can truthfully implement the balloon config store and export path without yet claiming concrete runtime `PATCH /balloon`, statistics, or hinting handler behavior.
- A configured balloon device should count as snapshot-load blocking state, because the repo already treats configured preboot resources as incompatible with snapshot import setup in the same process.
