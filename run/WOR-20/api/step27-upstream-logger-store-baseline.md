# Step 27 upstream logger store baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: `PUT /logger` accepted state updates and `/vm/config` export behavior

## Source packet

- `src/firecracker/src/api_server/request/logger.rs`
  - `parse_put_logger()` parses the request into `LoggerConfig` and wraps `VmmAction::ConfigureLogger`.
- `src/vmm/src/resources.rs`
  - `VmResources::from_json()` calls `crate::logger::LOGGER.update(logger_config)` when logger config is present.
  - `VmmConfig` keeps `logger` as an optional config field for input parsing.
  - `impl From<&VmResources> for VmmConfig` exports `logger: None`.

## Derived baseline facts

- `PUT /logger` should accept a valid logger configuration payload and update logger configuration state for later use.
- `/vm/config` should omit logger configuration even after successful `PUT /logger`.

## Local parity decision

- This slice is harness-first, not a behavior fix: Vesper already merges logger updates internally, rejects invalid log targets, and omits logger from `/vm/config`, but GH `#2` still needed upstream-backed full-router proof for the successful accepted-state/export path.
