# Upstream MMDS baseline notes

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- source review method: prior GitHub-source inspection for the pinned revision, captured during the WOR-20 local run because this checkout vendors `firecracker.yaml` but not the full upstream Rust tree

## Relevant upstream files

- `src/vmm/src/resources.rs`
- `src/vmm/src/lib.rs`
- `src/vmm/src/vmm_config/mmds.rs`
- `src/vmm/src/mmds/data_store.rs`
- `src/vmm/src/utils/net/ipv4addr.rs`
- `src/firecracker/src/api_server/request/mmds.rs`
- `src/firecracker/src/api_server/parsed_request.rs`

## Baseline findings for this slice

- `GET /mmds`, `PUT /mmds`, and `PATCH /mmds` are routed as `Any`; `PUT /mmds/config` is `PreBoot`.
- Firecracker exports `mmds_config` through full VM config, while `logger` and `metrics` remain omitted from that export.
- `MmdsConfig` defaults to version `V1`, default `imds_compat = false`, and a default IPv4 address of `169.254.169.254` when the caller omits it.
- MMDS config requires at least one network interface ID and all referenced interface IDs must already exist.
- MMDS IPv4 validation only accepts link-local `169.254.x.y` addresses except the `169.254.0.*` and `169.254.255.*` ranges.
- MMDS datastore `PUT` replaces the store and initializes it when needed.
- MMDS datastore `PATCH` uses JSON merge-patch semantics and fails if the store was never initialized.
- Firecracker treats MMDS datastore size overflow as HTTP `413` and the other MMDS action errors in this slice as HTTP `400`.
- A `GET /mmds` response serializes an uninitialized or `null` store as `{}` rather than JSON `null`.
- Runtime MMDS access is effectively tied to configured MMDS networking because the runtime accessor uses the network-bound MMDS path.
