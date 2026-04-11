# Upstream baseline note: `PUT /mmds/config` after boot

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- source review method: pinned-source inspection against the Firecracker GitHub repository at the locked revision

## Findings

- `src/firecracker/src/api_server/request/mmds.rs` parses `PUT /mmds/config` into `VmmAction::SetMmdsConfiguration(config)`.
- `src/vmm/src/rpc_interface.rs` classifies `SetMmdsConfiguration(_)` under runtime operations not allowed post-boot.
- The runtime controller returns `Err(VmmActionError::OperationNotSupportedPostBoot)` for that action after boot.
- `VmmActionError::OperationNotSupportedPostBoot` renders as `The requested operation is not supported after starting the microVM.`

## Relevance to WOR-20

- This confirms that postboot `PUT /mmds/config` should return a `400 InvalidState` envelope with the canonical Firecracker postboot-invalid-state message.
- The local router-level guard previously emitted an operation-specific `running` message instead, so the WOR-20 slice fixes a real parity mismatch rather than only adding proof.
