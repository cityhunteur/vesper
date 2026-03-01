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

echo "services stopped"
