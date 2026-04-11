# Step 105 upstream version runtime baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: `GET /version` response shape after the microVM is booted

## Source packet

- `run/WOR-20/api/step28-upstream-identity-version-baseline.md`
  - `parse_get_version()` maps `GET /version` to `VmmAction::GetVmmVersion`.
  - `ParsedRequest::convert_to_response()` serializes `VmmData::VmmVersion(version)` as JSON with the `firecracker_version` field.
- `src/firecracker/src/api_server/request/version.rs`
  - the request parser accepts `GET /version` directly and does not introduce a microVM lifecycle-state guard.

## Derived baseline facts

- `GET /version` should remain callable after boot because the pinned request/response path is not gated on the microVM lifecycle state.
- The runtime response should stay a JSON object with a `firecracker_version` field.

## Local parity decision

- This slice is harness-first, not a handler change: Vesper already serves `GET /version`, but the generated route-level behavior matrix still only recorded the preboot success case.
