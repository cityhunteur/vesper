# Step 103 upstream snapshot-create preboot invalid-state baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: preboot `PUT /snapshot/create` invalid-state behavior

## Source packet

- `run/WOR-20/api/step34-upstream-snapshot-create-paused-baseline.md`
  - records that `CreateSnapshot` can be called only after booting the microVM.
  - records that the booted microVM must be `Paused` before `PUT /snapshot/create` can proceed.
- `run/WOR-20/api/step65-upstream-snapshot-success-baseline.md`
  - confirms that a paused postboot request is the success path for snapshot creation.
- pinned neighboring WOR-20 invalid-state captures:
  - `run/WOR-20/api/step102-upstream-network-patch-preboot-baseline.md`
  - `run/WOR-20/api/step101-upstream-drive-patch-preboot-baseline.md`
  - `run/WOR-20/api/step100-upstream-balloon-stats-get-preboot-baseline.md`

## Derived baseline facts

- `PUT /snapshot/create` is postboot-only and additionally requires the booted microVM to be `Paused`.
- A request body containing `mem_file_path` and `snapshot_path` is a valid create-snapshot request shape, so a preboot request with that body exercises only the state-machine guard.
- Neighboring postboot-only Firecracker routes use the canonical preboot invalid-state envelope:
  - status: `400`
  - code: `InvalidState`
  - message: `The requested operation is not supported before starting the microVM.`

## Local parity decision

- This slice can truthfully tighten the local snapshot-create handler by:
  - preserving the existing running-state rejection and paused-state success behavior,
  - distinguishing the preboot request path from the running-state path,
  - and asserting the same canonical preboot invalid-state envelope already pinned for neighboring postboot-only routes.

- The exact preboot envelope is an inference from the pinned Firecracker postboot-only classification plus the existing upstream-backed `OperationNotSupportedPreBoot` captures listed above.
