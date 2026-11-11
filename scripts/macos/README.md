# scripts/macos

## Overview

Local developer loop for macOS. `dev-up.sh` builds the `vesperd` and
`vesper-proxy` binaries in the debug profile and launches both as background
processes under `<repo>/run/`, recording pid and log files. `dev-down.sh`
stops them via those pid files. Both are wrapped by the Makefile targets
`dev-up` and `dev-down`. `build-vmlauncher.sh` builds the Swift
Virtualization.framework helper in `launcher/` and ad-hoc signs it with the
virtualization entitlement (`make vmlauncher-build`). `share-watch-selftest.sh` checks the launcher's share
watcher without a VM (`make share-watch-selftest`).

## Interface

- `dev-up.sh` and `dev-down.sh` accept no CLI arguments and read no environment variables beyond the ambient `PATH` used to find `cargo`.
- `ROOT_DIR` is derived from the script location (`../..`); `BIN_DIR` is hardcoded to `${ROOT_DIR}/target/debug`; `RUN_DIR` is hardcoded to `${ROOT_DIR}/run`.
- `dev-up.sh` runs `cargo build --bin vesperd --bin vesper-proxy` with the repo root as cwd.
- `dev-up.sh` launches `vesperd` with `--socket ${RUN_DIR}/vesperd.sock`, `--baseline-lock docs/specs/design-v0.baselines.lock.yaml`, `--swagger third_party/firecracker/firecracker.yaml`, and `--entitlements-file packaging/macos/vesperd.entitlements` (all absolute under `ROOT_DIR`). `--surface-stub-matrix` is not passed, so `vesperd` loads its relative default `docs/generated/firecracker-api-surface-stub-matrix.json`, which resolves only because of the `pushd` to `ROOT_DIR`.
- `dev-up.sh` launches `vesper-proxy` with `--listen ${RUN_DIR}/docker.sock` and `--upstream unix://${RUN_DIR}/vesperd.sock`.
- `dev-up.sh` stdout: `vesperd PID: <pid>` and `vesper-proxy PID: <pid>`. `dev-down.sh` stdout: `services stopped`.
- Exit codes: `dev-up.sh` returns `0` on success and propagates failures from `cargo build`, `mkdir`, `pushd`, or `cat` via `set -e`; backgrounded binaries (`&`) do not affect its exit status even if they exit immediately. `dev-down.sh` returns `0` in practice because `kill` failures are suppressed with `|| true`; an unreadable pid file aborts under `set -e` because `cat` is unguarded.
- `build-vmlauncher.sh` accepts no arguments. It exits 2 on a non-Darwin host; runs `swift build -c release --package-path ${ROOT_DIR}/launcher`; copies the product from `swift build --show-bin-path` to `${ROOT_DIR}/build/vmlauncher/vesper-vmlauncher`; runs `codesign --force --sign - --entitlements ${ROOT_DIR}/packaging/macos/vesper-vmlauncher.entitlements` on it; exits 1 unless `codesign --display --entitlements -` then lists `com.apple.security.virtualization`; prints `built and signed <path>`.
- `share-watch-selftest.sh` accepts no arguments (`VMLAUNCHER` overrides the binary, default `build/vmlauncher/vesper-vmlauncher`; exits 2 when it is missing). It needs `python3` for a stand-in sink and runs 20 checks in two halves. First it starts `vesper-vmlauncher --watch-dry-run <temp dir>`, edits files and compares the printed paths: 11 checks (greeting, append, replay echo dropped, create, atomic save, delete sends the parent, create in the share root sent, delete in the share root not sent, path outside the share not sent, new directory and its file sent, configured path prefix used although FSEvents reports the resolved one). Then it adds `--watch-sink-socket <socket under /tmp>`: 9 checks (`sink_burst_delivered` for 3,000 files, `sink_lines_whole`, `sink_reconnects`, `sink_delivers_after_reconnect`, `sink_resends_unsent_edit`, `sigterm_exits_with_summary`; then, with a sink that accepts and never reads and `VESPER_SHARE_WATCH_PENDING_LIMIT=2000`, `stalled_sink_sigterm_exits` within 3 s, `stalled_sink_reported`, `queue_limit_drops_oldest`). Prints `ok`/`FAIL` lines; exits 1 on any failure. Also a step of the System VM Image workflow's macOS job (`launcher-build`, `macos-15`).
- Makefile entry points: `make dev-up`, `make dev-down`, `make vmlauncher-build`, `make share-watch-selftest`.

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
- `build-vmlauncher.sh` signs ad hoc (`--sign -`): the binary runs on the building machine but carries no Developer ID; the entitlement file contains only `com.apple.security.virtualization` (no App Sandbox, so the helper can read image files anywhere).

## Side Effects

- `dev-up.sh` creates `<repo>/run/` and writes build artifacts to `<repo>/target/debug/`.
- `dev-up.sh` spawns detached background `vesperd` and `vesper-proxy` processes, truncating and redirecting their stdout and stderr to `<repo>/run/vesperd.log` and `<repo>/run/vesper-proxy.log`.
- `dev-up.sh` writes `<repo>/run/vesperd.pid` and `<repo>/run/vesper-proxy.pid` via `echo $!`.
- The launched binaries (not the scripts) create Unix socket listeners at `<repo>/run/vesperd.sock` and `<repo>/run/docker.sock`. No TCP listeners are opened.
- `dev-down.sh` sends `SIGTERM` to the recorded pids if alive and deletes the two pid files.
- `build-vmlauncher.sh` writes `<repo>/launcher/.build/` (gitignored) and `<repo>/build/vmlauncher/vesper-vmlauncher`, and replaces that file's code signature.

## Verification

- Run: `make dev-up` then `make dev-down`; `make vmlauncher-build` then `build/vmlauncher/vesper-vmlauncher --help`
- Broader regression for the launched binaries: `cargo test`
