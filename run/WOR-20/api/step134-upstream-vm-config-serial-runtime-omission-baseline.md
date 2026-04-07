# Step 134 upstream vm-config serial runtime omission baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: runtime `GET /vm/config` serial omission after successful booted `PUT /serial`

## Source packet

- `run/WOR-20/api/step123-upstream-vm-config-serial-absent-capture-baseline.md`
  - documents the pinned preboot fact that successful `PUT /serial` updates serial configuration state but `/vm/config` still omits the `serial` field.
- `run/WOR-20/api/step108-upstream-serial-runtime-baseline.md`
  - documents the pinned runtime fact that `PUT /serial` remains callable after boot.
- `src/api.rs`
  - full VM config export does not serialize serial configuration into `/vm/config`.

## Derived baseline facts

- `PUT /serial` should remain callable after boot.
- Successful runtime serial configuration should not add a `serial` field to `GET /vm/config`; the omission persists across the boot boundary.

## Local parity decision

- Keep this slice capture-oriented: add one dedicated booted `/vm/config` omission proof for serial, register the runtime omission case in the generated behavior matrix, and leave handler behavior unchanged.
