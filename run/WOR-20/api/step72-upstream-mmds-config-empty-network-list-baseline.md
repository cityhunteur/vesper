# Step 72 upstream baseline notes: MMDS config empty network-interface list rejection

Pinned Firecracker baseline revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`

Primary sources inspected:

- `run/WOR-20/api/step7-upstream-mmds-baseline.md`
- `docs/specs/design-v0.baselines.lock.yaml`

Captured facts:

- The pinned MMDS baseline establishes that `PUT /mmds/config` is a preboot route.
- Firecracker MMDS config requires at least one network interface ID.
- MMDS action errors for invalid config in this slice are surfaced as HTTP `400`.

Inference from those sources:

- A `PUT /mmds/config` request with an empty `network_interfaces` list belongs to the pinned bad-input branch and should be rejected before MMDS networking configuration is stored.

Implication for the local Step 72 slice:

- Capture the already-implemented `PUT /mmds/config` validation branch on the full router with the current Firecracker-shaped `400 InvalidRequest` envelope and the existing controller detail string `The list of network interface IDs that allow forwarding MMDS requests is empty.`.
