# Step 107 upstream logger runtime baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: `PUT /logger` acceptance after the microVM is booted

## Source packet

- `run/WOR-20/api/step27-upstream-logger-store-baseline.md`
  - `parse_put_logger()` parses the request into `LoggerConfig` and wraps `VmmAction::ConfigureLogger`.
  - `VmResources::from_json()` updates logger configuration state when logger config is present.
- `src/firecracker/src/api_server/request/logger.rs`
  - the `PUT /logger` parser path does not introduce a microVM lifecycle-state guard.

## Derived baseline facts

- `PUT /logger` should remain callable after boot because the pinned logger configuration path is state-independent.
- A valid runtime logger configuration request should continue to return `204 No Content`.

## Local parity decision

- This slice is harness-first, not a handler change: Vesper already accepts logger updates after boot, but the generated route-level behavior matrix still only recorded the generic success path without explicit runtime proof.
