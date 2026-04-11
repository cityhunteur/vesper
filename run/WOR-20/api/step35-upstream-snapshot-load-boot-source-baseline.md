# Step 35 upstream baseline notes: snapshot-load rejection after boot-source configuration

Pinned Firecracker baseline revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`

Primary sources inspected:

- `docs/snapshotting/snapshot-support.md`
- `src/vmm/src/rpc_interface.rs`

Captured facts:

- The pinned snapshotting docs state that `LoadSnapshot` is allowed only before boot and before the microVM is configured; the only resources that can be configured beforehand are the logger and metrics systems.
- In the pinned `rpc_interface.rs`, `PrebootApiController` documents `boot_path` as the flag set by configuring boot-specific resources, and loading from snapshot is not allowed once that flag is true.
- Boot-source configuration is a boot-specific resource and therefore belongs to the set that blocks `LoadSnapshot`.

Implication for the local Step 35 slice:

- After a successful `PUT /boot-source`, a valid `PUT /snapshot/load` request should be rejected by the state guard with the snapshot-load invalid-state envelope rather than reaching the placeholder path.
