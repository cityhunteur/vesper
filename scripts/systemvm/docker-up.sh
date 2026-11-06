#!/usr/bin/env bash
# Expose the System VM's Docker API on the host through vesper-proxy:
# run/vesper/docker.sock (owner-only) -> vesper-proxy -> bridge
# run/systemvm/vsock/3.sock -> guest vsock 2375 -> /var/run/docker.sock.
# Requires `make systemvm-up`. Writes run/vesper/vesper-proxy.{pid,log}.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LAUNCH_CONFIG="${ROOT_DIR}/build/systemvm/launch-config.json"
RUN_DIR="${ROOT_DIR}/run/vesper"
SOCKET="${DOCKER_SOCKET:-${RUN_DIR}/docker.sock}"
PID_FILE="${RUN_DIR}/vesper-proxy.pid"
LOG_FILE="${RUN_DIR}/vesper-proxy.log"
PORT="${VESPER_DOCKER_PORT:-2375}"

cd "${ROOT_DIR}"
[ -f "${LAUNCH_CONFIG}" ] || { echo "docker-up: ${LAUNCH_CONFIG} missing; run 'make systemvm-prepare'" >&2; exit 2; }
BRIDGE="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("vsock_bridge_path") or "")' "${LAUNCH_CONFIG}")"
[ -n "${BRIDGE}" ] || { echo "docker-up: launch config has no vsock_bridge_path" >&2; exit 2; }
[ -S "${BRIDGE}" ] || { echo "docker-up: bridge ${BRIDGE} is not up; run 'make systemvm-up'" >&2; exit 2; }
BRIDGE_DIR="$(dirname "${BRIDGE}")"
CID="$(basename "${BRIDGE}" .sock)"

mkdir -p "${RUN_DIR}"
if [ -f "${PID_FILE}" ] && kill -0 "$(cat "${PID_FILE}")" 2>/dev/null; then
  echo "docker-up: already running (pid $(cat "${PID_FILE}")); socket ${SOCKET}"
  exit 0
fi

cargo build -q --bin vesper-proxy
: > "${LOG_FILE}"
nohup "${ROOT_DIR}/target/debug/vesper-proxy" \
  --listen "${SOCKET}" \
  --upstream "vsock://${CID}:${PORT}" \
  --vsock-socket-dir "${BRIDGE_DIR}" \
  --validate-upstream \
  < /dev/null >> "${LOG_FILE}" 2>&1 &
PID=$!
echo "${PID}" > "${PID_FILE}"
for _ in $(seq 1 20); do
  if ! kill -0 "${PID}" 2>/dev/null; then
    echo "docker-up: vesper-proxy exited; see ${LOG_FILE}" >&2; cat "${LOG_FILE}" >&2; rm -f "${PID_FILE}"; exit 1
  fi
  [ -S "${SOCKET}" ] && break
  sleep 0.5
done
[ -S "${SOCKET}" ] || { echo "docker-up: ${SOCKET} did not appear" >&2; exit 1; }
echo "docker-up: vesper-proxy pid ${PID}; DOCKER_HOST=unix://${SOCKET}"
ls -l "${SOCKET}"
