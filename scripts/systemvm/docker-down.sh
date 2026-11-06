#!/usr/bin/env bash
# Stop the vesper-proxy started by docker-up.sh (SIGTERM) and remove its socket.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
RUN_DIR="${ROOT_DIR}/run/vesper"
SOCKET="${DOCKER_SOCKET:-${RUN_DIR}/docker.sock}"
PID_FILE="${RUN_DIR}/vesper-proxy.pid"

if [ -f "${PID_FILE}" ]; then
  PID="$(cat "${PID_FILE}")"
  if kill -0 "${PID}" 2>/dev/null; then
    kill "${PID}" 2>/dev/null || true
    for _ in $(seq 1 10); do kill -0 "${PID}" 2>/dev/null || break; sleep 0.5; done
    kill -0 "${PID}" 2>/dev/null && kill -9 "${PID}" 2>/dev/null || true
  fi
  rm -f "${PID_FILE}"
fi
rm -f "${SOCKET}"
echo "docker-down: stopped"
