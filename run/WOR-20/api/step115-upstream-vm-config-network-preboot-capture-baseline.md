# Step 115 upstream vm-config network preboot capture baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: preboot `GET /vm/config` network-interface export capture registration

## Source packet

- `run/WOR-20/api/step61-upstream-network-create-baseline.md`
  - Preboot `PUT /network-interfaces/{iface_id}` should accept a valid request body and return `204 No Content`.
  - After a successful preboot create request, `/vm/config` should include the stored network-interface configuration.
- `tests/api_contract.rs`
  - `network_put_matches_pinned_preboot_behavior` already proves the full-router preboot create plus `/vm/config` export path.

## Derived baseline facts

- The repo already has pinned upstream evidence and router-level proof for the preboot `/vm/config` network-interface export shape.
- The remaining gap is only in the generated behavior-capture registry, which currently records other `/vm/config` export blocks but not the already-proven preboot network-interface one.

## Local parity decision

- This slice is harness-only: add the missing preboot capture entry and regenerate the behavior matrix without changing any handler or test behavior.
