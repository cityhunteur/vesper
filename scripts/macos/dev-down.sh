#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
RUN_DIR="${ROOT_DIR}/run"

for name in vesper-proxy vesperd; do
  pid_file="${RUN_DIR}/${name}.pid"
  if [[ -f "${pid_file}" ]]; then
    pid="$(cat "${pid_file}")"
    if kill -0 "${pid}" >/dev/null 2>&1; then
      kill "${pid}" || true
    fi
    rm -f "${pid_file}"
  fi
done

# A process killed before it could clean up leaves its socket; the reliability suite counts any socket under run/ as a
# leak. Only the dev loop's two sockets, and only sockets.
for sock in vesperd.sock docker.sock; do
  if [[ -S "${RUN_DIR}/${sock}" ]]; then rm -f "${RUN_DIR:?}/${sock}"; fi
done

echo "services stopped"
