# Step 73 upstream baseline notes: MMDS config unknown network-interface rejection

Pinned Firecracker baseline revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`

Primary sources inspected:

- `run/WOR-20/api/step7-upstream-mmds-baseline.md`
- `third_party/firecracker/firecracker.yaml`
- `docs/specs/design-v0.baselines.lock.yaml`

Captured facts:

- The pinned MMDS baseline establishes that `PUT /mmds/config` is a preboot route.
- Firecracker MMDS config requires at least one network interface ID and all referenced interface IDs must already exist.
- The pinned OpenAPI description says the `network_interfaces` entries "must be valid at the time of this request."
- MMDS action errors for invalid config in this slice are surfaced as HTTP `400`.

Inference from those sources:

- A `PUT /mmds/config` request that references a non-existent interface ID belongs to the pinned bad-input branch and should be rejected before MMDS networking configuration is stored.

Implication for the local Step 73 slice:

- Capture the already-implemented `PUT /mmds/config` validation branch on the full router with the current Firecracker-shaped `400 InvalidRequest` envelope and the existing controller detail string `The list of network interface IDs provided contains at least one ID that does not correspond to any existing network interface.`.
