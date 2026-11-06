# scripts/macos

## Overview

Local developer loop for macOS. `dev-up.sh` builds the `vesperd` and
`vesper-proxy` binaries in the debug profile and launches both as background
processes under `<repo>/run/`, recording pid and log files. `dev-down.sh`
stops them via those pid files. Both are wrapped by the Makefile targets
`dev-up` and `dev-down`.

## Interface

- `dev-up.sh` and `dev-down.sh` accept no CLI arguments and read no environment variables beyond the ambient `PATH` used to find `cargo`.
- `ROOT_DIR` is derived from the script location (`../..`); `BIN_DIR` is hardcoded to `${ROOT_DIR}/target/debug`; `RUN_DIR` is hardcoded to `${ROOT_DIR}/run`.
- `dev-up.sh` runs `cargo build --bin vesperd --bin vesper-proxy` with the repo root as cwd.
- `dev-up.sh` launches `vesperd` with `--socket ${RUN_DIR}/vesperd.sock`, `--baseline-lock docs/specs/design-v0.baselines.lock.yaml`, `--swagger third_party/firecracker/firecracker.yaml`, and `--entitlements-file packaging/macos/vesperd.entitlements` (all absolute under `ROOT_DIR`). `--surface-stub-matrix` is not passed, so `vesperd` loads its relative default `docs/generated/firecracker-api-surface-stub-matrix.json`, which resolves only because of the `pushd` to `ROOT_DIR`.
- `dev-up.sh` launches `vesper-proxy` with `--listen ${RUN_DIR}/docker.sock` and `--upstream unix://${RUN_DIR}/vesperd.sock`.
- `dev-up.sh` stdout: `vesperd PID: <pid>` and `vesper-proxy PID: <pid>`. `dev-down.sh` stdout: `services stopped`.
- Exit codes: `dev-up.sh` returns `0` on success and propagates failures from `cargo build`, `mkdir`, `pushd`, or `cat` via `set -e`; backgrounded binaries (`&`) do not affect its exit status even if they exit immediately. `dev-down.sh` returns `0` in practice because `kill` failures are suppressed with `|| true`; an unreadable pid file aborts under `set -e` because `cat` is unguarded.
- Makefile entry points: `make dev-up` and `make dev-down`.

## Invariants

- Both scripts use `set -euo pipefail`.
- `dev-up.sh`: `cargo build` must succeed before either binary is launched.
- `dev-up.sh` ordering: `vesperd` is started and its pid written before `vesper-proxy` is started. The script does not wait for `vesperd.sock` to exist before launching the proxy.
- `dev-up.sh` runs `mkdir -p "${RUN_DIR}"` before any pid or log file is written.
- `dev-up.sh` has no idempotency guard: re-running it without `dev-down.sh` overwrites the pid files and starts duplicate processes, losing the previous pids.
- `dev-down.sh` stops `vesper-proxy` first, then `vesperd` (reverse of start order), per the loop `for name in vesper-proxy vesperd`.
- `dev-down.sh` sends a signal only when the pid file exists and `kill -0 "${pid}"` succeeds; stale pid files are removed regardless.
- `dev-down.sh` sends default `SIGTERM` only, with no `SIGKILL` escalation and no wait for exit.
- `dev-down.sh` deletes only `${RUN_DIR}/<name>.pid`; `vesperd.sock`, `docker.sock`, and `*.log` are left in place.

## Side Effects

- `dev-up.sh` creates `<repo>/run/` and writes build artifacts to `<repo>/target/debug/`.
- `dev-up.sh` spawns detached background `vesperd` and `vesper-proxy` processes, truncating and redirecting their stdout and stderr to `<repo>/run/vesperd.log` and `<repo>/run/vesper-proxy.log`.
- `dev-up.sh` writes `<repo>/run/vesperd.pid` and `<repo>/run/vesper-proxy.pid` via `echo $!`.
- The launched binaries (not the scripts) create Unix socket listeners at `<repo>/run/vesperd.sock` and `<repo>/run/docker.sock`. No TCP listeners are opened.
- `dev-down.sh` sends `SIGTERM` to the recorded pids if alive and deletes the two pid files.

## Verification

- Run: `make dev-up` then `make dev-down`
- Broader regression for the launched binaries: `cargo test`
