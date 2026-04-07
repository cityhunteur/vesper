# Step 26 upstream metrics store baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: `PUT /metrics` stored state and `/vm/config` export behavior

## Source packet

- `src/firecracker/src/api_server/request/metrics.rs`
  - `parse_put_metrics()` parses the request into `MetricsConfig` and wraps `VmmAction::ConfigureMetrics`.
- `src/vmm/src/resources.rs`
  - `VmResources::from_json()` calls `init_metrics(metrics)` when metrics config is present.
  - `VmmConfig` keeps `metrics` as an optional config field for input parsing.
  - `impl From<&VmResources> for VmmConfig` exports `metrics: None`.

## Derived baseline facts

- `PUT /metrics` should accept a valid metrics configuration payload and store metrics initialization intent for later use.
- `/vm/config` should omit metrics configuration even after successful `PUT /metrics`.

## Local parity decision

- This slice is harness-first, not a behavior fix: Vesper already stores metrics config internally, rejects reinitialization, and omits metrics from `/vm/config`, but GH `#2` still needed upstream-backed full-router proof for the successful store/export path.
