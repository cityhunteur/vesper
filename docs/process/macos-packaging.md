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
   With `plutil` present this checks plist syntax first. Then, on every host, each plist must hold exactly its
   approved keys: none for `vesperd` and the proxy, `com.apple.security.virtualization` for the launcher (TD-010,
   ADR 0012).
3. Start services:
   `make dev-up`
   Runs `cargo build --bin vesperd --bin vesper-proxy`, signs both (`scripts/macos/sign.sh`), starts `vesperd --backend model` (the in-memory model; the default backend needs the System VM, D-063) on `run/vesperd.sock` with the baseline lock,
   swagger and `--entitlements-file packaging/macos/vesperd.entitlements` (the surface stub matrix uses its default
   path), starts `vesper-proxy` on `run/docker.sock` with upstream `unix://run/vesperd.sock`, and writes
   `run/{vesperd,vesper-proxy}.{pid,log}`. Details: `scripts/macos/README.md`.
4. Stop services:
   `make dev-down`

## Startup entitlement validation

`vesperd` supports an entitlement validation hook:

`--entitlements-file packaging/macos/vesperd.entitlements`

When provided, the file is read before start; no key is required, because `vesperd` applies its own seatbelt
profile (ADR 0012). `vesper-proxy --entitlements-file` does the same (TD-017).
