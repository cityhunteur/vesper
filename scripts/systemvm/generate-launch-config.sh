#!/usr/bin/env bash
# Write build/systemvm/launch-config.json. With no arguments the config is derived
# from the image lock and the fetched files (kernel, initramfs, boot media, kernel
# command line, ready pattern). Positional overrides: KERNEL INITRD [ROOTFS].
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
IMAGE_DIR="${ROOT_DIR}/build/systemvm"
OUT="${IMAGE_DIR}/launch-config.json"
CONSOLE_LOG="${ROOT_DIR}/run/systemvm/console.log"

mkdir -p "${IMAGE_DIR}" "${ROOT_DIR}/run/systemvm"
cd "${ROOT_DIR}"

if [ $# -eq 0 ]; then
  [ -f "${IMAGE_DIR}/image-manifest.json" ] || { echo "generate-launch-config: ${IMAGE_DIR}/image-manifest.json missing; run 'make systemvm-fetch' first" >&2; exit 2; }
  # VESPER_SHARES: space-separated host directories to share over virtio-fs, each `PATH` or `PATH:ro` (GH #29).
  SHARE_ARGS=(); for s in ${VESPER_SHARES:-}; do SHARE_ARGS+=(--share "$s"); done
  exec cargo run --bin systemvmctl -- launch-config --from-lock --dir "${IMAGE_DIR}" --console-log "${CONSOLE_LOG}" --out "${OUT}" "${SHARE_ARGS[@]}"
fi

KERNEL_PATH="$1"
INITRD_PATH="${2:-}"
ROOTFS_PATH="${3:-}"
ARGS=(--kernel "${KERNEL_PATH}")
[ -n "${INITRD_PATH}" ] && ARGS+=(--initrd "${INITRD_PATH}")
[ -n "${ROOTFS_PATH}" ] && ARGS+=(--rootfs "${ROOTFS_PATH}")
exec cargo run --bin systemvmctl -- launch-config "${ARGS[@]}" --console-log "${CONSOLE_LOG}" --out "${OUT}"
