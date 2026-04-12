# WOR-106 hotplug-memory endpoint baseline packet

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: `GET /hotplug/memory`, `PUT /hotplug/memory`, `PATCH /hotplug/memory`, and the family-level `/vm/config` export fact for the stored `memory-hotplug` block
- GitHub source issue: `#108`
- umbrella context: GH `#2` / WOR-20

## Source packet

- `run/WOR-20/api/step6-upstream-hotplug-baseline.md`
  - pins the route-family semantics: `PUT /hotplug/memory` is preboot-only, while `GET` and `PATCH` are postboot-only
  - records the upstream `MemoryHotplugConfig` defaults and validation rules
  - records the runtime `MemoryHotplugStatus` field set and the `/vm/config` export fact
- `run/WOR-20/api/step93-upstream-hotplug-get-preboot-baseline.md`
  - grounds the preboot `GET /hotplug/memory` invalid-state envelope in the pinned Firecracker runtime-only controller path
- `run/WOR-20/api/step94-upstream-hotplug-patch-preboot-baseline.md`
  - grounds the preboot `PATCH /hotplug/memory` invalid-state envelope in the pinned Firecracker runtime-only controller path
- `run/WOR-20/api/step142-upstream-hotplug-runtime-updated-status-baseline.md`
  - pins the runtime `GET /hotplug/memory` read-after-resize fact for `requested_size_mib`
- `run/WOR-20/api/step145-upstream-hotplug-invalid-total-size-baseline.md`
  - pins the preboot `PUT /hotplug/memory` invalid-total-size validator branch
- `run/WOR-20/api/step146-upstream-hotplug-invalid-block-size-baseline.md`
  - pins the preboot `PUT /hotplug/memory` invalid-block-size validator branch
- `run/WOR-20/api/step147-upstream-hotplug-invalid-requested-size-baseline.md`
  - pins the runtime `PATCH /hotplug/memory` invalid-requested-size validator branch
- `run/WOR-20/api/step112-upstream-vm-config-memory-hotplug-runtime-baseline.md`
  - pins the runtime `/vm/config` export fact for the stored `memory-hotplug` block after boot

## Repo evidence crosswalk

- `tests/api_contract.rs`
  - `memory_hotplug_get_returns_initial_runtime_status_on_full_router`
  - `memory_hotplug_get_returns_updated_requested_size_after_patch_on_full_router`
  - `vm_config_memory_hotplug_export_remains_available_after_boot_on_full_router`
  - `memory_hotplug_get_rejects_preboot_reads_on_full_router`
  - `memory_hotplug_patch_rejects_preboot_updates_on_full_router`
  - `memory_hotplug_put_rejects_postboot_updates_on_full_router`
  - `memory_hotplug_put_rejects_invalid_total_size_preboot_on_full_router`
  - `memory_hotplug_put_rejects_invalid_block_size_preboot_on_full_router`
  - `memory_hotplug_patch_rejects_invalid_requested_size_on_full_router`
- `docs/specs/firecracker-api-behavior-captures.yaml`
  - `GET /hotplug/memory`
    - `preboot_invalid_state`
    - `runtime_initial_status`
    - `runtime_updated_requested_size`
  - `PUT /hotplug/memory`
    - `put_valid_preboot`
    - `invalid_block_size_preboot`
    - `invalid_total_size_preboot`
    - `postboot_invalid_state`
  - `PATCH /hotplug/memory`
    - `preboot_invalid_state`
    - `patch_valid_runtime`
    - `invalid_requested_size_runtime`
  - `GET /vm/config`
    - `memory_hotplug_export_preboot`
    - `memory_hotplug_export_runtime`

## Registry truth

- `docs/specs/firecracker-api-baseline-captures.yaml` remains empty by design because `src/api_matrix.rs` only uses that registry for matrix-backed placeholder routes.
- `/hotplug/memory` is already a concrete handler family, so adding it to the placeholder capture registry would be incorrect even though the family still needs a durable endpoint-level packet.
- For this route family, the truthful endpoint packet is this issue-local note plus the route-level behavior captures and full-router contract coverage listed above.

## Local parity decision

- This slice does not claim new `/hotplug/memory` handler behavior.
- This slice packages the existing pinned upstream facts and repo evidence for the hotplug-memory family into a single issue-local endpoint packet, clarifies the placeholder-registry boundary, and keeps GH `#2` open for broader endpoint-level parity work.
