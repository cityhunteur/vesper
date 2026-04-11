# Step 119 upstream vm-config pmem preboot capture baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: preboot `GET /vm/config` pmem export capture registration

## Source packet

- `run/WOR-20/api/step16-upstream-pmem-create-baseline.md`
  - documents that preboot `PUT /pmem/{id}` defaults `root_device` and `read_only` to `false`
  - documents that replacing an existing pmem config by id is allowed
- `tests/api_contract.rs`
  - `pmem_put_matches_pinned_root_device_behavior` already proves the full-router preboot store plus `/vm/config` export path for the defaulted boolean shape

## Derived baseline facts

- The repo already carries pinned upstream evidence plus a full-router proof for the preboot `/vm/config` pmem export surface.
- The current proof directly pins the defaulted `root_device=false` and `read_only=false` export behavior for a stored pmem config.

## Local parity decision

- Keep this slice harness-only: register the missing preboot pmem export case for `GET /vm/config`, regenerate the matrix, and preserve the evidence bundle without changing handler logic.
