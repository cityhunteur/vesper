# Upstream MMDS runtime configured-store baseline note

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- upstream baseline anchor: `run/WOR-20/api/step7-upstream-mmds-baseline.md`

## Why this step exists

- The checked-in behavior capture registry already records a pinned `GET /mmds` `runtime_configured_store` success case.
- The local contract suite also already exercised that path inside `mmds_flow_matches_pinned_baseline`, but the proof was embedded in a broader MMDS flow and the parity audit text did not call the booted configured-store read out explicitly.

## Baseline interpretation carried forward

- Firecracker serves MMDS data at runtime only when MMDS networking has been configured against an existing network interface before boot.
- Once the datastore is initialized and MMDS networking is configured, booted `GET /mmds` returns the stored JSON object rather than rejecting the request.
- This step does not introduce a new upstream behavior claim; it makes the already-pinned runtime configured-store read explicit in the local route-level proof and issue bundle.
