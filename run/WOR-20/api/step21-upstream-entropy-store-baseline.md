# Step 21 upstream entropy store baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: preboot `PUT /entropy` stored-config and `/vm/config` export behavior

## Source packet

- `src/firecracker/src/api_server/request/entropy.rs`
  - `parse_put_entropy()` deserializes directly into `EntropyDeviceConfig`.
- `src/vmm/src/vmm_config/entropy.rs`
  - `EntropyDeviceConfig` stores an optional `rate_limiter`.
  - `EntropyDeviceBuilder::config()` re-serializes the stored entropy device back into `EntropyDeviceConfig`.

## Derived baseline facts

- Preboot `PUT /entropy` should accept a valid `EntropyDeviceConfig` payload and store the configured rate limiter.
- `/vm/config` should export the stored entropy rate limiter shape through the `entropy` section.

## Local parity decision

- This slice can truthfully tighten the harness by adding full-router proof for the existing entropy store/export behavior without changing the implementation path.
