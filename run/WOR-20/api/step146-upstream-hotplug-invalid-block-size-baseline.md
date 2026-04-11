# WOR-20 step146 upstream hotplug invalid-block-size baseline notes

- Firecracker baseline revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- Capture date: `2026-04-07`
- Capture method: issue-local follow-up to `run/WOR-20/api/step6-upstream-hotplug-baseline.md`, which already pinned the upstream hotplug validator sources and constraints for `MemoryHotplugConfig`

## Findings

- `PUT /hotplug/memory` remains a preboot-only route; step6 already tied the request to Firecracker's `SetMemoryHotplugDevice` parsing and validation path.
- The pinned upstream hotplug validator requires `block_size_mib` to be at least `2` MiB and a power of two.
  - Source lineage already recorded in step6: `vmm/src/vmm_config/memory_hotplug.rs`
  - A request body with `{"block_size_mib":3,"total_size_mib":1024}` violates that validator branch before the later slot-size and total-size checks run.
- This step does not widen the control-plane semantics beyond the step6 parity note; it makes the already-pinned invalid-block-size branch explicit in the route-level behavior bundle.

## Local parity implication

- Vesper should reject preboot `PUT /hotplug/memory` requests whose `block_size_mib` is below the pinned minimum or not a power of two with a deterministic `400 InvalidRequest` envelope.
