#!/usr/bin/env bash
# ADR 0012: signs the host binaries ad hoc with the hardened runtime and their plists, after each build. Arguments:
# the profile directory under target/ (default debug). Fails if a binary is missing or its signature does not verify.
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PROFILE="${1:-debug}"
for bin in vesper-proxy; do
  b="${ROOT_DIR}/target/${PROFILE}/${bin}"
  [ -x "$b" ] || { echo "sign: $b missing; build it first" >&2; exit 1; }
  codesign --force --sign - --options runtime --entitlements "${ROOT_DIR}/packaging/macos/${bin}.entitlements" "$b"
  codesign --verify --strict "$b"
  echo "sign: $b (ad hoc, hardened runtime)"
done
