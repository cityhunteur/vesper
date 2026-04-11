# Step 30 Upstream Snapshot-Load Parser Baseline

Pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`

Primary sources inspected:

- `src/firecracker/src/api_server/request/snapshot.rs`
- `src/firecracker/src/api_server/parsed_request.rs`

Observed pinned facts for `PUT /snapshot/load` parser behavior:

1. `parse_put_snapshot_load()` rejects requests that omit both `mem_backend` and `mem_file_path`.
   - `MISSING_FIELD = "missing field: either \`mem_backend\` or \`mem_file_path\` is required"`
2. `parse_put_snapshot_load()` rejects requests that provide both `mem_backend` and `mem_file_path`.
   - `TOO_MANY_FIELDS = "too many fields: either \`mem_backend\` or \`mem_file_path\` exclusively is required"`
3. Both conditions are surfaced through `RequestError::SerdeJson`.
   - `RequestError::SerdeJson` renders as:
     - `An error occurred when deserializing the json body of a request: {0}.`
4. The resulting fault strings for these two cases are therefore:
   - `An error occurred when deserializing the json body of a request: missing field: either \`mem_backend\` or \`mem_file_path\` is required.`
   - `An error occurred when deserializing the json body of a request: too many fields: either \`mem_backend\` or \`mem_file_path\` exclusively is required.`

This slice only covers the snapshot-load memory-source parser faults. Broader snapshot runtime and preboot guard parity remains separate work.
