# Step 18 upstream drive cache default baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: preboot `PUT /drives/{drive_id}` stored-config defaulting for omitted `cache_type`

## Source packet

- `src/firecracker/src/api_server/request/drive.rs`
  - `parse_put_drive()` deserializes into `BlockDeviceConfig` and forwards the config unchanged after path/body id validation.
- `src/vmm/src/vmm_config/drive.rs`
  - `BlockDeviceConfig.cache_type` is declared as `#[serde(default)] pub cache_type: CacheType`.
  - `BlockBuilder::configs()` re-serializes stored drive configs back into `BlockDeviceConfig`.
  - The upstream test fixtures use `CacheType::Unsafe` as the omitted/default cache mode.

## Derived baseline fact

- Preboot `PUT /drives/{drive_id}` that omits `cache_type` should still store and re-export the drive config with `cache_type = "Unsafe"`.

## Local parity decision

- This slice can truthfully tighten the control-plane stub by defaulting omitted drive `cache_type` to `Unsafe` in the stored config shape and proving that `/vm/config` exports the same pinned default.
