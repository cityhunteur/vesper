#!/usr/bin/env bash
# Boot the System VM launch config N times through vesper-vmlauncher and
# write run/systemvm/boot.log plus boot-report.json. Positional overrides:
# ITERATIONS TIMEOUT COMMAND ARGS (ARGS comma-separated).
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LAUNCHER="${ROOT_DIR}/build/vmlauncher/vesper-vmlauncher"
LAUNCH_CONFIG="${ROOT_DIR}/build/systemvm/launch-config.json"
ITERATIONS="${1:-5}"
TIMEOUT="${2:-60}"
COMMAND="${3:-${LAUNCHER}}"
ARGS="${4:---config,${LAUNCH_CONFIG},--timeout-seconds,${TIMEOUT}}"

if [ "${COMMAND}" = "${LAUNCHER}" ]; then
  [ -x "${LAUNCHER}" ] || { echo "boot-smoke: ${LAUNCHER} missing; run 'make vmlauncher-build' first" >&2; exit 2; }
  [ -f "${LAUNCH_CONFIG}" ] || { echo "boot-smoke: ${LAUNCH_CONFIG} missing; run 'make systemvm-fetch systemvm-launch-config' first" >&2; exit 2; }
fi

mkdir -p "${ROOT_DIR}/run/systemvm"

# The boot-time threshold comes from the image lock (boot.max_boot_ms); MAX_BOOT_MS overrides it.
if [ -z "${MAX_BOOT_MS:-}" ]; then
  eval "$(cargo run -q --bin systemvmctl -- lock-env --lock "${ROOT_DIR}/docs/specs/systemvm-image.lock.yaml" | grep '^VESPER_IMAGE_MAX_BOOT_MS=')"
  MAX_BOOT_MS="${VESPER_IMAGE_MAX_BOOT_MS}"
fi

cargo run --bin systemvmctl -- boot-smoke \
  --iterations "${ITERATIONS}" \
  --timeout-seconds "$((TIMEOUT + 10))" \
  --command "${COMMAND}" \
  --args="${ARGS}" \
  --max-boot-ms "${MAX_BOOT_MS}" \
  --log "${ROOT_DIR}/run/systemvm/boot.log" \
  --report "${ROOT_DIR}/run/systemvm/boot-report.json"
