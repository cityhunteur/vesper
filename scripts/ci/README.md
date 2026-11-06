# scripts/ci

## Overview

CI helper for the macOS packaging surface. The single script
`validate-entitlements.sh` lints the two entitlement plists under
`packaging/macos/` so that `vesperd` and `vesper-proxy` ship with the sandbox
and virtualization entitlements the platform layer expects. It is wrapped by
the Makefile target `validate-entitlements`.

## Interface

- `validate-entitlements.sh` accepts no CLI arguments; positional arguments are ignored.
- `VALIDATE_ENTITLEMENTS_FORCE_PYTHON` (env, default `0`): when set to `1` the `plutil` path is skipped and the Python `plistlib` validator runs instead.
- `ROOT_DIR` is derived from the script location (`../..`), not configurable.
- Files read: `packaging/macos/vesperd.entitlements` (`VESPERD_FILE`) and `packaging/macos/vesper-proxy.entitlements` (`PROXY_FILE`).
- External tools: `plutil -lint <file>` when `plutil` is on `PATH`; otherwise `python3 -` with an inline `plistlib` program receiving the two paths as `argv[1]` and `argv[2]`.
- Exit codes: `0` on success (explicit `exit 0` after both `plutil -lint` calls, or both `<path>: OK (validated with python plistlib)` lines printed); non-zero when `plutil -lint` fails, a plist cannot be parsed, the plist root is not a dictionary, or a required key is missing or not `true`.
- Makefile entry point: `make validate-entitlements` runs `bash scripts/ci/validate-entitlements.sh`.

## Invariants

- `set -euo pipefail`: any failing command, unset variable, or failed pipeline aborts the script.
- Python path (`expected_keys`): `vesperd.entitlements` must have `com.apple.security.app-sandbox` and `com.apple.security.virtualization` both equal to boolean `true`; `vesper-proxy.entitlements` must have `com.apple.security.app-sandbox` equal to `true`. A key whose value is anything other than `True` counts as missing.
- Python path (`isinstance(data, dict)`): each plist root must be a dictionary.
- The `plutil -lint` path checks plist syntax only. The required-key check runs only when `plutil` is absent or `VALIDATE_ENTITLEMENTS_FORCE_PYTHON=1`.
- Both entitlement files must exist and be readable.

## Side Effects

- Spawns `plutil -lint` subprocesses (read-only on the plist files).
- Spawns a `python3` subprocess that reads the two plists and prints status lines to stdout.
- No files are written and no network calls are made.

## Verification

- Run: `make validate-entitlements`
- Force the Python validator path: `VALIDATE_ENTITLEMENTS_FORCE_PYTHON=1 bash scripts/ci/validate-entitlements.sh`
