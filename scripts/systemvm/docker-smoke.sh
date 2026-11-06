#!/usr/bin/env bash
# GH #15 checklist through the canonical ingress: socket exists and is
# owner-only, `docker version` and `docker info` answer through vesper-proxy,
# a trivial container (busybox from the pinned ISO) runs, and raw sockets are
# not reachable from macOS. Writes run/systemvm/docker-smoke-report.json.
set -uo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SOCKET="${DOCKER_SOCKET:-${ROOT_DIR}/run/vesper/docker.sock}"
REPORT="${ROOT_DIR}/run/systemvm/docker-smoke-report.json"
TARBALL="${ROOT_DIR}/build/systemvm/trivial-rootfs.tar"
DOCKER="${DOCKER_BIN:-docker}"
export DOCKER_HOST="unix://${SOCKET}"
cd "${ROOT_DIR}"
mkdir -p "$(dirname "${REPORT}")"

steps=()
record() { # name ok detail
  local detail; detail="$(printf '%s' "$3" | tr '\n' ' ' | cut -c1-300 | sed 's/"/\\"/g')"
  steps+=("{\"name\":\"$1\",\"ok\":$2,\"detail\":\"${detail}\"}")
  if [ "$2" = true ]; then echo "ok   $1: ${detail:0:120}"; else echo "FAIL $1: ${detail:0:200}"; fi
}

if [ -S "${SOCKET}" ]; then
  mode="$(stat -f '%Lp' "${SOCKET}" 2>/dev/null || stat -c '%a' "${SOCKET}")"
  if [ "${mode}" = "600" ]; then record socket_exists_owner_only true "${SOCKET} mode ${mode}"; else record socket_exists_owner_only false "${SOCKET} mode ${mode}, expected 600"; fi
else
  record socket_exists_owner_only false "${SOCKET} is not a socket; run 'make docker-up'"
fi

out="$("${DOCKER}" version --format '{{.Server.Version}} {{.Server.Os}}/{{.Server.Arch}}' 2>&1)"; rc=$?
[ $rc -eq 0 ] && record docker_version true "server ${out}" || record docker_version false "${out}"

out="$("${DOCKER}" info --format '{{.ServerVersion}} containerd={{.ContainerdCommit.ID}} driver={{.Driver}} cgroup={{.CgroupVersion}}' 2>&1)"; rc=$?
[ $rc -eq 0 ] && record docker_info true "${out}" || record docker_info false "${out}"

if [ -f "${TARBALL}" ]; then
  out="$("${DOCKER}" import "${TARBALL}" vesper/trivial:local 2>&1)"; rc=$?
  [ $rc -eq 0 ] && record docker_import true "${out} ($(stat -f '%z' "${TARBALL}" 2>/dev/null || stat -c '%s' "${TARBALL}") bytes through the proxy)" || record docker_import false "${out}"
  out="$("${DOCKER}" run --rm vesper/trivial:local /bin/busybox echo vesper-container-ok 2>&1)"; rc=$?
  if [ $rc -eq 0 ] && [ "${out}" = "vesper-container-ok" ]; then record trivial_container_runs true "${out}"; else record trivial_container_runs false "rc=${rc} ${out}"; fi
else
  record docker_import false "${TARBALL} missing; run 'make systemvm-trivial-image'"
  record trivial_container_runs false "skipped"
fi

# Raw sockets: the only host sockets are the ingress and the owner-only bridge; the bridge
# cannot reach ports nothing listens on (containerd has no vsock listener).
others="$(find "${ROOT_DIR}/run" -type s ! -name docker.sock ! -name 3.sock ! -name vesperd.sock 2>/dev/null)"
if [ -z "${others}" ]; then record no_raw_host_sockets true "host sockets under run/: $(find "${ROOT_DIR}/run" -type s | sed "s|${ROOT_DIR}/||" | tr '\n' ' ')"; else record no_raw_host_sockets false "unexpected sockets: ${others}"; fi
out="$(cargo run -q --bin systemvmctl -- control --port 2376 handshake 2>&1)"; rc=$?
[ $rc -ne 0 ] && record raw_guest_port_denied true "guest port 2376 (no listener) refused: ${out}" || record raw_guest_port_denied false "unexpected success: ${out}"

passed=true; for s in "${steps[@]}"; do case "$s" in *'"ok":false'*) passed=false;; esac; done
printf '{"socket":"%s","steps":[%s],"passed":%s}\n' "${SOCKET}" "$(IFS=,; echo "${steps[*]}")" "${passed}" > "${REPORT}"
echo "docker smoke: passed=${passed} (report: ${REPORT})"
[ "${passed}" = true ]
