# Step 100 upstream balloon statistics-get preboot invalid-state baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: preboot `GET /balloon/statistics` invalid-state behavior

## Source packet

- `run/WOR-20/api/step11-upstream-balloon-stats-baseline.md`
  - records that `GET /balloon/statistics` is runtime-only.
  - records the route's runtime success and fault surface once the microVM has started.
- `src/firecracker/src/api_server/request/balloon.rs`
  - the balloon statistics route is parsed as the dedicated stats request path and depends on the live balloon device path after boot.
- pinned neighboring WOR-20 invalid-state captures:
  - `run/WOR-20/api/step97-upstream-balloon-hinting-status-preboot-baseline.md`
  - `run/WOR-20/api/step99-upstream-balloon-hinting-stop-preboot-baseline.md`
  - `run/WOR-20/api/step93-upstream-hotplug-get-preboot-baseline.md`

## Derived baseline facts

- `GET /balloon/statistics` is runtime-only.
- The route takes no request body, so a preboot request exercises only the state-machine guard.
- Neighboring runtime-only Firecracker read routes use the canonical preboot invalid-state envelope:
  - status: `400`
  - code: `InvalidState`
  - message: `The requested operation is not supported before starting the microVM.`

## Local parity decision

- This slice can truthfully seed a route-level capture for the preboot `GET /balloon/statistics` invalid-state path by:
  - reusing the pinned runtime-only classification from step 11,
  - sending an otherwise valid empty-body read request through the full router,
  - and asserting the same canonical preboot invalid-state envelope already pinned for neighboring runtime-only routes.

- The exact preboot envelope is an inference from the pinned Firecracker runtime-only classification plus the existing upstream-backed `OperationNotSupportedPreBoot` captures listed above.
