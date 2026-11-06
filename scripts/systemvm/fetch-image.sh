#!/usr/bin/env bash
# Fetch the pinned System VM boot image (kernel + initramfs) named by
# docs/specs/systemvm-image.lock.yaml, verify every digest, and write
# build/systemvm/image-manifest.json. Idempotent: a cached archive with the
# pinned digest is reused and no network access happens.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LOCK_PATH="${1:-${ROOT_DIR}/docs/specs/systemvm-image.lock.yaml}"
IMAGE_DIR="${ROOT_DIR}/build/systemvm"
CACHE_DIR="${IMAGE_DIR}/cache"
MANIFEST_PATH="${IMAGE_DIR}/image-manifest.json"

mkdir -p "${CACHE_DIR}"
cd "${ROOT_DIR}"

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

# The lock is parsed once, in Rust, so the shell never interprets YAML.
eval "$(cargo run -q --bin systemvmctl -- lock-env --lock "${LOCK_PATH}")"

ARCHIVE_PATH="${CACHE_DIR}/$(basename "${VESPER_IMAGE_URL}")"

if [ -f "${ARCHIVE_PATH}" ] && [ "$(sha256_of "${ARCHIVE_PATH}")" = "${VESPER_IMAGE_SHA256}" ]; then
  echo "systemvm-fetch: using cached ${ARCHIVE_PATH}"
else
  echo "systemvm-fetch: downloading ${VESPER_IMAGE_URL}"
  rm -f "${ARCHIVE_PATH}"
  curl -fsSL --retry 3 --retry-delay 5 --max-time 1800 -o "${ARCHIVE_PATH}.part" "${VESPER_IMAGE_URL}"
  mv "${ARCHIVE_PATH}.part" "${ARCHIVE_PATH}"
fi

ACTUAL_SHA="$(sha256_of "${ARCHIVE_PATH}")"
if [ "${ACTUAL_SHA}" != "${VESPER_IMAGE_SHA256}" ]; then
  echo "systemvm-fetch: archive sha256 ${ACTUAL_SHA} does not match pinned ${VESPER_IMAGE_SHA256}" >&2
  exit 1
fi

tar -xzf "${ARCHIVE_PATH}" -C "${CACHE_DIR}" "${VESPER_IMAGE_KERNEL_MEMBER}" "${VESPER_IMAGE_INITRD_MEMBER}"
cp -f "${CACHE_DIR}/${VESPER_IMAGE_KERNEL_MEMBER}" "${IMAGE_DIR}/$(basename "${VESPER_IMAGE_KERNEL_MEMBER}")"
cp -f "${CACHE_DIR}/${VESPER_IMAGE_INITRD_MEMBER}" "${IMAGE_DIR}/$(basename "${VESPER_IMAGE_INITRD_MEMBER}")"

# Member digests and the expected image_id are checked in Rust.
cargo run -q --bin systemvmctl -- verify-image --lock "${LOCK_PATH}" --dir "${IMAGE_DIR}" --out "${MANIFEST_PATH}"
