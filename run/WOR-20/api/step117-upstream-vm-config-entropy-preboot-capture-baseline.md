# Step 117 upstream vm-config entropy preboot capture baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: preboot `GET /vm/config` entropy export capture registration

## Source packet

- `run/WOR-20/api/step21-upstream-entropy-store-baseline.md`
  - documents that preboot `PUT /entropy` stores the configured entropy rate limiter
  - documents that `/vm/config` exports the stored entropy shape through the `entropy` section
- `tests/api_contract.rs`
  - `entropy_put_matches_pinned_store_behavior` already proves the full-router preboot store plus `/vm/config` export path

## Derived baseline facts

- The repo already carries pinned upstream evidence plus a full-router proof for the preboot `/vm/config` entropy export surface.
- The remaining gap is only the missing generated behavior-matrix capture entry for that already-proven path.

## Local parity decision

- Keep this slice harness-only: register the missing preboot entropy export case for `GET /vm/config`, regenerate the matrix, and preserve the evidence bundle without changing handler logic.
