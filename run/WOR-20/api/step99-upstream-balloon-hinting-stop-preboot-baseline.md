# Step 99 upstream balloon hinting-stop preboot invalid-state baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: preboot `PATCH /balloon/hinting/stop` invalid-state behavior

## Source packet

- `run/WOR-20/api/step13-upstream-balloon-hinting-control-baseline.md`
  - records that `PATCH /balloon/hinting/stop` is runtime-only.
  - records that stop accepts an empty body and ignores request-body contents.
- `src/firecracker/src/api_server/request/balloon.rs`
  - `parse_patch_hinting()` routes `/balloon/hinting/stop` through the runtime-only hinting control path.
- pinned neighboring WOR-20 invalid-state captures:
  - `run/WOR-20/api/step98-upstream-balloon-hinting-start-preboot-baseline.md`
  - `run/WOR-20/api/step97-upstream-balloon-hinting-status-preboot-baseline.md`
  - `run/WOR-20/api/step96-upstream-balloon-stats-patch-preboot-baseline.md`

## Derived baseline facts

- `PATCH /balloon/hinting/stop` is runtime-only.
- An empty request body is valid for the stop action at runtime, so a preboot request exercises only the state-machine guard.
- Neighboring runtime-only Firecracker routes use the canonical preboot invalid-state envelope:
  - status: `400`
  - code: `InvalidState`
  - message: `The requested operation is not supported before starting the microVM.`

## Local parity decision

- This slice can truthfully seed a route-level capture for the preboot `PATCH /balloon/hinting/stop` invalid-state path by:
  - reusing the pinned runtime-only classification from step 13,
  - sending an otherwise valid empty-body stop request through the full router,
  - and asserting the same canonical preboot invalid-state envelope already pinned for neighboring runtime-only routes.

- The exact preboot envelope is an inference from the pinned Firecracker runtime-only classification plus the existing upstream-backed `OperationNotSupportedPreBoot` captures listed above.
