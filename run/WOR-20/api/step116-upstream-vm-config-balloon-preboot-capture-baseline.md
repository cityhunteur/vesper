# Step 116 upstream vm-config balloon preboot capture baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: preboot `GET /vm/config` balloon export capture registration

## Source packet

- `run/WOR-20/api/step9-upstream-balloon-config-baseline.md`
  - `PUT /balloon` is a preboot operation and stores Firecracker defaults for omitted optional fields.
  - `/vm/config` includes `balloon` when a balloon device has been configured.
- `tests/api_contract.rs`
  - `balloon_config_matches_pinned_store_behavior` already proves the full-router preboot balloon store plus `/vm/config` export path.

## Derived baseline facts

- The repo already has pinned upstream evidence and router-level proof for the preboot `/vm/config` balloon export shape.
- The remaining gap is only in the generated behavior-capture registry, which currently records other `/vm/config` export blocks but not the already-proven preboot balloon one.

## Local parity decision

- This slice is harness-only: add the missing preboot capture entry and regenerate the behavior matrix without changing any handler or test behavior.
