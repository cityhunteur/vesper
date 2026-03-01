#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
KERNEL_PATH="${1:-${ROOT_DIR}/assets/systemvm/vmlinux}"
ROOTFS_PATH="${2:-${ROOT_DIR}/assets/systemvm/rootfs.ext4}"

mkdir -p "${ROOT_DIR}/build/systemvm"

cargo run --bin systemvmctl -- build \
  --kernel "${KERNEL_PATH}" \
  --rootfs "${ROOTFS_PATH}" \
  --out "${ROOT_DIR}/build/systemvm/image-manifest.json"
