#!/usr/bin/env bash
# ctr conformance for the Vesper shim (GH #26): the shim, bundle, restart and exec suites through `ctr` inside the
# System VM, merged with the baseline lock and `baselinectl` metadata into run/systemvm/ctr-conformance-report.json.
# Usage: ctr-conformance.sh [--publish]   (--publish copies a passing report to docs/specs/ctr-conformance-baseline.json,
#        which the CI workflow validates and uploads; needs `make systemvm-up`)
set -uo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"; cd "${ROOT_DIR}"
PUBLISH=0; for a in "$@"; do [ "$a" = --publish ] && PUBLISH=1; done
RUN="${ROOT_DIR}/run/systemvm"; mkdir -p "${RUN}"
[ -S run/systemvm/vsock/bridge.sock ] || [ -S run/systemvm/vsock/3.sock ] || make systemvm-up >/dev/null || { echo "ctr-conformance: System VM did not come up"; exit 2; }
cargo run -q --bin systemvmctl -- control shim-suite --report "${RUN}/shim-report.json" || true
cargo run -q --bin systemvmctl -- control bundle-suite --report "${RUN}/bundle-report.json" || true
cargo run -q --bin systemvmctl -- control restart-suite --report "${RUN}/restart-report.json" || true
cargo run -q --bin systemvmctl -- control exec-suite --report "${RUN}/exec-report.json" || true
CPU="$(sysctl -n machdep.cpu.brand_string 2>/dev/null | tr ' ' '-' | tr '[:upper:]' '[:lower:]' || uname -m)"
cargo run -q --bin baselinectl -- --mode generate --backend backend-a --host-os macos --host-cpu "${CPU}" --metadata "${RUN}/ctr-conformance-metadata.json" >/dev/null
COMMIT="$(git rev-parse --short HEAD 2>/dev/null || echo unknown)"
if [ -n "$(git status --porcelain)" ]; then COMMIT="${COMMIT}-dirty"; fi
python3 -I scripts/systemvm/ctr_conformance_report.py --out "${RUN}/ctr-conformance-report.json" --shim "${RUN}/shim-report.json" --bundle "${RUN}/bundle-report.json" --restart "${RUN}/restart-report.json" --exec "${RUN}/exec-report.json" --metadata "${RUN}/ctr-conformance-metadata.json" --host "$(sw_vers -productName 2>/dev/null) $(sw_vers -productVersion 2>/dev/null) / $(sysctl -n machdep.cpu.brand_string 2>/dev/null)" --commit "${COMMIT}"
rc=$?
if [ "${rc}" = 0 ] && [ "${PUBLISH}" = 1 ]; then cp "${RUN}/ctr-conformance-report.json" docs/specs/ctr-conformance-baseline.json; echo "ctr-conformance: published docs/specs/ctr-conformance-baseline.json"; fi
exit "${rc}"
