#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VESPERD_FILE="${ROOT_DIR}/packaging/macos/vesperd.entitlements"
PROXY_FILE="${ROOT_DIR}/packaging/macos/vesper-proxy.entitlements"
LAUNCHER_FILE="${ROOT_DIR}/packaging/macos/vesper-vmlauncher.entitlements"

if [ "${VALIDATE_ENTITLEMENTS_FORCE_PYTHON:-0}" != "1" ] && command -v plutil >/dev/null 2>&1; then
  plutil -lint "${VESPERD_FILE}"
  plutil -lint "${PROXY_FILE}"
  plutil -lint "${LAUNCHER_FILE}"
  exit 0
fi

python3 - <<'PY' "${VESPERD_FILE}" "${PROXY_FILE}" "${LAUNCHER_FILE}"
import plistlib
import sys
from pathlib import Path

paths = [Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3])]
expected_keys = {
    paths[0]: {"com.apple.security.app-sandbox", "com.apple.security.virtualization"},
    paths[1]: {"com.apple.security.app-sandbox"},
    paths[2]: {"com.apple.security.virtualization"},
}

for path in paths:
    with path.open("rb") as handle:
        data = plistlib.load(handle)
    if not isinstance(data, dict):
        raise SystemExit(f"{path}: entitlement root must be a plist dictionary")
    missing = sorted(key for key in expected_keys[path] if data.get(key) is not True)
    if missing:
        raise SystemExit(
            f"{path}: missing required enabled entitlements: {', '.join(missing)}"
        )
    print(f"{path}: OK (validated with python plistlib)")
PY
