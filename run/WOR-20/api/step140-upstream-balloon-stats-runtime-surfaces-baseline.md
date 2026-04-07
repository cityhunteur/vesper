# Upstream balloon statistics runtime read surfaces baseline note

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- upstream baseline anchor: `run/WOR-20/api/step11-upstream-balloon-stats-baseline.md`

## Why this step exists

- The checked-in behavior capture registry already records pinned runtime `GET /balloon/statistics` missing-device, stats-disabled, and enabled-stats cases.
- The local contract suite already exercised those paths inside broader balloon statistics tests, but the read proofs were not surfaced as dedicated full-router route checks.

## Baseline interpretation carried forward

- After boot, `GET /balloon/statistics` returns `400 InvalidRequest` with `Internal VMM error: Failed perform action on device: Device not found` when no balloon device exists.
- After boot with statistics disabled, `GET /balloon/statistics` returns `400 InvalidRequest` with `Internal VMM error: Balloon: Received stats query when stats are disabled.`
- After boot with statistics enabled preboot, `GET /balloon/statistics` returns `200` and reports the configured target/actual balloon sizes.
- This step does not introduce a new upstream behavior claim; it makes the already-pinned runtime `GET /balloon/statistics` read surfaces explicit in the local route-level proof bundle.
