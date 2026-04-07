# WOR-20 step145 upstream hotplug invalid-total-size baseline notes

- Firecracker baseline revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- Capture date: `2026-04-07`
- Capture method: issue-local follow-up to `run/WOR-20/api/step6-upstream-hotplug-baseline.md`, which already pinned the upstream hotplug validator sources and constraints for `MemoryHotplugConfig`

## Findings

- `PUT /hotplug/memory` remains a preboot-only route; step6 already tied the request to Firecracker's `SetMemoryHotplugDevice` parsing and validation path.
- The pinned upstream hotplug validator requires `total_size_mib` to be at least `slot_size_mib` and a multiple of `slot_size_mib`.
  - Source lineage already recorded in step6: `vmm/src/vmm_config/memory_hotplug.rs`
  - Default `slot_size_mib` is `128`, so a request body with `{"total_size_mib":130}` is rejected by the upstream constraint.
- This step does not widen the control-plane semantics beyond the step6 parity note; it makes the already-pinned invalid-total-size branch explicit in the route-level behavior bundle.

## Local parity implication

- Vesper should reject preboot `PUT /hotplug/memory` requests whose `total_size_mib` is not a valid multiple of the effective slot size with a deterministic `400 InvalidRequest` envelope.
