# Step 104 upstream instance-info runtime-state baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: `GET /` instance-info state values after successful snapshot-load transitions

## Source packet

- `run/WOR-20/api/step28-upstream-identity-version-baseline.md`
  - `GET /` serializes the current `InstanceInfo` object.
- `third_party/firecracker/firecracker.yaml`
  - `InstanceInfo.state` is an enum with the pinned values `Not started`, `Running`, and `Paused`.
- `run/WOR-20/api/step65-upstream-snapshot-success-baseline.md`
  - successful `PUT /snapshot/load` should advance the local instance state to `Paused` by default
  - successful `PUT /snapshot/load` should advance the local instance state to `Running` when `resume_vm=true`

## Derived baseline facts

- `GET /` should continue to serialize the current instance-information object after snapshot-load success, not only in preboot.
- After a successful default snapshot load, the `GET /` `state` field should be `Paused`.
- After a successful snapshot load with `resume_vm=true`, the `GET /` `state` field should be `Running`.

## Local parity decision

- This slice is harness-first, not a handler change: the existing snapshot-load success work already updates local microVM state, but the generated `GET /` behavior capture still under-reported the runtime `Paused` and `Running` surfaces.
