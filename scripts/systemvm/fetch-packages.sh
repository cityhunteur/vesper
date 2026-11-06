#!/usr/bin/env bash
# Download the pinned System VM packages (docs/specs/systemvm-packages.lock.yaml)
# into build/systemvm/cache/apks and verify every digest. Cached files with the
# pinned digest are reused without network access.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LOCK_PATH="${1:-${ROOT_DIR}/docs/specs/systemvm-packages.lock.yaml}"
APKS_DIR="${ROOT_DIR}/build/systemvm/cache/apks"

mkdir -p "${APKS_DIR}"
cd "${ROOT_DIR}"

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

downloaded=0
while IFS=$'\t' read -r file url sha; do
  path="${APKS_DIR}/${file}"
  if [ -f "${path}" ] && [ "$(sha256_of "${path}")" = "${sha}" ]; then
    continue
  fi
  echo "fetch-packages: downloading ${url}"
  rm -f "${path}"
  curl -fsSL --retry 3 --retry-delay 5 --max-time 600 -o "${path}.part" "${url}"
  mv "${path}.part" "${path}"
  downloaded=$((downloaded + 1))
done < <(cargo run -q --bin systemvmctl -- package-list --lock "${LOCK_PATH}")

echo "fetch-packages: ${downloaded} downloaded"
cargo run -q --bin systemvmctl -- verify-packages --lock "${LOCK_PATH}" --dir "${APKS_DIR}"
