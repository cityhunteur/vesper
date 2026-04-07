# Upstream baseline note: `PUT /boot-source` after boot

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- source review method: pinned-source inspection against the Firecracker GitHub repository at the locked revision

## Findings

- `src/firecracker/src/api_server/request/boot_source.rs` parses `PUT /boot-source` into `VmmAction::ConfigureBootSource`.
- `src/vmm/src/rpc_interface.rs` classifies `ConfigureBootSource(_)` under runtime operations not allowed post-boot.
- The runtime controller returns `Err(VmmActionError::OperationNotSupportedPostBoot)` for that action after boot.
- `VmmActionError::OperationNotSupportedPostBoot` renders as `The requested operation is not supported after starting the microVM.`

## Relevance to WOR-20

- This confirms that postboot `PUT /boot-source` should return a `400 InvalidState` envelope with the canonical Firecracker postboot-invalid-state message.
- The step-88 contract proof verifies that the local router now matches that pinned Firecracker envelope on the full route.
