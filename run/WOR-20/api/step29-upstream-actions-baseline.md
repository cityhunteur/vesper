# Step 29 upstream actions baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: `PUT /actions` state guards and response behavior

## Source packet

- `src/firecracker/src/api_server/request/actions.rs`
  - `parse_put_actions()` maps `FlushMetrics` to `VmmAction::FlushMetrics`, `InstanceStart` to `VmmAction::StartMicroVm`, and `SendCtrlAltDel` to either `VmmAction::SendCtrlAltDel` on `x86_64` or a `BadRequest` parse error on `aarch64`.
- `src/vmm/src/rpc_interface.rs`
  - `PrebootApiController::handle_preboot_request()` returns `OperationNotSupportedPreBoot` for `FlushMetrics` and `SendCtrlAltDel`.
  - `PrebootApiController::handle_preboot_request()` routes `StartMicroVm` through `start_microvm()`.
  - `RuntimeApiController::handle_request()` returns `OperationNotSupportedPostBoot` for `StartMicroVm`.
  - `RuntimeApiController::handle_request()` allows `FlushMetrics` and, on `x86_64`, `SendCtrlAltDel`.
- `src/vmm/src/builder.rs`
  - `StartMicrovmError::MissingKernelConfig` displays `Cannot start microvm without kernel configuration.`
- `src/firecracker/src/api_server/parsed_request.rs`
  - `ParsedRequest::convert_to_response()` maps `VmmActionError` values to `400 BadRequest` with the error string in `fault_message`.

## Derived baseline facts

- `PUT /actions` with `{"action_type":"InstanceStart"}` should reject preboot when no boot source is configured with the missing-kernel fault string.
- `PUT /actions` with `{"action_type":"InstanceStart"}` should reject after boot with the postboot-invalid-state fault string.
- `PUT /actions` with `{"action_type":"FlushMetrics"}` should reject preboot with the preboot-invalid-state fault string and succeed postboot.
- On `x86_64`, `PUT /actions` with `{"action_type":"SendCtrlAltDel"}` should reject preboot with the preboot-invalid-state fault string and succeed postboot.
- On `aarch64`, `PUT /actions` with `{"action_type":"SendCtrlAltDel"}` should be rejected at parse time with the `SendCtrlAltDel does not supported on aarch64.` fault string.

## Local parity decision

- This slice is harness-first, not a behavior fix: Vesper already follows the pinned `/actions` guard surface for the covered cases, but GH `#2` still needed upstream-backed full-router proof instead of unit-only coverage.
