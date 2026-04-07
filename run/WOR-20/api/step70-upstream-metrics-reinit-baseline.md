# Step 70 upstream metrics reinitialization baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: `PUT /metrics` duplicate initialization rejection

## Primary sources inspected

- `src/firecracker/src/api_server/request/metrics.rs`
  - `parse_put_metrics()` maps a valid request body into `VmmAction::ConfigureMetrics`.
- `src/vmm/src/resources.rs`
  - `VmResources::from_json()` calls `init_metrics(metrics)` when metrics config is present.
- `src/vmm/src/vmm_config/metrics.rs`
  - `init_metrics()` opens the target and forwards initialization to `METRICS.init(writer)`.
- `src/vmm/src/logger/metrics.rs`
  - `Metrics::init()` returns `MetricsError::AlreadyInitialized` on a second initialization.
  - `MetricsError::AlreadyInitialized` renders as `Reinitialization of metrics not allowed.`.
- `tests/integration_tests/functional/test_cmd_line_parameters.py`
  - `test_cli_metrics_path_if_metrics_initialized_twice_fail()` expects a repeated metrics initialization attempt to fail with `Reinitialization of metrics not allowed.`.

## Derived baseline facts

- `PUT /metrics` accepts a valid metrics configuration payload once and initializes the metrics sink immediately.
- A second metrics initialization attempt is rejected rather than silently replacing the existing sink.
- The user-visible duplicate-init detail is `Reinitialization of metrics not allowed.`; combined with the config-initialization wrapper, the full request-level failure string is `Cannot initialize the metrics system due to bad user input: Reinitialization of metrics not allowed.`.

## Local parity decision

- This slice is harness-first, not a behavior fix: Vesper already rejects duplicate metrics initialization with the pinned Firecracker-shaped `400 InvalidRequest` envelope, but GH `#2` still needed upstream-backed full-router proof and a route-level behavior-matrix capture for that error case.
