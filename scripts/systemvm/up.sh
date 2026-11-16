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

# A boot whose data disk belongs to the other container mode does not start Docker (ADR 0010), so the launcher's
# probe fails. Say so, from the System VM's console.
refused_reason() {
  local console why
  console="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("console_log_path") or "")' "${LAUNCH_CONFIG}" 2>/dev/null || true)"
  [ -n "${console}" ] || return 0
  why="$(grep -a 'vesper: CONTAINERS refused' "${console}" 2>/dev/null | tail -1 | tr -d '\r')"
  [ -n "${why}" ] && echo "systemvm-up: the System VM did not start Docker: ${why#vesper: } (the data disk belongs to the other container mode)" >&2
  return 0
}
for _ in $(seq 1 "${TIMEOUT}"); do
  if ! kill -0 "${PID}" 2>/dev/null; then
    refused_reason
    echo "systemvm-up: launcher exited early; see ${LOG_FILE}" >&2
    tail -5 "${LOG_FILE}" >&2
    rm -f "${PID_FILE}"
    exit 1
  fi
  if grep -q "${WANT}" "${LOG_FILE}" && { [ -z "${BRIDGE}" ] || [ -S "${ROOT_DIR}/${BRIDGE}" ] || [ -S "${BRIDGE}" ]; }; then
    echo "systemvm-up: running (pid ${PID}); bridge ${BRIDGE:-none}; log ${LOG_FILE}"
    grep -E '"status":"(ready|probe-ok|bridge-listening)"' "${LOG_FILE}"
    # virtio-fs shares (GH #29): mount each configured host directory in the guest at the same path.
    python3 -c 'import json,sys
for s in json.load(open(sys.argv[1])).get("shared_directories", []):
    print(s["tag"], s["host_path"], "ro" if s.get("read_only") else "rw")' "${LAUNCH_CONFIG}" | while read -r tag path mode; do
      if out="$(cargo run -q --bin systemvmctl -- control share mount "${tag}" "${path}" "${mode}" 2>&1)"; then
        echo "systemvm-up: ${out}"
      else
        echo "systemvm-up: share ${tag} -> ${path} failed: ${out}" >&2
      fi
    done
    exit 0
  fi
  sleep 1
done
refused_reason
echo "systemvm-up: timed out after ${TIMEOUT}s waiting for ${WANT}; see ${LOG_FILE}" >&2
kill "${PID}" 2>/dev/null || true
rm -f "${PID_FILE}"
exit 1
