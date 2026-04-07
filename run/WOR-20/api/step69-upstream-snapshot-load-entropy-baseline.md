# Step 69 upstream baseline notes: snapshot-load rejection after entropy configuration

Pinned Firecracker baseline revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`

Primary sources inspected:

- `docs/snapshotting/snapshot-support.md`
- prior pinned-source packet captured in `run/WOR-20/api/step21-upstream-entropy-store-baseline.md`

Captured facts:

- The pinned snapshotting docs state that `LoadSnapshot` is allowed only before boot and before the microVM is configured; the only resources that can be configured beforehand are the logger and metrics systems.
- The pinned entropy source packet already established that `PUT /entropy` is a preboot route that stores entropy-device configuration.
- Inference from those pinned sources: entropy configuration is not part of the logger-and-metrics exception set, so it belongs to the configured-resource set that blocks `LoadSnapshot`.

Implication for the local Step 69 slice:

- After a successful `PUT /entropy`, a valid `PUT /snapshot/load` request should be rejected by the state guard with the snapshot-load invalid-state envelope rather than reaching the stub success path.
