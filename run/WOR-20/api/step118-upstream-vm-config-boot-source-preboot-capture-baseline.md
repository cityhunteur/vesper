# Step 118 upstream vm-config boot-source preboot capture baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: preboot `GET /vm/config` boot-source export capture registration

## Source packet

- `run/WOR-20/api/step22-upstream-boot-source-store-baseline.md`
  - documents that preboot `PUT /boot-source` stores the configured kernel path, optional initrd path, and boot arguments
  - documents that `/vm/config` exports the stored boot source through the `boot-source` section
- `tests/api_contract.rs`
  - `boot_source_put_matches_pinned_store_behavior` already proves the full-router preboot store plus `/vm/config` export path

## Derived baseline facts

- The repo already carries pinned upstream evidence plus a full-router proof for the preboot `/vm/config` boot-source export surface.
- The existing proof uses ephemeral tempfile paths, so the capture should pin the exported path fields by type and preserve the exact boot-args string.

## Local parity decision

- Keep this slice harness-only: register the missing preboot boot-source export case for `GET /vm/config`, regenerate the matrix, and preserve the evidence bundle without changing handler logic.
