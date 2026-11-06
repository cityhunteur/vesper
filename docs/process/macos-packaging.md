---
status: verified
owner: cityhunteur
last_reviewed: 2026-11-06
---

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
   With `plutil` present this checks plist syntax only. The required-key check (`com.apple.security.app-sandbox` and
   `com.apple.security.virtualization` for `vesperd`, `com.apple.security.app-sandbox` for the proxy) runs on the
   Python path, which CI uses on Linux (`rust-tests.yml`) and which `VALIDATE_ENTITLEMENTS_FORCE_PYTHON=1` forces locally.
3. Start services:
   `make dev-up`
   Runs `cargo build --bin vesperd --bin vesper-proxy`, starts `vesperd` on `run/vesperd.sock` with the baseline lock,
   swagger and `--entitlements-file packaging/macos/vesperd.entitlements` (the surface stub matrix uses its default
   path), starts `vesper-proxy` on `run/docker.sock` with upstream `unix://run/vesperd.sock`, and writes
   `run/{vesperd,vesper-proxy}.{pid,log}`. Details: `scripts/macos/README.md`.
4. Stop services:
   `make dev-down`

## Startup entitlement validation

`vesperd` supports an entitlement validation hook:

`--entitlements-file packaging/macos/vesperd.entitlements`

When provided, startup fails unless the literal `<key>com.apple.security.virtualization</key>` appears in the file;
the value is not inspected, and without the flag no check runs (`src/platform.rs`, `src/bin/vesperd.rs`).
`vesper-proxy` has no entitlements flag; its plist is validated by the lint only.
