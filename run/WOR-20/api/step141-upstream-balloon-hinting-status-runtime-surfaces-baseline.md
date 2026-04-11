# Upstream balloon hinting status runtime read surfaces baseline note

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- upstream baseline anchor: `run/WOR-20/api/step12-upstream-balloon-hinting-status-baseline.md`

## Why this step exists

- The checked-in behavior capture registry already records pinned runtime `GET /balloon/hinting/status` missing-device, hinting-disabled, and idle-status cases.
- The local contract suite already exercised those paths, but they were still grouped inside broader hinting-status tests instead of being surfaced as explicit route-level read proofs.

## Baseline interpretation carried forward

- After boot, `GET /balloon/hinting/status` returns `400 InvalidRequest` with `Balloon update error: Failed perform action on device: Device not found` when no balloon device exists.
- After boot with hinting disabled, `GET /balloon/hinting/status` returns `400 InvalidRequest` with `Balloon update error: Balloon: Attempting to use hinting when not enabled`.
- After boot with hinting enabled before any control command runs, `GET /balloon/hinting/status` returns `200` and reports the idle status shape with `host_cmd = 0` and `guest_cmd = null`.
- This step does not introduce a new upstream behavior claim; it makes the already-pinned runtime `GET /balloon/hinting/status` read surfaces explicit in the local route-level proof bundle.
