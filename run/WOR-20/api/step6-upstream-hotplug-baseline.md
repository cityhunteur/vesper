# WOR-20 step6 upstream hotplug baseline notes

- Firecracker baseline revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- Capture date: `2026-04-06`
- Capture method: inspected pinned upstream source via GitHub repository paths for Firecracker issue-parity review

## Findings

- `PUT /hotplug/memory` is preboot-only.
  - Source: `firecracker/src/api_server/request/hotplug/memory.rs`
  - The request maps to `SetMemoryHotplugDevice`, which the preboot controller accepts and the runtime controller rejects with the postboot operation-not-supported path.
- `GET /hotplug/memory` is postboot-only.
  - Sources: `firecracker/src/api_server/request/hotplug/memory.rs`, `vmm/src/rpc_interface.rs`
  - The request maps to `GetMemoryHotplugStatus`, which the preboot controller rejects and the runtime controller serves from virtio-mem status.
- `PATCH /hotplug/memory` is postboot-only.
  - Sources: `firecracker/src/api_server/request/hotplug/memory.rs`, `vmm/src/rpc_interface.rs`
  - The request maps to `UpdateMemoryHotplugSize`, which the preboot controller rejects and the runtime controller applies as a resize request.
- `MemoryHotplugConfig` defaults and validation are defined in `vmm/src/vmm_config/memory_hotplug.rs`.
  - `block_size_mib` default: `2`
  - `slot_size_mib` default: `128`
  - `block_size_mib` must be at least `2` and a power of two.
  - `slot_size_mib` must be at least `128` and a multiple of `block_size_mib`.
  - `total_size_mib` must be at least `slot_size_mib` and a multiple of `slot_size_mib`.
- Virtio-mem runtime status shape is defined in `vmm/src/devices/virtio/mem/device.rs`.
  - Fields: `block_size_mib`, `total_size_mib`, `slot_size_mib`, `plugged_size_mib`, `requested_size_mib`
  - Initial status after configuration reports `plugged_size_mib = 0` and `requested_size_mib = 0`.
- `/vm/config` includes `memory-hotplug` in the upstream full configuration export.
  - Sources: `vmm/src/resources.rs`, `vmm/src/lib.rs`

## Local parity implications

- Vesper should advertise `/hotplug/memory` semantics as `PB` for `PUT` and `PO` for `GET` and `PATCH`.
- `GET /vm/config` should serialize the configured `memory-hotplug` block once the preboot config is accepted.
- Contract coverage should assert the Firecracker defaults and initial runtime status shape even while the backend remains a surface stub.
