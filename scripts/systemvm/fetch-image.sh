#!/usr/bin/env bash
# Fetch the pinned System VM boot image named by docs/specs/systemvm-image.lock.yaml,
# verify every digest, unpack the kernel, and write build/systemvm/image-manifest.json.
# Idempotent: a cached source file with the pinned digest is reused and no network
# access happens.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LOCK_PATH="${1:-${ROOT_DIR}/docs/specs/systemvm-image.lock.yaml}"
IMAGE_DIR="${ROOT_DIR}/build/systemvm"
CACHE_DIR="${IMAGE_DIR}/cache"
MANIFEST_PATH="${IMAGE_DIR}/image-manifest.json"
UNPACK_ZBOOT="${ROOT_DIR}/scripts/systemvm/unpack-zboot.py"

mkdir -p "${CACHE_DIR}"
cd "${ROOT_DIR}"

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

require_digest() { # label path expected
  local actual
  actual="$(sha256_of "$2")"
  if [ "${actual}" != "$3" ]; then
    echo "systemvm-fetch: $1 sha256 ${actual} does not match pinned $3" >&2
    exit 1
  fi
}

# bsdtar (libarchive) reads ISO 9660 images; GNU tar does not.
archive_tool() {
  if command -v bsdtar >/dev/null 2>&1; then
    echo bsdtar
  elif tar --version 2>/dev/null | grep -q bsdtar; then
    echo tar
  else
    echo "systemvm-fetch: need bsdtar (libarchive) to read the image; on Debian/Ubuntu: apt-get install libarchive-tools" >&2
    exit 1
  fi
}

# The lock is parsed once, in Rust, so the shell never interprets YAML.
eval "$(cargo run -q --bin systemvmctl -- lock-env --lock "${LOCK_PATH}")"

SOURCE_PATH="${CACHE_DIR}/$(basename "${VESPER_IMAGE_URL}")"

if [ -f "${SOURCE_PATH}" ] && [ "$(sha256_of "${SOURCE_PATH}")" = "${VESPER_IMAGE_SHA256}" ]; then
  echo "systemvm-fetch: using cached ${SOURCE_PATH}"
else
  echo "systemvm-fetch: downloading ${VESPER_IMAGE_URL}"
  rm -f "${SOURCE_PATH}"
  curl -fsSL --retry 3 --retry-delay 5 --max-time 1800 -o "${SOURCE_PATH}.part" "${VESPER_IMAGE_URL}"
  mv "${SOURCE_PATH}.part" "${SOURCE_PATH}"
fi
require_digest "source" "${SOURCE_PATH}" "${VESPER_IMAGE_SHA256}"

TAR="$(archive_tool)"
"${TAR}" -xf "${SOURCE_PATH}" -C "${CACHE_DIR}" "${VESPER_IMAGE_KERNEL_MEMBER}" "${VESPER_IMAGE_INITRD_MEMBER}"
require_digest "kernel member" "${CACHE_DIR}/${VESPER_IMAGE_KERNEL_MEMBER}" "${VESPER_IMAGE_KERNEL_MEMBER_SHA256}"
require_digest "initrd member" "${CACHE_DIR}/${VESPER_IMAGE_INITRD_MEMBER}" "${VESPER_IMAGE_INITRD_MEMBER_SHA256}"

unpack() { # method src dst
  case "$1" in
    none) cp -f "$2" "$3" ;;
    zboot-gzip) python3 -I "${UNPACK_ZBOOT}" "$2" "$3" ;;
    *) echo "systemvm-fetch: unknown unpack method $1" >&2; exit 1 ;;
  esac
}
unpack "${VESPER_IMAGE_KERNEL_UNPACK}" "${CACHE_DIR}/${VESPER_IMAGE_KERNEL_MEMBER}" "${IMAGE_DIR}/${VESPER_IMAGE_KERNEL_FILE}"
unpack "${VESPER_IMAGE_INITRD_UNPACK}" "${CACHE_DIR}/${VESPER_IMAGE_INITRD_MEMBER}" "${IMAGE_DIR}/${VESPER_IMAGE_INITRD_FILE}"
if [ -n "${VESPER_IMAGE_BOOT_MEDIA_FILE}" ]; then
  cp -f "${SOURCE_PATH}" "${IMAGE_DIR}/${VESPER_IMAGE_BOOT_MEDIA_FILE}"
fi

# Final digests and the expected image_id are checked in Rust.
cargo run -q --bin systemvmctl -- verify-image --lock "${LOCK_PATH}" --dir "${IMAGE_DIR}" --out "${MANIFEST_PATH}"
