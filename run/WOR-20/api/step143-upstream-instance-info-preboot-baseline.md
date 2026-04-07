# Upstream baseline note: `GET /` before boot

- Firecracker baseline revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- Capture date: `2026-04-07`
- Source review method: repo-local baseline inference from `run/WOR-20/api/step28-upstream-identity-version-baseline.md`

## Findings

- `run/WOR-20/api/step28-upstream-identity-version-baseline.md` already records that `parse_get_instance_info()` maps `GET /` to `VmmAction::GetVmInstanceInfo`.
- The same pinned source packet records that `ParsedRequest::convert_to_response()` serializes `VmmData::InstanceInformation(info)` directly as the response body.
- The pinned Firecracker instance-info source also records that `VmState::NotStarted` displays as the string `Not started`.

## Local parity implication

- Vesper should keep returning a serialized instance-information document from `GET /` before boot with `state = "Not started"` and the standard identity fields present.
- This note narrows the earlier combined identity/version evidence into a route-specific baseline anchor for the existing preboot `GET /` instance-info surface.
