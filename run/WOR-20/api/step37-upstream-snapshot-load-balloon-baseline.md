# Step 37 upstream baseline notes: snapshot-load rejection after balloon configuration

Pinned Firecracker baseline revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`

Primary sources inspected:

- `docs/snapshotting/snapshot-support.md`
- `src/vmm/src/rpc_interface.rs`

Captured facts:

- The pinned snapshotting docs state that `LoadSnapshot` is allowed only before boot and before configuring resources other than logger and metrics.
- In the pinned `rpc_interface.rs`, `PrebootApiController` documents `boot_path` as the flag that blocks snapshot loading once boot-specific resources have been configured.
- Inference from those sources: balloon configuration is a preboot resource configuration and is not logger or metrics, so it belongs to the configured-resource set that blocks `LoadSnapshot`.

Implication for the local Step 37 slice:

- After a successful `PUT /balloon`, a valid `PUT /snapshot/load` request should be rejected by the state guard with the snapshot-load invalid-state envelope rather than reaching the placeholder path.
