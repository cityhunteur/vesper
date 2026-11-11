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
  # VESPER_DATA_DISK: persistent data disk image (GH #164); default run/systemvm/data.img, "none" for tmpfs only.
  # Created sparse on first use with VESPER_DATA_DISK_GIB (default 64) GiB; the System VM formats it on first boot.
  DATA_ARGS=()
  DATA_DISK="${VESPER_DATA_DISK:-${ROOT_DIR}/run/systemvm/data.img}"
  if [ "${DATA_DISK}" != none ]; then
    case "${DATA_DISK}" in /*) ;; *) DATA_DISK="${ROOT_DIR}/${DATA_DISK}" ;; esac
    if [ ! -f "${DATA_DISK}" ]; then
      mkdir -p "$(dirname "${DATA_DISK}")"
      dd if=/dev/zero of="${DATA_DISK}" bs=1 count=0 seek="${VESPER_DATA_DISK_GIB:-64}g" 2>/dev/null \
        || { echo "generate-launch-config: cannot create ${DATA_DISK}" >&2; exit 2; }
      # Container images and volumes may hold secrets: readable by the owner only.
      chmod 600 "${DATA_DISK}"
      echo "generate-launch-config: created sparse data disk ${DATA_DISK} (${VESPER_DATA_DISK_GIB:-64} GiB)"
    fi
    DATA_ARGS=(--data-disk "${DATA_DISK}")
  fi
  exec cargo run --bin systemvmctl -- launch-config --from-lock --dir "${IMAGE_DIR}" --console-log "${CONSOLE_LOG}" --out "${OUT}" "${SHARE_ARGS[@]}" "${DATA_ARGS[@]}"
fi

KERNEL_PATH="$1"
INITRD_PATH="${2:-}"
ROOTFS_PATH="${3:-}"
ARGS=(--kernel "${KERNEL_PATH}")
[ -n "${INITRD_PATH}" ] && ARGS+=(--initrd "${INITRD_PATH}")
[ -n "${ROOTFS_PATH}" ] && ARGS+=(--rootfs "${ROOTFS_PATH}")
exec cargo run --bin systemvmctl -- launch-config "${ARGS[@]}" --console-log "${CONSOLE_LOG}" --out "${OUT}"
