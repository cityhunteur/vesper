# Upstream balloon runtime read surfaces baseline note

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- upstream baseline anchor: `run/WOR-20/api/step10-upstream-balloon-runtime-update-baseline.md`

## Why this step exists

- The checked-in behavior capture registry already records pinned `GET /balloon` runtime missing-device and runtime updated-target cases.
- The local contract suite already exercised those paths inside broader balloon runtime flow tests, but the read proofs were not surfaced as dedicated full-router route checks.

## Baseline interpretation carried forward

- After boot, `GET /balloon` returns `400 InvalidRequest` with `Internal VMM error: Failed perform action on device: Device not found` when no balloon device exists.
- After a successful runtime `PATCH /balloon`, `GET /balloon` returns the updated balloon target while preserving the configured `deflate_on_oom` flag.
- This step does not introduce a new upstream behavior claim; it makes the already-pinned runtime `GET /balloon` read surfaces explicit in the local route-level proof bundle.
