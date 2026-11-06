#!/usr/bin/env bash
# Build the overlay archive and the derived initramfs named by the image lock's
# overlay section, from systemvm/overlay and the verified package cache, then
# re-verify the image so build/systemvm/image-manifest.json records the result.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LOCK_PATH="${1:-${ROOT_DIR}/docs/specs/systemvm-image.lock.yaml}"
IMAGE_DIR="${ROOT_DIR}/build/systemvm"
APKS_DIR="${IMAGE_DIR}/cache/apks"

cd "${ROOT_DIR}"
eval "$(cargo run -q --bin systemvmctl -- lock-env --lock "${LOCK_PATH}")"

if [ -z "${VESPER_IMAGE_OVERLAY_FILE}" ]; then
  echo "build-overlay: the image lock has no overlay section; nothing to build"
  exit 0
fi
[ -f "${IMAGE_DIR}/${VESPER_IMAGE_INITRD_FILE}" ] || { echo "build-overlay: ${IMAGE_DIR}/${VESPER_IMAGE_INITRD_FILE} missing; run 'make systemvm-fetch' first" >&2; exit 2; }

PACKAGES_LOCK="${ROOT_DIR}/${VESPER_IMAGE_OVERLAY_PACKAGES_LOCK}"
cargo run -q --bin systemvmctl -- verify-packages --lock "${PACKAGES_LOCK}" --dir "${APKS_DIR}" >/dev/null

APK_ARGS=()
while IFS=$'\t' read -r file url sha; do
  APK_ARGS+=(--apk "${APKS_DIR}/${file}")
done < <(cargo run -q --bin systemvmctl -- package-list --lock "${PACKAGES_LOCK}")

python3 -I "${ROOT_DIR}/scripts/systemvm/build-initramfs.py" \
  --overlay-dir "${ROOT_DIR}/${VESPER_IMAGE_OVERLAY_DIR}" \
  "${APK_ARGS[@]}" \
  --base-initrd "${IMAGE_DIR}/${VESPER_IMAGE_INITRD_FILE}" \
  --apkovl-path "${VESPER_IMAGE_OVERLAY_APKOVL_PATH}" \
  --out-apkovl "${IMAGE_DIR}/localhost.apkovl.tar.gz" \
  --out-initrd "${IMAGE_DIR}/${VESPER_IMAGE_OVERLAY_FILE}"

cargo run -q --bin systemvmctl -- verify-image --require-overlay --lock "${LOCK_PATH}" --dir "${IMAGE_DIR}" --out "${IMAGE_DIR}/image-manifest.json"
