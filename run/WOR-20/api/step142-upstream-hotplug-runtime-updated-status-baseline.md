# Upstream baseline note: `GET /hotplug/memory` after a runtime resize request

- Firecracker baseline revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- Capture date: `2026-04-07`
- Source review method: repo-local baseline inference from `run/WOR-20/api/step6-upstream-hotplug-baseline.md` plus the pinned `MemoryHotplugStatus` schema in `third_party/firecracker/firecracker.yaml`

## Findings

- `run/WOR-20/api/step6-upstream-hotplug-baseline.md` already records that `PATCH /hotplug/memory` maps to `UpdateMemoryHotplugSize`, is postboot-only, and the runtime controller applies it as a resize request.
- The pinned Firecracker OpenAPI schema still exposes `GET /hotplug/memory` as the runtime `MemoryHotplugStatus` read path, including `block_size_mib`, `slot_size_mib`, `total_size_mib`, `plugged_size_mib`, and `requested_size_mib`.
- Combining those pinned facts, a runtime `GET /hotplug/memory` read taken after a successful resize request should surface the requested target through `requested_size_mib` while preserving the configured geometry fields in the same response shape.

## Local parity implication

- Vesper should preserve the existing initial runtime hotplug status shape and additionally report the updated `requested_size_mib` after a successful booted `PATCH /hotplug/memory` request.
- This note extends the already-pinned hotplug read surface without claiming a new handler behavior outside the existing runtime read and resize flow.
