# Upstream balloon preboot read surfaces baseline note

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- upstream baseline anchor: `run/WOR-20/api/step9-upstream-balloon-config-baseline.md`

## Why this step exists

- The checked-in behavior capture registry already records pinned `GET /balloon` preboot missing-device and preboot configured-defaults cases.
- The local contract suite already exercised those paths inside `balloon_config_matches_pinned_store_behavior`, but both reads were embedded in a broader store/export flow and were not surfaced as dedicated full-router proofs.

## Baseline interpretation carried forward

- Before boot, `GET /balloon` returns `400 InvalidRequest` with `Balloon config error: No balloon device found.` when no balloon device has been configured.
- After preboot `PUT /balloon` with a minimal config, `GET /balloon` returns the stored descriptor with Firecracker defaults for omitted optional fields:
  - `stats_polling_interval_s = 0`
  - `free_page_hinting = false`
  - `free_page_reporting = false`
- This step does not introduce a new upstream behavior claim; it makes the already-pinned preboot `GET /balloon` read surfaces explicit in the local route-level proof bundle.
