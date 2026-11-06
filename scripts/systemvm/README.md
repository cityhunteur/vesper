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
- `fetch-image.sh`: `$1` -> `LOCK_PATH` (default `${ROOT_DIR}/docs/specs/systemvm-image.lock.yaml`). Runs `cargo run -q --bin systemvmctl -- lock-env --lock <LOCK_PATH>` and `eval`s its `VESPER_IMAGE_*` lines; downloads `VESPER_IMAGE_URL` to `${ROOT_DIR}/build/systemvm/cache/<basename>` with `curl -fsSL --retry 3` unless a cached copy already has `VESPER_IMAGE_SHA256`; verifies the source digest (`sha256sum`, or `shasum -a 256` on macOS); extracts `VESPER_IMAGE_KERNEL_MEMBER` and `VESPER_IMAGE_INITRD_MEMBER` into the cache with `bsdtar` (or a `tar` that is bsdtar) and verifies `*_MEMBER_SHA256`; unpacks each member per `*_UNPACK` (`none` copies, `zboot-gzip` runs `python3 -I unpack-zboot.py`) to `${ROOT_DIR}/build/systemvm/<*_FILE>`; copies the source to `${ROOT_DIR}/build/systemvm/${VESPER_IMAGE_BOOT_MEDIA_FILE}` when set; runs `cargo run -q --bin systemvmctl -- verify-image --lock <LOCK_PATH> --dir ${ROOT_DIR}/build/systemvm --out ${ROOT_DIR}/build/systemvm/image-manifest.json`. Exits 1 without bsdtar.
- `unpack-zboot.py INPUT OUTPUT`: standard-library Python; requires the `MZ`/`zimg` header and `gzip` compression type, gunzips the payload at the header's offset/size, requires the `ARM\x64` magic at offset 56 of the result, writes it, prints `unpacked … (<n> bytes)`. Exit 1 on any format mismatch, 2 on usage.
- `build-image.sh`: no arguments -> `cargo run --bin systemvmctl -- verify-image --dir ${ROOT_DIR}/build/systemvm --out ${ROOT_DIR}/build/systemvm/image-manifest.json`. With `$1` -> `KERNEL_PATH`, optional `$2` -> `INITRD_PATH`, `$3` -> `ROOTFS_PATH`: `systemvmctl build --kernel … [--initrd …] [--rootfs …]`.
- `generate-launch-config.sh`: no arguments -> `systemvmctl launch-config --from-lock --dir ${ROOT_DIR}/build/systemvm --console-log ${ROOT_DIR}/run/systemvm/console.log --out ${ROOT_DIR}/build/systemvm/launch-config.json`, after checking that `image-manifest.json` exists (exit 2 otherwise). With positional `KERNEL [INITRD] [ROOTFS]` the explicit form is used.
- `boot-smoke.sh`: `$1` -> `--iterations` (default `5`), `$2` -> launcher timeout in seconds (default `60`; `systemvmctl --timeout-seconds` is set 10 s higher as a backstop), `$3` -> `--command` (default `${ROOT_DIR}/build/vmlauncher/vesper-vmlauncher`), `$4` -> `--args=<value>` (default `--config,${ROOT_DIR}/build/systemvm/launch-config.json,--timeout-seconds,<TIMEOUT>`, comma-separated and passed as a single `--args=` token). With the default command it exits 2 unless the launcher binary and the launch config exist. Runs `cargo run --bin systemvmctl -- boot-smoke ... --log ${ROOT_DIR}/run/systemvm/boot.log --report ${ROOT_DIR}/run/systemvm/boot-report.json`.
- Exit codes: no explicit `exit` statements; under `set -e` each script exits with the status of the first failing command, otherwise with the exit status of `cargo run`.
- Makefile entry points: `make systemvm-fetch`, `make systemvm-build`, `make systemvm-launch-config`, `make systemvm-boot-smoke`.

## Invariants

- All four shell scripts use `set -euo pipefail`.
- All file-path arguments are absolute under the computed `ROOT_DIR`. `fetch-image.sh`, `build-image.sh` and `generate-launch-config.sh` `cd "${ROOT_DIR}"`; `boot-smoke.sh` never `cd`s, so its `cargo run` executes in the caller's cwd and fails outside the crate tree.
- `build-image.sh` and `generate-launch-config.sh` run `mkdir -p` on their output directories before `systemvmctl` writes to them.
- `fetch-image.sh` never trusts a download: the source and member digests are checked in bash, the unpacked-file digests plus `expected_image_id` in Rust, and any mismatch exits non-zero before the manifest is written. `generate-launch-config.sh` with no arguments requires a manifest (so a fetch happened) but `launch-config` itself only checks that strings are non-blank. `boot-smoke.sh` with the default command requires the launcher binary and the launch config.
- `fetch-image.sh` is idempotent: with a cached archive of the pinned digest it performs no network access.

## Side Effects

- `fetch-image.sh` creates `${ROOT_DIR}/build/systemvm/cache`, downloads about 80 MB into it, extracts `boot/` members there, writes the unpacked kernel, the initramfs and a copy of the ISO to `${ROOT_DIR}/build/systemvm/`, and spawns `cargo run -q --bin systemvmctl` twice (`lock-env`, `verify-image`) plus `python3 -I unpack-zboot.py`; `verify-image` writes `${ROOT_DIR}/build/systemvm/image-manifest.json`.
- `boot-smoke.sh` (default command) starts a Virtualization.framework VM per iteration through the launcher, which truncates and writes `${ROOT_DIR}/run/systemvm/console.log`.
- `build-image.sh` creates `${ROOT_DIR}/build/systemvm` and spawns `cargo run --bin systemvmctl build`, which writes `${ROOT_DIR}/build/systemvm/image-manifest.json`.
- `generate-launch-config.sh` creates `${ROOT_DIR}/build/systemvm` and `${ROOT_DIR}/run/systemvm`, and spawns `cargo run --bin systemvmctl launch-config`, which writes `${ROOT_DIR}/build/systemvm/launch-config.json` with `${ROOT_DIR}/run/systemvm/console.log` as the console log path.
- `boot-smoke.sh` creates `${ROOT_DIR}/run/systemvm` and spawns `cargo run --bin systemvmctl boot-smoke`, which writes `${ROOT_DIR}/run/systemvm/boot.log` and `${ROOT_DIR}/run/systemvm/boot-report.json`.
- Every `cargo run` may compile the crate, writing to `target/`.

## Verification

- Run: `make systemvm-fetch`, `make systemvm-build`, `make systemvm-launch-config`, `make systemvm-boot-smoke`
- CI: `.github/workflows/systemvm-image.yml` runs `systemvm-fetch`, `systemvm-build` and `systemvm-launch-config` on Linux (with `libarchive-tools`) and asserts the manifest `image_id` equals the lock's `expected_image_id`; `boot-smoke` with the launcher runs only on macOS developer hardware.
- Broader regression for `systemvmctl` and the `systemvm` module: `cargo test`
