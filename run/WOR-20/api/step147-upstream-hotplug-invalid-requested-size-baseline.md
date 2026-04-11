# WOR-20 step147 upstream hotplug invalid-requested-size baseline notes

- Firecracker baseline revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- Capture date: `2026-04-07`
- Capture method: issue-local follow-up to `run/WOR-20/api/step6-upstream-hotplug-baseline.md`, which already pinned the upstream hotplug validator sources and constraints for `MemoryHotplugConfig`

## Findings

- `PATCH /hotplug/memory` remains a runtime-only route; step6 already tied the request to Firecracker's `UpdateMemoryHotplugSize` runtime path.
- The pinned upstream hotplug validator requires `requested_size_mib` to be a multiple of `block_size_mib` and no larger than `total_size_mib`.
  - Source lineage follows the same upstream validator family already noted in step6: `vmm/src/vmm_config/memory_hotplug.rs`
  - With the default `block_size_mib = 2` and `total_size_mib = 1024`, a runtime patch body of `{"requested_size_mib":513}` violates the resize validator branch.
- This step does not widen the control-plane semantics beyond the step6 parity note; it makes the already-pinned invalid-requested-size runtime branch explicit in the route-level behavior bundle.

## Local parity implication

- Vesper should reject runtime `PATCH /hotplug/memory` requests whose `requested_size_mib` is not aligned to the configured block size or exceeds the configured total size with a deterministic `400 InvalidRequest` envelope.
