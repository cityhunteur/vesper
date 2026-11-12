# Generated Docs

This directory is reserved for generated documentation and reports that are durable enough to keep in the repository.

Possible uses:
- generated route coverage reports,
- generated quality score snapshots,
- generated API examples,
- generated Firecracker surface-stub matrices such as `firecracker-api-surface-stub-matrix.json`,
- generated Firecracker route-behavior matrices such as `firecracker-api-behavior-matrix.json`.
- the Backend A behaviour-matrix replay report `backend-a-behavior-replay.json` (`make systemvm-replay-publish`, ADR 0007 slice 6).

Do not store transient issue-run artifacts here; those belong under `.symphony/runs/` or `run/`.
