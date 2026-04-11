# Step 127 upstream vm-config network runtime baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: `GET /vm/config` network-interface export surface after a successful runtime patch

## Source packet

- `run/WOR-20/api/step15-upstream-network-update-baseline.md`
  - `PATCH /network-interfaces/{iface_id}` is runtime-only.
  - accepted runtime patches update an existing interface's live rate-limiter configuration.
- `run/WOR-20/api/step61-upstream-network-create-baseline.md`
  - full configuration export includes configured network interfaces in `/vm/config`.
- `src/api.rs`
  - full VM config export serializes stored network-interface state without a boot-state-specific branch.

## Derived baseline facts

- `GET /vm/config` should remain callable after boot after a successful runtime network-interface patch.
- The booted full-config export should preserve the stored interface identity and host device name while reflecting the updated runtime RX rate-limiter configuration.

## Local parity decision

- This slice is harness-first, not a handler change: Vesper already proves the runtime network patch/export path through the full router, but the generated route-level behavior matrix still only recorded the preboot network export case.
