# Step 135 upstream vm-config metrics runtime omission baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: runtime `GET /vm/config` metrics omission after successful booted `PUT /metrics`

## Source packet

- `run/WOR-20/api/step124-upstream-vm-config-metrics-absent-capture-baseline.md`
  - documents the pinned preboot fact that successful `PUT /metrics` updates metrics configuration state but `/vm/config` still omits the `metrics` field.
- `run/WOR-20/api/step109-upstream-metrics-runtime-baseline.md`
  - documents the pinned runtime fact that `PUT /metrics` remains callable after boot.
- `src/api.rs`
  - full VM config export does not serialize metrics configuration into `/vm/config`.

## Derived baseline facts

- `PUT /metrics` should remain callable after boot.
- Successful runtime metrics configuration should not add a `metrics` field to `GET /vm/config`; the omission persists across the boot boundary.

## Local parity decision

- Keep this slice capture-oriented: add one dedicated booted `/vm/config` omission proof for metrics, register the runtime omission case in the generated behavior matrix, and leave handler behavior unchanged.
