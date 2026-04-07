# Step 109 upstream metrics runtime baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: `PUT /metrics` acceptance after the microVM is booted

## Source packet

- `run/WOR-20/api/step26-upstream-metrics-store-baseline.md`
  - `parse_put_metrics()` parses the request into `MetricsConfig` and wraps `VmmAction::ConfigureMetrics`.
  - `VmResources::from_json()` calls `init_metrics(metrics)` when metrics config is present.
- `src/firecracker/src/api_server/request/metrics.rs`
  - the `PUT /metrics` parser path does not introduce a microVM lifecycle-state guard.

## Derived baseline facts

- `PUT /metrics` should remain callable after boot because the pinned metrics configuration path is state-independent.
- A valid runtime metrics configuration request should continue to return `204 No Content` when metrics have not already been initialized.

## Local parity decision

- This slice is harness-first, not a handler change: Vesper already accepts first-time metrics initialization after boot, but the generated route-level behavior matrix still only recorded the generic success case plus duplicate-init rejection.
