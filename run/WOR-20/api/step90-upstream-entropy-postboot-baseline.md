# Upstream baseline note: `PUT /entropy` after boot

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- source review method: repo-local baseline inference from `run/WOR-20/api/step21-upstream-entropy-store-baseline.md` plus neighboring pinned `OperationNotSupportedPostBoot` captures for preboot-only configuration routes

## Findings

- `run/WOR-20/api/step21-upstream-entropy-store-baseline.md` already records that `src/firecracker/src/api_server/request/entropy.rs` parses `PUT /entropy` through the entropy-device configuration request path.
- The pinned Firecracker runtime controller returns `VmmActionError::OperationNotSupportedPostBoot` for neighboring preboot-only configuration routes such as `PUT /boot-source`, `PUT /mmds/config`, and `PUT /cpu-config` once the microVM has started.
- `VmmActionError::OperationNotSupportedPostBoot` renders as `The requested operation is not supported after starting the microVM.`

## Relevance to WOR-20

- This is an inference rather than a fresh direct source capture for the entropy route, but it grounds the expected postboot `PUT /entropy` envelope in the same pinned request/controller pattern already established for adjacent preboot-only configuration endpoints.
- The step-90 contract proof verifies that the local router matches that pinned Firecracker-shaped `400 InvalidState` envelope on the full route.
