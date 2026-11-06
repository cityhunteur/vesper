#!/usr/bin/env bash
# Write build/systemvm/image-manifest.json. With no arguments the fetched image is
# re-verified against the lock (same as the end of fetch-image.sh). Positional
# overrides: KERNEL INITRD [ROOTFS] digest explicit files instead.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
IMAGE_DIR="${ROOT_DIR}/build/systemvm"
OUT="${IMAGE_DIR}/image-manifest.json"

mkdir -p "${IMAGE_DIR}"
cd "${ROOT_DIR}"

if [ $# -eq 0 ]; then
  exec cargo run --bin systemvmctl -- verify-image --dir "${IMAGE_DIR}" --out "${OUT}"
fi

KERNEL_PATH="$1"
INITRD_PATH="${2:-}"
ROOTFS_PATH="${3:-}"
ARGS=(--kernel "${KERNEL_PATH}")
[ -n "${INITRD_PATH}" ] && ARGS+=(--initrd "${INITRD_PATH}")
[ -n "${ROOTFS_PATH}" ] && ARGS+=(--rootfs "${ROOTFS_PATH}")
exec cargo run --bin systemvmctl -- build "${ARGS[@]}" --out "${OUT}"
