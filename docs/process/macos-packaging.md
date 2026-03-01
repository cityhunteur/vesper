# macOS Packaging and Entitlements

This repository includes a baseline packaging workflow for local macOS development:

- `packaging/macos/vesperd.entitlements`
- `packaging/macos/vesper-proxy.entitlements`
- `scripts/macos/dev-up.sh` and `scripts/macos/dev-down.sh`
- `Makefile` build and run targets

## Build and run

1. Build binaries:
   `make build`
2. Validate entitlement plist files:
   `make validate-entitlements`
3. Start services:
   `make dev-up`
4. Stop services:
   `make dev-down`

## Startup entitlement validation

`vesperd` supports an entitlement validation hook:

`--entitlements-file packaging/macos/vesperd.entitlements`

When provided, startup fails if required entitlement keys are missing.
