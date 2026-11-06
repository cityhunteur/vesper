#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
KERNEL_PATH="${1:-${ROOT_DIR}/build/systemvm/vmlinuz-virt}"
INITRD_PATH="${2:-${ROOT_DIR}/build/systemvm/initramfs-virt}"
ROOTFS_PATH="${3:-}"

if [ ! -f "${KERNEL_PATH}" ] || [ ! -f "${INITRD_PATH}" ]; then
  echo "build-image: missing ${KERNEL_PATH} or ${INITRD_PATH}; run 'make systemvm-fetch' first" >&2
  exit 2
fi

mkdir -p "${ROOT_DIR}/build/systemvm"

ARGS=(--kernel "${KERNEL_PATH}" --initrd "${INITRD_PATH}")
if [ -n "${ROOTFS_PATH}" ]; then
  ARGS+=(--rootfs "${ROOTFS_PATH}")
fi

cargo run --bin systemvmctl -- build "${ARGS[@]}" \
  --out "${ROOT_DIR}/build/systemvm/image-manifest.json"
