#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BIN_DIR="${ROOT_DIR}/target/debug"
RUN_DIR="${ROOT_DIR}/run"

mkdir -p "${RUN_DIR}"

pushd "${ROOT_DIR}" >/dev/null

cargo build --bin vesperd --bin vesper-proxy

"${BIN_DIR}/vesperd" \
  --socket "${RUN_DIR}/vesperd.sock" \
  --baseline-lock "${ROOT_DIR}/docs/specs/design-v0.baselines.lock.yaml" \
  --swagger "${ROOT_DIR}/third_party/firecracker/firecracker.yaml" \
  --entitlements-file "${ROOT_DIR}/packaging/macos/vesperd.entitlements" \
  >"${RUN_DIR}/vesperd.log" 2>&1 &

echo $! >"${RUN_DIR}/vesperd.pid"

"${BIN_DIR}/vesper-proxy" \
  --listen "${RUN_DIR}/docker.sock" \
  --upstream "unix://${RUN_DIR}/vesperd.sock" \
  >"${RUN_DIR}/vesper-proxy.log" 2>&1 &

echo $! >"${RUN_DIR}/vesper-proxy.pid"

popd >/dev/null

echo "vesperd PID: $(cat "${RUN_DIR}/vesperd.pid")"
echo "vesper-proxy PID: $(cat "${RUN_DIR}/vesper-proxy.pid")"
