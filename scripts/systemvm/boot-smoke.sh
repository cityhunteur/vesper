#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
mkdir -p "${ROOT_DIR}/run/systemvm"

cargo run --bin systemvmctl -- boot-smoke \
  --iterations "${1:-5}" \
  --timeout-seconds "${2:-20}" \
  --command "${3:-sh}" \
  --args="${4:--c,echo boot-ok}" \
  --log "${ROOT_DIR}/run/systemvm/boot.log" \
  --report "${ROOT_DIR}/run/systemvm/boot-report.json"
