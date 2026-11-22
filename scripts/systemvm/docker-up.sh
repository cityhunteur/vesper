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

# Container mode (ADR 0010). The launch config on macOS says which mode this boot was asked for; the System VM
# says which one it runs. They must agree: a boot that reports unmapped containers without being asked to, or
# refuses its data disk, is not served.
containers_check() {
  local want got="" legacy="" h
  want="$(python3 -c 'import json,sys
c = json.load(open(sys.argv[1])).get("kernel_cmdline", "")
print("unmapped" if "vesper.containers=unmapped" in c.split() else "mapped")' "${LAUNCH_CONFIG}")"
  for _ in $(seq 1 60); do
    h="$(cargo run -q --bin systemvmctl -- control health 2>/dev/null || true)"
    got="$(printf '%s' "${h}" | sed -n 's/.*"containers":"\([^"]*\)".*/\1/p')"
    legacy="$(printf '%s' "${h}" | sed -n 's/.*"legacy_store":"\([^"]*\)".*/\1/p')"
    case "${got}" in mapped|unmapped|refused*) break ;; esac
    sleep 1
  done
  case "${got}" in
    "") echo "docker-up: the System VM did not report its container mode; rebuild the overlay (make systemvm-overlay) and restart" >&2; return 1 ;;
    refused*) echo "docker-up: the System VM did not start Docker: ${got} (the data disk belongs to the other container mode)" >&2; return 1 ;;
  esac
  if [ "${got}" != "${want}" ]; then
    echo "docker-up: the launch config asks for ${want} containers but the System VM reports ${got}; not serving Docker" >&2
    return 1
  fi
  if [ "${got}" = unmapped ] && [ "${VESPER_ALLOW_UNMAPPED:-0}" != 1 ]; then
    echo "docker-up: this boot runs unmapped containers (a container can be root on the System VM), which only the upstream suite's runner may serve. An interrupted run leaves this behind: restore build/systemvm/launch-config.upstream-saved.json over build/systemvm/launch-config.json, or run make systemvm-prepare, then make systemvm-down systemvm-up docker-up" >&2
    return 1
  fi
  if [ "${got}" = unmapped ]; then
    echo "docker-up: containers unmapped: this boot is for the upstream Firecracker suite only; a container can be root on the System VM (ADR 0010)"
  else
    echo "docker-up: containers mapped: no container is root on the System VM (ADR 0010)"
  fi
  case "${legacy}" in present*) echo "docker-up: the data disk also holds a Docker store from before container remapping (${legacy#present }); it is not used and not deleted. List its volumes with 'cargo run -q --bin systemvmctl -- control data legacy status'; import one with '... legacy import-volume NAME', or delete the store with '... data -- legacy delete --yes'" ;; esac
}

mkdir -p "${RUN_DIR}"
if [ -f "${PID_FILE}" ] && kill -0 "$(cat "${PID_FILE}")" 2>/dev/null; then
  containers_check || exit 1
  echo "docker-up: already running (pid $(cat "${PID_FILE}")); socket ${SOCKET}"
  exit 0
fi
containers_check || exit 1

cargo build -q --bin vesper-proxy
# Hardened runtime, ad hoc (ADR 0012); the proxy applies its own seatbelt profile at start.
bash "${ROOT_DIR}/scripts/macos/sign.sh" debug >/dev/null
# Published ports (GH #28): host loopback -> vesper-proxy -> bridge -> guest vesper-port-relay (vsock 2400).
PORT_ARGS=(--publish-ports --relay-port "${VESPER_PORT_RELAY:-2400}" --ports-status "${RUN_DIR}/ports.json")
: > "${LOG_FILE}"
nohup "${ROOT_DIR}/target/debug/vesper-proxy" \
  --listen "${SOCKET}" \
  --upstream "vsock://${CID}:${PORT}" \
  --vsock-socket-dir "${BRIDGE_DIR}" \
  --validate-upstream \
  "${PORT_ARGS[@]}" \
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
echo "docker-up: vesper-proxy pid ${PID}; DOCKER_HOST=unix://${SOCKET}; published ports via the guest relay (status ${RUN_DIR}/ports.json)"
ls -l "${SOCKET}"
