# Step 28 upstream identity/version baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: `GET /` instance info and `GET /version` response behavior

## Source packet

- `src/firecracker/src/api_server/request/instance_info.rs`
  - `parse_get_instance_info()` maps `GET /` to `VmmAction::GetVmInstanceInfo`.
- `src/firecracker/src/api_server/request/version.rs`
  - `parse_get_version()` maps `GET /version` to `VmmAction::GetVmmVersion`.
- `src/firecracker/src/api_server/parsed_request.rs`
  - `ParsedRequest::convert_to_response()` serializes `VmmData::InstanceInformation(info)` directly as the response body.
  - `ParsedRequest::convert_to_response()` serializes `VmmData::VmmVersion(version)` as JSON with the `firecracker_version` field.
- `src/vmm/src/vmm_config/instance_info.rs`
  - `VmState::NotStarted` displays as the string `Not started`.

## Derived baseline facts

- `GET /` should return a serialized instance-information object.
- The preboot instance-info state string should be `Not started`.
- `GET /version` should return JSON with a `firecracker_version` field.

## Local parity decision

- This slice is harness-first, not a behavior fix: Vesper already serves the read-only identity/version endpoints, but GH `#2` still needed upstream-backed full-router proof for their preboot response surface.
