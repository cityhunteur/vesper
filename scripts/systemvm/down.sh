#!/usr/bin/env bash
# Stop the System VM started by up.sh: ask the guest agent to power off, wait
# for the launcher to exit, fall back to SIGTERM (launcher stops the VM).
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
RUN_DIR="${ROOT_DIR}/run/systemvm"
PID_FILE="${RUN_DIR}/vmlauncher.pid"
LAUNCH_CONFIG="${ROOT_DIR}/build/systemvm/launch-config.json"
WAIT="${1:-30}"

if [ ! -f "${PID_FILE}" ]; then
  echo "systemvm-down: not running (no ${PID_FILE})"
  exit 0
fi
PID="$(cat "${PID_FILE}")"
if ! kill -0 "${PID}" 2>/dev/null; then
  echo "systemvm-down: stale pid file removed"
  rm -f "${PID_FILE}"
  exit 0
fi

cd "${ROOT_DIR}"
if cargo run -q --bin systemvmctl -- control --config "${LAUNCH_CONFIG}" shutdown 2>/dev/null; then
  echo "systemvm-down: guest acknowledged shutdown"
else
  echo "systemvm-down: agent unreachable; sending SIGTERM to the launcher"
  kill "${PID}" 2>/dev/null || true
fi
for _ in $(seq 1 "${WAIT}"); do
  kill -0 "${PID}" 2>/dev/null || { rm -f "${PID_FILE}"; echo "systemvm-down: stopped"; exit 0; }
  sleep 1
done
echo "systemvm-down: launcher still running after ${WAIT}s; sending SIGTERM" >&2
kill "${PID}" 2>/dev/null || true
for _ in $(seq 1 10); do
  kill -0 "${PID}" 2>/dev/null || { rm -f "${PID_FILE}"; echo "systemvm-down: stopped"; exit 0; }
  sleep 1
done
echo "systemvm-down: launcher did not exit" >&2
exit 1
