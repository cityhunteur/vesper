# Step 25 upstream serial store baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: `PUT /serial` stored state and `/vm/config` export behavior

## Source packet

- `src/firecracker/src/api_server/request/serial.rs`
  - `parse_put_serial()` parses the request into `SerialConfig` and wraps `VmmAction::ConfigureSerial`.
- `src/vmm/src/resources.rs`
  - `VmmConfig` declares `serial_config` with `#[serde(skip)]`, so the field is not part of exported full-configuration JSON.
  - `VmResources::from_json()` copies `serial_cfg.serial_out_path` into `resources.serial_out_path`.
  - `impl From<&VmResources> for VmmConfig` exports `serial_config: None`.

## Derived baseline facts

- `PUT /serial` should accept a valid serial configuration payload and store serial output configuration state for later use.
- `/vm/config` should omit serial configuration even after successful `PUT /serial`.

## Local parity decision

- This slice is harness-first, not a behavior fix: Vesper already stores serial config internally and omits it from `/vm/config`, but GH `#2` still needed upstream-backed full-router proof for that behavior.
