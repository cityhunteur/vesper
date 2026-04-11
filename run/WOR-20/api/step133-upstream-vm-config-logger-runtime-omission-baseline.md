# Step 133 upstream vm-config logger runtime omission baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: runtime `GET /vm/config` logger omission after successful booted `PUT /logger`

## Source packet

- `run/WOR-20/api/step125-upstream-vm-config-logger-absent-capture-baseline.md`
  - documents the pinned preboot fact that successful `PUT /logger` updates logger configuration state but `/vm/config` still omits the `logger` field.
- `run/WOR-20/api/step107-upstream-logger-runtime-baseline.md`
  - documents the pinned runtime fact that `PUT /logger` remains callable after boot.
- `src/api.rs`
  - full VM config export does not serialize logger configuration into `/vm/config`.

## Derived baseline facts

- `PUT /logger` should remain callable after boot.
- Successful runtime logger configuration should not add a `logger` field to `GET /vm/config`; the omission persists across the boot boundary.

## Local parity decision

- Keep this slice capture-oriented: add one dedicated booted `/vm/config` omission proof for logger, register the runtime omission case in the generated behavior matrix, and leave handler behavior unchanged.
