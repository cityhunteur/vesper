# scripts/systemvm

## Overview

Thin Bash wrappers around the `systemvmctl` binary (`src/bin/systemvmctl.rs`).
Each script resolves the repo root, creates the output directories it needs,
and shells out to `cargo run --bin systemvmctl` with one subcommand: `build`,
`launch-config`, or `boot-smoke`. Optional positional arguments are forwarded
as flag values. The Makefile targets `systemvm-build`,
`systemvm-launch-config`, and `systemvm-boot-smoke` call these scripts with no
arguments, so Makefile invocations always use the defaults below.

## Interface

- All three scripts compute `ROOT_DIR` as the repo root two levels above the script and consume no environment variables.
- `build-image.sh`: `$1` -> `KERNEL_PATH` (default `${ROOT_DIR}/assets/systemvm/vmlinux`), `$2` -> `ROOTFS_PATH` (default `${ROOT_DIR}/assets/systemvm/rootfs.ext4`). Runs `cargo run --bin systemvmctl -- build --kernel <KERNEL_PATH> --rootfs <ROOTFS_PATH> --out ${ROOT_DIR}/build/systemvm/image-manifest.json`.
- `generate-launch-config.sh`: same `$1` and `$2` semantics and defaults as `build-image.sh`. Runs `cargo run --bin systemvmctl -- launch-config --kernel <KERNEL_PATH> --rootfs <ROOTFS_PATH> --console-log ${ROOT_DIR}/run/systemvm/console.log --out ${ROOT_DIR}/build/systemvm/launch-config.json`.
- `boot-smoke.sh`: `$1` -> `--iterations` (default `5`), `$2` -> `--timeout-seconds` (default `20`), `$3` -> `--command` (default `sh`), `$4` -> `--args=<value>` (default `-c,echo boot-ok`, comma-separated and passed as a single `--args=` token). Runs `cargo run --bin systemvmctl -- boot-smoke ... --log ${ROOT_DIR}/run/systemvm/boot.log --report ${ROOT_DIR}/run/systemvm/boot-report.json`.
- Exit codes: no explicit `exit` statements; under `set -e` each script exits with the status of the first failing command, otherwise with the exit status of `cargo run`.
- Makefile entry points: `make systemvm-build`, `make systemvm-launch-config`, `make systemvm-boot-smoke`.

## Invariants

- All three scripts use `set -euo pipefail`.
- All file-path arguments are absolute under the computed `ROOT_DIR`, but the scripts never `cd`, so `cargo run` executes in the caller's cwd and fails outside the crate tree.
- `build-image.sh` and `generate-launch-config.sh` run `mkdir -p` on their output directories before `systemvmctl` writes to them.
- The scripts perform no input validation themselves. `systemvmctl build` reads the kernel and rootfs files, so missing paths fail there; `launch-config` only checks that the strings are non-blank and writes whatever paths it is given.

## Side Effects

- `build-image.sh` creates `${ROOT_DIR}/build/systemvm` and spawns `cargo run --bin systemvmctl build`, which writes `${ROOT_DIR}/build/systemvm/image-manifest.json`.
- `generate-launch-config.sh` creates `${ROOT_DIR}/build/systemvm` and `${ROOT_DIR}/run/systemvm`, and spawns `cargo run --bin systemvmctl launch-config`, which writes `${ROOT_DIR}/build/systemvm/launch-config.json` with `${ROOT_DIR}/run/systemvm/console.log` as the console log path.
- `boot-smoke.sh` creates `${ROOT_DIR}/run/systemvm` and spawns `cargo run --bin systemvmctl boot-smoke`, which writes `${ROOT_DIR}/run/systemvm/boot.log` and `${ROOT_DIR}/run/systemvm/boot-report.json`.
- Every `cargo run` may compile the crate, writing to `target/`.

## Verification

- Run: `make systemvm-build`, `make systemvm-launch-config`, `make systemvm-boot-smoke`
- Broader regression for `systemvmctl` and the `systemvm` module: `cargo test`
