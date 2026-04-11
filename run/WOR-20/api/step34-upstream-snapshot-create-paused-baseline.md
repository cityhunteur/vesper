# Step 34 upstream baseline notes: snapshot-create paused-state prerequisites

Pinned Firecracker baseline revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`

Primary sources inspected:

- `docs/snapshotting/snapshot-support.md`
- `src/vmm/src/rpc_interface.rs`

Captured facts:

- The pinned snapshotting docs state that `Pause`, `Resume`, and `CreateSnapshot` can be called only after booting the microVM, while `LoadSnapshot` is allowed only before boot.
- In the pinned `Creating snapshots` section, the documented prerequisite is explicit: the microVM is `Paused`.
- In the pinned `rpc_interface.rs`, `VmmAction::CreateSnapshot` is documented as callable only after the microVM has booted and only when the microVM is in `Paused` state.

Implication for the local Step 34 slice:

- The full router should reject `PUT /snapshot/create` while the VM is running.
- Once the VM is booted and paused, `PUT /snapshot/create` should reach the route-specific placeholder path rather than fail the state guard.
