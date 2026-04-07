# Step 108 upstream serial runtime baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: `PUT /serial` acceptance after the microVM is booted

## Source packet

- `run/WOR-20/api/step25-upstream-serial-store-baseline.md`
  - `parse_put_serial()` parses the request into `SerialConfig` and wraps `VmmAction::ConfigureSerial`.
  - `VmResources::from_json()` copies `serial_cfg.serial_out_path` into stored resources.
- `src/firecracker/src/api_server/request/serial.rs`
  - the `PUT /serial` parser path does not introduce a microVM lifecycle-state guard.

## Derived baseline facts

- `PUT /serial` should remain callable after boot because the pinned serial configuration path is state-independent.
- A valid runtime serial configuration request should continue to return `204 No Content`.

## Local parity decision

- This slice is harness-first, not a handler change: Vesper already accepts serial updates after boot, but the generated route-level behavior matrix still only recorded the generic success path without explicit runtime proof.
