# Step 102 upstream network-patch preboot invalid-state baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: preboot `PATCH /network-interfaces/{iface_id}` invalid-state behavior

## Source packet

- `run/WOR-20/api/step15-upstream-network-update-baseline.md`
  - records that `PATCH /network-interfaces/{iface_id}` is runtime-only.
  - records that runtime net patching updates an existing interface's rate-limiter configuration.
- `src/firecracker/src/api_server/request/net.rs`
  - `parse_patch_net()` routes accepted requests through the runtime network-update path.
- pinned neighboring WOR-20 invalid-state captures:
  - `run/WOR-20/api/step101-upstream-drive-patch-preboot-baseline.md`
  - `run/WOR-20/api/step100-upstream-balloon-stats-get-preboot-baseline.md`
  - `run/WOR-20/api/step94-upstream-hotplug-patch-preboot-baseline.md`

## Derived baseline facts

- `PATCH /network-interfaces/{iface_id}` is runtime-only.
- A request body containing a matching `iface_id` and a valid rate-limiter update is valid for the runtime patch path, so a preboot request with that body exercises only the state-machine guard.
- Neighboring runtime-only Firecracker routes use the canonical preboot invalid-state envelope:
  - status: `400`
  - code: `InvalidState`
  - message: `The requested operation is not supported before starting the microVM.`

## Local parity decision

- This slice can truthfully seed a route-level capture for the preboot `PATCH /network-interfaces/{iface_id}` invalid-state path by:
  - reusing the pinned runtime-only classification from step 15,
  - sending an otherwise valid `{"iface_id":"eth0","rx_rate_limiter":{"bandwidth":{"size":1024,"refill_time":1000}}}` patch request through the full router,
  - and asserting the same canonical preboot invalid-state envelope already pinned for neighboring runtime-only routes.

- The exact preboot envelope is an inference from the pinned Firecracker runtime-only classification plus the existing upstream-backed `OperationNotSupportedPreBoot` captures listed above.
