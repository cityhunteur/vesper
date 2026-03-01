#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
KERNEL_PATH="${1:-${ROOT_DIR}/assets/systemvm/vmlinux}"
ROOTFS_PATH="${2:-${ROOT_DIR}/assets/systemvm/rootfs.ext4}"

mkdir -p "${ROOT_DIR}/build/systemvm" "${ROOT_DIR}/run/systemvm"

cargo run --bin systemvmctl -- launch-config \
  --kernel "${KERNEL_PATH}" \
  --rootfs "${ROOTFS_PATH}" \
  --console-log "${ROOT_DIR}/run/systemvm/console.log" \
  --out "${ROOT_DIR}/build/systemvm/launch-config.json"
