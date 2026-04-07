# Step 33 Upstream Snapshot-Load Non-Boot Config Baseline

Pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`

Primary sources inspected:

- `src/vmm/src/rpc_interface.rs`
- `src/firecracker/src/api_server/request/snapshot.rs`

Observed pinned facts relevant to snapshot load after logger, metrics, and serial configuration:

1. In `PrebootApiController::handle_preboot_request()`:
   - `ConfigureLogger` updates logger state and returns success
   - `ConfigureMetrics` initializes metrics and returns success
   - `ConfigureSerial` stores `serial_out_path` and returns success
2. None of those three preboot actions marks the `boot_path` guard.
3. `LoadSnapshotError::LoadSnapshotNotAllowed` is tied to boot-specific resources tracked through `boot_path`.
4. Therefore the pinned baseline allows `PUT /snapshot/load` to remain reachable after logger, metrics, and serial configuration, before any later snapshot implementation result is produced.

This slice only covers that low-risk reachability fact. It does not claim snapshot restore functionality or broader snapshot runtime parity.
