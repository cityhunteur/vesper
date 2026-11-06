#!/usr/bin/env bash
# Backend A parity baseline (GH #21): API conformance of the stock pinned-release Firecracker inside the
# System VM plus two performance runs, merged with the baseline lock, parity metadata and the compat
# exception registry into run/systemvm/parity-baseline-report.json.
# Usage: parity-baseline.sh [PERF_CYCLES] [--publish]   (default 20 cycles; --publish copies the report to
#        docs/specs/backend-a-parity-baseline.json, the capture CI validates and uploads)
set -uo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"
CYCLES=20; PUBLISH=0
for a in "$@"; do case "$a" in --publish) PUBLISH=1 ;; *) CYCLES="$a" ;; esac; done
RUN_DIR="${ROOT_DIR}/run/systemvm"; mkdir -p "${RUN_DIR}"
SUITE="${RUN_DIR}/parity-suite-report.json"
META="${RUN_DIR}/parity-metadata.json"
REPORT="${RUN_DIR}/parity-baseline-report.json"
HOST="$(sw_vers -productName 2>/dev/null) $(sw_vers -productVersion 2>/dev/null) / $(sysctl -n machdep.cpu.brand_string 2>/dev/null || uname -m)"
COMMIT="$(git rev-parse --short HEAD 2>/dev/null || echo unknown)"
CPU="$(sysctl -n machdep.cpu.brand_string 2>/dev/null | tr ' ' '-' | tr '[:upper:]' '[:lower:]' || uname -m)"

[ -S run/systemvm/vsock/bridge.sock ] || make systemvm-up >/dev/null || { echo "parity-baseline: System VM did not come up"; exit 2; }
cargo run -q --bin systemvmctl -- control parity-suite --report "${SUITE}" --perf-cycles "${CYCLES}" || true
cargo run -q --bin baselinectl -- --mode generate --backend backend-a --host-os macos --host-cpu "${CPU}" --metadata "${META}" >/dev/null
cargo run -q --bin compatexctl -- --registry docs/specs/compat-exceptions.yaml || { echo "parity-baseline: compat registry invalid"; exit 1; }
python3 -I scripts/systemvm/parity_baseline_report.py --out "${REPORT}" --suite "${SUITE}" --metadata "${META}" --host "${HOST}" --commit "${COMMIT}"
rc=$?
if [ "${rc}" = 0 ] && [ "${PUBLISH}" = 1 ]; then
  cp "${REPORT}" docs/specs/backend-a-parity-baseline.json
  echo "parity-baseline: published docs/specs/backend-a-parity-baseline.json"
fi
exit "${rc}"
