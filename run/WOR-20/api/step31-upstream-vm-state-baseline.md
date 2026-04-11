# Step 31 Upstream VM State Baseline

Pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`

Primary sources inspected:

- `src/firecracker/src/api_server/request/snapshot.rs`
- `src/vmm/src/rpc_interface.rs`
- `src/firecracker/src/api_server/parsed_request.rs`

Observed pinned facts for `PATCH /vm`:

1. `parse_patch_vm_state()` maps:
   - `{"state":"Paused"}` -> `VmmAction::Pause`
   - `{"state":"Resumed"}` -> `VmmAction::Resume`
2. In `PrebootApiController::handle_preboot_request()`:
   - `Pause` and `Resume` are rejected with `VmmActionError::OperationNotSupportedPreBoot`
   - that error renders as `The requested operation is not supported before starting the microVM.`
3. In `RuntimeApiController::handle_request()`:
   - `Pause` dispatches to `pause()`
   - `Resume` dispatches to `resume()`
   - successful runtime pause/resume actions return `VmmData::Empty`, which `ParsedRequest::convert_to_response()` renders as `204 No Content`

This slice only covers the `PATCH /vm` pause/resume request and controller behavior. Broader runtime-sensitive snapshot and VM lifecycle parity remains separate work.
