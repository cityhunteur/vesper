# Step 71 upstream baseline notes: vsock invalid guest-CID rejection

Pinned Firecracker baseline revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`

Primary sources inspected:

- `third_party/firecracker/firecracker.yaml`
- `run/WOR-20/api/step20-upstream-vsock-store-baseline.md`

Captured facts:

- The pinned OpenAPI surface declares `PUT /vsock` as a bad-input `400` route when vsock creation or update receives invalid input.
- In the pinned `Vsock` schema, `guest_cid` is required and has `minimum: 3`.
- The earlier step-20 baseline note already established that Firecracker parses `PUT /vsock` into the upstream vsock device config model and accepts the deprecated `vsock_id` field only as an optional parsed input.

Inference from those sources:

- A `PUT /vsock` request with `guest_cid < 3` belongs to the pinned bad-input branch and should be rejected before the vsock configuration is stored.

Implication for the local Step 71 slice:

- Capture the already-implemented `PUT /vsock` invalid-input branch on the full router with the current Firecracker-shaped `400 InvalidRequest` envelope and the existing controller detail string `Guest CID must be greater than or equal to 3.`.
