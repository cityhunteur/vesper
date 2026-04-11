# Upstream baseline note: `PUT /vsock` after boot

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- source review method: repo-local baseline inference from `run/WOR-20/api/step20-upstream-vsock-store-baseline.md` plus neighboring pinned `OperationNotSupportedPostBoot` captures for preboot-only configuration routes

## Findings

- `run/WOR-20/api/step20-upstream-vsock-store-baseline.md` already records that `PUT /vsock` parses into the upstream vsock device configuration request path and is captured there as a preboot stored-config mutation.
- Neighboring preboot-only configuration routes such as `PUT /boot-source`, `PUT /cpu-config`, `PUT /entropy`, `PUT /hotplug/memory`, and `PUT /mmds/config` already have pinned WOR-20 captures showing that the Firecracker runtime controller returns `VmmActionError::OperationNotSupportedPostBoot` after boot.
- `VmmActionError::OperationNotSupportedPostBoot` renders as `The requested operation is not supported after starting the microVM.`

## Relevance to WOR-20

- This is an inference rather than a fresh direct source capture for `/vsock`, but it grounds the expected postboot `PUT /vsock` envelope in the same pinned request/controller pattern already established for adjacent preboot-only configuration endpoints.
- The step-92 contract proof verifies that the local router matches that pinned Firecracker-shaped `400 InvalidState` envelope on the full route.
