# Upstream baseline note: `PATCH /hotplug/memory` before boot

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- source review method: repo-local baseline inference from `run/WOR-20/api/step6-upstream-hotplug-baseline.md` plus the pinned `OperationNotSupportedPreBoot` captures already recorded for other runtime-only routes

## Findings

- `run/WOR-20/api/step6-upstream-hotplug-baseline.md` already records that `PATCH /hotplug/memory` maps to `UpdateMemoryHotplugSize`, is postboot-only, and is rejected by the preboot controller.
- Existing pinned WOR-20 captures for other runtime-only routes already establish the canonical preboot controller envelope: `VmmActionError::OperationNotSupportedPreBoot` renders as `The requested operation is not supported before starting the microVM.`
- The local router already emits that canonical preboot invalid-state message for documented postboot-only routes, including the hotplug resize route guarded before the concrete handler executes.

## Relevance to WOR-20

- This is an inference rather than a fresh direct source capture for the specific hotplug resize route, but it grounds the expected preboot `PATCH /hotplug/memory` envelope in the same pinned runtime-only controller pattern already established for adjacent postboot-only endpoints.
- The step-94 contract proof verifies that the local router matches that pinned Firecracker-shaped `400 InvalidState` envelope on the full route.
