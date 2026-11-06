# scripts/systemvm

## Overview

Thin Bash wrappers around the `systemvmctl` binary (`src/bin/systemvmctl.rs`).
Each script resolves the repo root, creates the output directories it needs,
and shells out to `cargo run --bin systemvmctl`. `fetch-image.sh` is the only
script that touches the network: it downloads the archive pinned in
`docs/specs/systemvm-image.lock.yaml`, verifies it, extracts the kernel and
initramfs, and has `systemvmctl verify-image` check them. The others call one
subcommand each: `build`, `launch-config`, or `boot-smoke`. Optional positional
arguments are forwarded as flag values. The Makefile targets `systemvm-fetch`,
`systemvm-build`, `systemvm-launch-config`, and `systemvm-boot-smoke` call these
scripts with no arguments, so Makefile invocations always use the defaults
below.

## Interface

- All three scripts compute `ROOT_DIR` as the repo root two levels above the script and consume no environment variables.
- `fetch-image.sh`: `$1` -> `LOCK_PATH` (default `${ROOT_DIR}/docs/specs/systemvm-image.lock.yaml`). Runs `cargo run -q --bin systemvmctl -- lock-env --lock <LOCK_PATH>` and `eval`s its `VESPER_IMAGE_*` lines; downloads `VESPER_IMAGE_URL` to `${ROOT_DIR}/build/systemvm/cache/<basename>` with `curl -fsSL --retry 3` unless a cached copy already has `VESPER_IMAGE_SHA256`; verifies the archive digest (`sha256sum`, or `shasum -a 256` on macOS); extracts `VESPER_IMAGE_KERNEL_MEMBER` and `VESPER_IMAGE_INITRD_MEMBER` into the cache and copies them to `${ROOT_DIR}/build/systemvm/<basename>`; runs `cargo run -q --bin systemvmctl -- verify-image --lock <LOCK_PATH> --dir ${ROOT_DIR}/build/systemvm --out ${ROOT_DIR}/build/systemvm/image-manifest.json`.
- `build-image.sh`: `$1` -> `KERNEL_PATH` (default `${ROOT_DIR}/build/systemvm/vmlinuz-virt`), `$2` -> `INITRD_PATH` (default `${ROOT_DIR}/build/systemvm/initramfs-virt`), optional `$3` -> `ROOTFS_PATH`. Exits 2 if the kernel or initrd file is missing. Runs `cargo run --bin systemvmctl -- build --kernel <KERNEL_PATH> --initrd <INITRD_PATH> [--rootfs <ROOTFS_PATH>] --out ${ROOT_DIR}/build/systemvm/image-manifest.json`.
- `generate-launch-config.sh`: same `$1`, `$2`, `$3` semantics, defaults and missing-file exit as `build-image.sh`. Runs `cargo run --bin systemvmctl -- launch-config --kernel <KERNEL_PATH> --initrd <INITRD_PATH> [--rootfs <ROOTFS_PATH>] --console-log ${ROOT_DIR}/run/systemvm/console.log --out ${ROOT_DIR}/build/systemvm/launch-config.json`.
- `boot-smoke.sh`: `$1` -> `--iterations` (default `5`), `$2` -> `--timeout-seconds` (default `20`), `$3` -> `--command` (default `sh`), `$4` -> `--args=<value>` (default `-c,echo boot-ok`, comma-separated and passed as a single `--args=` token). Runs `cargo run --bin systemvmctl -- boot-smoke ... --log ${ROOT_DIR}/run/systemvm/boot.log --report ${ROOT_DIR}/run/systemvm/boot-report.json`.
- Exit codes: no explicit `exit` statements; under `set -e` each script exits with the status of the first failing command, otherwise with the exit status of `cargo run`.
- Makefile entry points: `make systemvm-fetch`, `make systemvm-build`, `make systemvm-launch-config`, `make systemvm-boot-smoke`.

## Invariants

- All four scripts use `set -euo pipefail`.
- All file-path arguments are absolute under the computed `ROOT_DIR`. `fetch-image.sh` does `cd "${ROOT_DIR}"`; the other three never `cd`, so their `cargo run` executes in the caller's cwd and fails outside the crate tree.
- `build-image.sh` and `generate-launch-config.sh` run `mkdir -p` on their output directories before `systemvmctl` writes to them.
- `build-image.sh` and `generate-launch-config.sh` check only that the kernel and initrd files exist; `systemvmctl build` digests the files, while `launch-config` only checks that the strings are non-blank and writes whatever paths it is given. `fetch-image.sh` never trusts a download: the archive digest is checked in bash and the member digests plus `expected_image_id` in Rust, and any mismatch exits non-zero before the manifest is written.
- `fetch-image.sh` is idempotent: with a cached archive of the pinned digest it performs no network access.

## Side Effects

- `fetch-image.sh` creates `${ROOT_DIR}/build/systemvm/cache`, downloads up to about 330 MB into it, extracts `boot/` members there, copies the kernel and initramfs to `${ROOT_DIR}/build/systemvm/`, and spawns `cargo run -q --bin systemvmctl` twice (`lock-env`, `verify-image`); `verify-image` writes `${ROOT_DIR}/build/systemvm/image-manifest.json`.
- `build-image.sh` creates `${ROOT_DIR}/build/systemvm` and spawns `cargo run --bin systemvmctl build`, which writes `${ROOT_DIR}/build/systemvm/image-manifest.json`.
- `generate-launch-config.sh` creates `${ROOT_DIR}/build/systemvm` and `${ROOT_DIR}/run/systemvm`, and spawns `cargo run --bin systemvmctl launch-config`, which writes `${ROOT_DIR}/build/systemvm/launch-config.json` with `${ROOT_DIR}/run/systemvm/console.log` as the console log path.
- `boot-smoke.sh` creates `${ROOT_DIR}/run/systemvm` and spawns `cargo run --bin systemvmctl boot-smoke`, which writes `${ROOT_DIR}/run/systemvm/boot.log` and `${ROOT_DIR}/run/systemvm/boot-report.json`.
- Every `cargo run` may compile the crate, writing to `target/`.

## Verification

- Run: `make systemvm-fetch`, `make systemvm-build`, `make systemvm-launch-config`, `make systemvm-boot-smoke`
- CI: `.github/workflows/systemvm-image.yml` runs the first three on Linux and asserts the manifest `image_id` equals the lock's `expected_image_id`.
- Broader regression for `systemvmctl` and the `systemvm` module: `cargo test`
