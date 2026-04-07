# Step 68 upstream baseline notes: snapshot-load rejection after MMDS configuration

Pinned Firecracker baseline revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`

Primary sources inspected:

- `docs/snapshotting/snapshot-support.md`
- prior pinned-source packet captured in `run/WOR-20/api/step7-upstream-mmds-baseline.md`

Captured facts:

- The pinned snapshotting docs state that `LoadSnapshot` is allowed only before boot and before the microVM is configured; the only resources that can be configured beforehand are the logger and metrics systems.
- The pinned MMDS source packet already established that `PUT /mmds/config` is a preboot route that stores MMDS networking configuration and depends on pre-existing network-interface state.
- Inference from those pinned sources: MMDS networking configuration is not part of the logger-and-metrics exception set, so it belongs to the configured-resource set that blocks `LoadSnapshot`.

Implication for the local Step 68 slice:

- After a successful `PUT /mmds/config`, a valid `PUT /snapshot/load` request should be rejected by the state guard with the snapshot-load invalid-state envelope rather than reaching the stub success path.
