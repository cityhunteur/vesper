# Step 101 upstream drive-patch preboot invalid-state baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: preboot `PATCH /drives/{drive_id}` invalid-state behavior

## Source packet

- `run/WOR-20/api/step14-upstream-drive-update-baseline.md`
  - records that `PATCH /drives/{drive_id}` is runtime-only.
  - records that `PATCH /drives/{drive_id}` with only `drive_id` is a valid request shape upstream.
- `src/firecracker/src/api_server/request/drive.rs`
  - `parse_patch_drive()` routes accepted requests through the runtime drive-update path.
- pinned neighboring WOR-20 invalid-state captures:
  - `run/WOR-20/api/step100-upstream-balloon-stats-get-preboot-baseline.md`
  - `run/WOR-20/api/step99-upstream-balloon-hinting-stop-preboot-baseline.md`
  - `run/WOR-20/api/step94-upstream-hotplug-patch-preboot-baseline.md`

## Derived baseline facts

- `PATCH /drives/{drive_id}` is runtime-only.
- A request body containing only `drive_id` is valid for the runtime patch path, so a preboot request with that body exercises only the state-machine guard.
- Neighboring runtime-only Firecracker routes use the canonical preboot invalid-state envelope:
  - status: `400`
  - code: `InvalidState`
  - message: `The requested operation is not supported before starting the microVM.`

## Local parity decision

- This slice can truthfully seed a route-level capture for the preboot `PATCH /drives/{drive_id}` invalid-state path by:
  - reusing the pinned runtime-only classification from step 14,
  - sending an otherwise valid `{"drive_id":"rootfs"}` patch request through the full router,
  - and asserting the same canonical preboot invalid-state envelope already pinned for neighboring runtime-only routes.

- The exact preboot envelope is an inference from the pinned Firecracker runtime-only classification plus the existing upstream-backed `OperationNotSupportedPreBoot` captures listed above.
