#!/usr/bin/env bash
# Owner-hardware evidence (TD-005, D-084): the System VM reliability suite and the System VM suites on this Mac, one
# bundle with the git SHA, the baseline metadata and every report (scripts/ci/nightly_evidence.py). GitHub's macOS
# runners cannot boot the System VM, so this runs here; its bundle never counts as remote green.
#   make nightly                  run into run/nightly/<UTC time>/bundle
#   make nightly PUBLISH=1        also copy the bundle to docs/generated/nightly/ to commit (CI validates it, uploads
#                                 it as an artifact, and warns when it is old)
# CYCLES (default 10) sets the reliability suite's boot cycles; NIGHTLY_SUITES replaces the suite list. It stops
# Docker and the System VM first, and leaves both running with the snapshot share (make systemvm-up-snapshot).
# A scheduled run is documented in docs/process/nightly-evidence.md (a launchd agent; not installed by anything).
set -uo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"
CYCLES="${CYCLES:-10}"
SUITES="${NIGHTLY_SUITES:-systemvm-control-suite systemvm-services-suite systemvm-launchapi-suite systemvm-vesperd-suite systemvm-snapshot-suite systemvm-replay-suite docker-conformance ports-suite}"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
OUT="${ROOT_DIR}/run/nightly/${STAMP}"
RESULTS="${OUT}/results.tsv"
mkdir -p "${OUT}/logs"
: > "${RESULTS}"
STARTED="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
SHA="$(git rev-parse HEAD)"
DIRTY=()
[ -n "$(git status --porcelain --untracked-files=no)" ] && DIRTY=(--git-dirty)
say() { echo "nightly: $*" | tee -a "${OUT}/nightly.log"; }

# The report each target writes under run/systemvm/.
report_for() {
  case "$1" in
    systemvm-reliability) echo reliability ;;
    systemvm-*-suite) local n="${1#systemvm-}"; echo "${n%-suite}" ;;
    *-suite) echo "${1%-suite}" ;;
    *) echo "$1" ;;
  esac
}

run_target() { # target [make args...]
  local target="$1" start rc
  shift
  start="$(date +%s)"
  say "${target}: started"
  make "${target}" "$@" < /dev/null > "${OUT}/logs/${target}.log" 2>&1
  rc=$?
  say "${target}: exit ${rc}"
  printf '%s\t%s\t%s\t%s\n' "${target}" "${rc}" "${ROOT_DIR}/run/systemvm/$(report_for "${target}")-report.json" \
    "${start}" >> "${RESULTS}"
}

say "commit ${SHA}${DIRTY[0]:+ (tree has changes not committed)}; cycles ${CYCLES}; suites: ${SUITES}"
# The launcher the suites boot is built from this commit, so the bundle's SHA names what ran.
make -s vmlauncher-build < /dev/null > "${OUT}/logs/vmlauncher-build.log" 2>&1 || { say "vmlauncher-build failed"; exit 2; }
make -s docker-down < /dev/null > /dev/null 2>&1 || true
make -s systemvm-down < /dev/null > /dev/null 2>&1 || true
# The reliability suite boots and stops its own System VMs.
run_target systemvm-reliability CYCLES="${CYCLES}"
if make -s systemvm-up-snapshot < /dev/null > "${OUT}/logs/systemvm-up.log" 2>&1 \
  && make -s docker-up < /dev/null > "${OUT}/logs/docker-up.log" 2>&1; then
  for target in ${SUITES}; do run_target "${target}"; done
else
  # No System VM: each suite is recorded as failed with no report, so the bundle says so.
  say "the System VM or the Docker socket did not come up; see ${OUT}/logs"
  for target in ${SUITES}; do printf '%s\t%s\t%s\t%s\n' "${target}" 99 /nonexistent "$(date +%s)" >> "${RESULTS}"; done
fi

CPU="$(sysctl -n machdep.cpu.brand_string 2>/dev/null | tr ' ' '-' | tr '[:upper:]' '[:lower:]' || uname -m)"
cargo run -q --bin baselinectl -- --mode generate --backend backend-a --host-os macos --host-cpu "${CPU}" \
  --metadata "${OUT}/parity-metadata.json" > /dev/null 2>&1 || say "baseline metadata: baselinectl failed"
HOST="$(sw_vers -productName 2>/dev/null) $(sw_vers -productVersion 2>/dev/null) / $(sysctl -n machdep.cpu.brand_string 2>/dev/null || uname -m)"
python3 -I scripts/ci/nightly_evidence.py --build "${OUT}/bundle" --results "${RESULTS}" --git-sha "${SHA}" \
  ${DIRTY[@]+"${DIRTY[@]}"} --started-at "${STARTED}" --host "${HOST}" --metadata "${OUT}/parity-metadata.json" \
  | tee -a "${OUT}/nightly.log"
rc=${PIPESTATUS[0]}
if [ "${PUBLISH:-0}" = 1 ]; then
  rm -rf "${ROOT_DIR:?}/docs/generated/nightly"
  cp -R "${OUT}/bundle" "${ROOT_DIR}/docs/generated/nightly"
  say "published docs/generated/nightly; commit it to update the evidence CI checks"
fi
exit "${rc}"
