# Step 144 upstream snapshot-load `resume_vm=true` success baseline

Pinned Firecracker baseline revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`

Source packet:

- `third_party/firecracker/firecracker.yaml`
- `run/WOR-20/api/step65-upstream-snapshot-success-baseline.md`
- `run/WOR-20/api/step104-upstream-instance-info-runtime-state-baseline.md`

Captured facts:

1. The pinned OpenAPI spec records `204 No Content` as the success response for `PUT /snapshot/load`.
2. The pinned `SnapshotLoadParams` schema documents `resume_vm` as resuming the VM when snapshot load succeeds.
3. Step 65 already anchored the local Phase 0 interpretation that successful snapshot load returns `204` once the parser and fresh-process resource guards are satisfied.
4. Step 104 already anchored that the resulting instance state becomes `Running` after a successful snapshot load when `resume_vm=true`.

Implication for the local Step 144 slice:

- The route-level behavior registry should record a second `204` success case for `PUT /snapshot/load` when the request explicitly sets `resume_vm=true`.
- This is a harness-first evidence update, not a handler change: the existing control-plane implementation and full-router contract test already make the request succeed and transition instance state to `Running`.
