# Step 65 upstream baseline notes: snapshot success semantics after parser and state guards

Pinned Firecracker baseline revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`

Primary sources inspected:

- `third_party/firecracker/firecracker.yaml`
- `run/WOR-20/api/step33-upstream-snapshot-load-nonboot-config-baseline.md`
- `run/WOR-20/api/step34-upstream-snapshot-create-paused-baseline.md`

Captured facts:

1. In the pinned OpenAPI spec, `PUT /snapshot/create` lists a `204` success response with description `Snapshot created`.
2. In the pinned OpenAPI spec, `PUT /snapshot/load` lists a `204` success response with description `Snapshot loaded`.
3. In the pinned `SnapshotLoadParams` schema, `resume_vm` is documented as resuming the VM when the snapshot load succeeds.
4. Step 34 already proved that `PUT /snapshot/create` reaches the route-specific path only after the microVM is booted and paused.
5. Step 33 already proved that `PUT /snapshot/load` remains reachable after logger, metrics, and serial configuration, before any later snapshot implementation detail is considered.

Implication for the local Step 65 slice:

- The Phase 0 snapshot-create stub should return `204 No Content` once the existing paused-state prerequisite is satisfied.
- The Phase 0 snapshot-load stub should return `204 No Content` once the existing parser and fresh-process resource guards are satisfied.
- Snapshot load should advance the local instance state to `Paused` by default and to `Running` when `resume_vm=true`, matching the pinned parameter description instead of leaving the process in `PreBoot`.
