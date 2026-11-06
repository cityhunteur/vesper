#!/usr/bin/env bash
# Build the Swift Virtualization.framework helper and ad-hoc sign it with the
# virtualization entitlement. Output: build/vmlauncher/vesper-vmlauncher.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PACKAGE_DIR="${ROOT_DIR}/launcher"
OUT_DIR="${ROOT_DIR}/build/vmlauncher"
ENTITLEMENTS="${ROOT_DIR}/packaging/macos/vesper-vmlauncher.entitlements"
BINARY="${OUT_DIR}/vesper-vmlauncher"

if [ "$(uname -s)" != "Darwin" ]; then
  echo "build-vmlauncher: Virtualization.framework needs macOS; nothing to build on $(uname -s)" >&2
  exit 2
fi

swift build -c release --package-path "${PACKAGE_DIR}"
BUILT="$(swift build -c release --package-path "${PACKAGE_DIR}" --show-bin-path)/vesper-vmlauncher"

mkdir -p "${OUT_DIR}"
cp -f "${BUILT}" "${BINARY}"
codesign --force --sign - --entitlements "${ENTITLEMENTS}" "${BINARY}"
codesign --display --entitlements - "${BINARY}" 2>&1 | grep -q "com.apple.security.virtualization" \
  || { echo "build-vmlauncher: entitlement missing after signing" >&2; exit 1; }
echo "built and signed ${BINARY}"
