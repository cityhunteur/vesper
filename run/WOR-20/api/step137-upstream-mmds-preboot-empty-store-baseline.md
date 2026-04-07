# Upstream MMDS preboot empty-store baseline note

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- upstream baseline anchor: `run/WOR-20/api/step7-upstream-mmds-baseline.md`

## Why this step exists

- The checked-in behavior capture registry already records a pinned `GET /mmds` `preboot_empty_store` success case.
- The local contract suite already exercised that path inside `mmds_flow_matches_pinned_baseline`, but the proof was embedded in a broader MMDS flow and the parity audit text still did not name the empty-store `{}` surface explicitly.

## Baseline interpretation carried forward

- Before the MMDS datastore has been initialized, Firecracker serializes `GET /mmds` as `{}` rather than JSON `null`.
- This empty-store surface is valid before boot and does not require MMDS networking configuration.
- This step does not introduce a new upstream behavior claim; it makes the already-pinned preboot empty-store success path explicit in the local route-level proof and issue bundle.
