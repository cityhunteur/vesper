#!/usr/bin/env bash
# Start the System VM in the background with the vsock bridge and keep it
# running: launcher with --keep-running, pid in run/systemvm/vmlauncher.pid,
# status lines in run/systemvm/vmlauncher.log. Waits until the probe (or the
# ready pattern when there is no probe) succeeds and the bridge socket exists.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LAUNCHER="${ROOT_DIR}/build/vmlauncher/vesper-vmlauncher"
LAUNCH_CONFIG="${ROOT_DIR}/build/systemvm/launch-config.json"
RUN_DIR="${ROOT_DIR}/run/systemvm"
PID_FILE="${RUN_DIR}/vmlauncher.pid"
LOG_FILE="${RUN_DIR}/vmlauncher.log"
TIMEOUT="${1:-180}"

[ -x "${LAUNCHER}" ] || { echo "systemvm-up: ${LAUNCHER} missing; run 'make vmlauncher-build'" >&2; exit 2; }
[ -f "${LAUNCH_CONFIG}" ] || { echo "systemvm-up: ${LAUNCH_CONFIG} missing; run 'make systemvm-prepare'" >&2; exit 2; }
mkdir -p "${RUN_DIR}"

if [ -f "${PID_FILE}" ] && kill -0 "$(cat "${PID_FILE}")" 2>/dev/null; then
  echo "systemvm-up: already running (pid $(cat "${PID_FILE}"))"
  exit 0
fi

BRIDGE="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("vsock_bridge_path") or "")' "${LAUNCH_CONFIG}")"
HAS_PROBE="$(python3 -c 'import json,sys; print("1" if json.load(open(sys.argv[1])).get("probe") else "")' "${LAUNCH_CONFIG}")"
WANT='"status":"ready"'
[ -n "${HAS_PROBE}" ] && WANT='"status":"probe-ok"'

: > "${LOG_FILE}"
# Fully detached: no inherited stdin/stdout, so callers (make, CI, agents) are not held open.
nohup "${LAUNCHER}" --config "${LAUNCH_CONFIG}" --keep-running --timeout-seconds "${TIMEOUT}" < /dev/null >> "${LOG_FILE}" 2>&1 &
PID=$!
echo "${PID}" > "${PID_FILE}"

for _ in $(seq 1 "${TIMEOUT}"); do
  if ! kill -0 "${PID}" 2>/dev/null; then
    echo "systemvm-up: launcher exited early; see ${LOG_FILE}" >&2
    tail -5 "${LOG_FILE}" >&2
    rm -f "${PID_FILE}"
    exit 1
  fi
  if grep -q "${WANT}" "${LOG_FILE}" && { [ -z "${BRIDGE}" ] || [ -S "${ROOT_DIR}/${BRIDGE}" ] || [ -S "${BRIDGE}" ]; }; then
    echo "systemvm-up: running (pid ${PID}); bridge ${BRIDGE:-none}; log ${LOG_FILE}"
    grep -E '"status":"(ready|probe-ok|bridge-listening)"' "${LOG_FILE}"
    exit 0
  fi
  sleep 1
done
echo "systemvm-up: timed out after ${TIMEOUT}s waiting for ${WANT}; see ${LOG_FILE}" >&2
kill "${PID}" 2>/dev/null || true
rm -f "${PID_FILE}"
exit 1
