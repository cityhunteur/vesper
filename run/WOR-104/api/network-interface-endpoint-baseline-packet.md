# WOR-104 network-interface endpoint baseline packet

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: `PUT /network-interfaces/{iface_id}`, `PATCH /network-interfaces/{iface_id}`, and the family-level `/vm/config` export facts for stored and runtime-updated network interfaces
- GitHub source issue: `#109`
- umbrella context: GH `#2` / WOR-20

## Source packet

- `run/WOR-20/api/step61-upstream-network-create-baseline.md`
  - pins the preboot create path and the preboot `/vm/config` export fact
- `run/WOR-20/api/step62-upstream-network-create-conflict-baseline.md`
  - pins the duplicate guest-MAC and duplicate host tap reuse failures for preboot `PUT /network-interfaces/{iface_id}`
- `run/WOR-20/api/step15-upstream-network-update-baseline.md`
  - pins the shared path/body `iface_id` mismatch string and the runtime patch missing-interface fault
  - records that `PATCH /network-interfaces/{iface_id}` is runtime-only and updates rate limiter state on an existing interface
- `run/WOR-20/api/step102-upstream-network-patch-preboot-baseline.md`
  - grounds the preboot `PATCH /network-interfaces/{iface_id}` invalid-state envelope in the pinned runtime-only controller path
- `run/WOR-20/api/step115-upstream-vm-config-network-preboot-capture-baseline.md`
  - pins the preboot `/vm/config` export capture for a stored network interface
- `run/WOR-20/api/step127-upstream-vm-config-network-runtime-baseline.md`
  - pins the runtime `/vm/config` export fact after a successful runtime network patch

## Repo evidence crosswalk

- `tests/api_contract.rs`
  - `network_put_matches_pinned_preboot_behavior`
  - `network_put_rejects_path_body_id_mismatch_on_full_router`
  - `network_put_rejects_pinned_preboot_conflicts`
  - `network_patch_rejects_preboot_updates_on_full_router`
  - `network_patch_matches_pinned_runtime_behavior`
- `docs/specs/firecracker-api-behavior-captures.yaml`
  - `PUT /network-interfaces/{iface_id}`
    - `id_mismatch_preboot`
    - `put_valid_preboot`
    - `duplicate_guest_mac_preboot`
    - `duplicate_host_dev_name_preboot`
  - `PATCH /network-interfaces/{iface_id}`
    - `preboot_invalid_state`
    - `id_mismatch_runtime`
    - `missing_interface_runtime`
    - `update_existing_runtime`
  - `GET /vm/config`
    - `network_interfaces_export_preboot`
    - `network_interfaces_export_runtime`

## Registry truth

- `docs/specs/firecracker-api-baseline-captures.yaml` remains empty by design because `src/api_matrix.rs` only uses that registry for matrix-backed placeholder routes.
- `/network-interfaces/{iface_id}` is already a concrete handler family, so adding it to the placeholder capture registry would be incorrect even though the family still needs a durable endpoint packet.
- For this route family, the truthful endpoint packet is this issue-local note plus the route-level behavior captures and full-router contract coverage listed above.

## Local parity decision

- This slice does not claim new `/network-interfaces/{iface_id}` handler behavior.
- This slice packages the existing pinned upstream facts and repo evidence for the network-interface family into a single issue-local endpoint packet, keeps the placeholder-registry boundary explicit, and leaves GH `#2` open for broader endpoint-level parity work.
