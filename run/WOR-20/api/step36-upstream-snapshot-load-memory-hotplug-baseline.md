# Step 36 upstream baseline notes: snapshot-load rejection after memory-hotplug configuration

Pinned Firecracker baseline revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`

Primary sources inspected:

- `docs/snapshotting/snapshot-support.md`
- `src/vmm/src/rpc_interface.rs`

Captured facts:

- The pinned snapshotting docs state that `LoadSnapshot` is allowed only before boot and before configuring resources other than logger and metrics.
- In the pinned `rpc_interface.rs`, `PrebootApiController` documents `boot_path` as the flag set by configuring boot-specific resources, and loading from snapshot is not allowed once that flag is true.
- Memory-hotplug configuration is a preboot resource configuration and therefore belongs to the set that blocks `LoadSnapshot`.

Implication for the local Step 36 slice:

- After a successful `PUT /hotplug/memory`, a valid `PUT /snapshot/load` request should be rejected by the state guard with the snapshot-load invalid-state envelope rather than reaching the placeholder path.
