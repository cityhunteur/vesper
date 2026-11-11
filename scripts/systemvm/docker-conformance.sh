#!/usr/bin/env bash
# GH #27 checklist through the canonical ingress (run/vesper/docker.sock -> vesper-proxy -> guest dockerd):
# core Docker CLI commands, context management and auth header passthrough, protocol and error passthrough
# for unsupported operations, and the ingress boundary. Writes run/systemvm/docker-conformance-report.json.
# Usage: docker-conformance.sh   (needs `make systemvm-up && make docker-up`; PULL_IMAGE overrides alpine:3.22)
set -uo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SOCKET="${DOCKER_SOCKET:-${ROOT_DIR}/run/vesper/docker.sock}"
REPORT="${ROOT_DIR}/run/systemvm/docker-conformance-report.json"
TARBALL="${ROOT_DIR}/build/systemvm/trivial-rootfs.tar"
DOCKER="${DOCKER_BIN:-docker}"
PULL_IMAGE="${PULL_IMAGE:-alpine:3.22}"
IMG=vesper/trivial:conf
export DOCKER_HOST="unix://${SOCKET}"
cd "${ROOT_DIR}"
mkdir -p "$(dirname "${REPORT}")"

steps=()
record() { # name ok detail
  local detail; detail="$(printf '%s' "$3" | tr '\n' ' ' | cut -c1-300 | sed 's/\\/\\\\/g; s/"/\\"/g')"
  steps+=("{\"name\":\"$1\",\"ok\":$2,\"detail\":\"${detail}\"}")
  if [ "$2" = true ]; then echo "ok   $1: ${detail:0:140}"; else echo "FAIL $1: ${detail:0:200}"; fi
}
cleanup() { "${DOCKER}" rm -f vc1 vc2 vc3 >/dev/null 2>&1; "${DOCKER}" context rm -f vesper-conformance >/dev/null 2>&1; }
cleanup

[ -S "${SOCKET}" ] || { echo "docker-conformance: ${SOCKET} is not a socket; run 'make docker-up'" >&2; exit 2; }
[ -f "${TARBALL}" ] || { echo "docker-conformance: ${TARBALL} missing; run 'make systemvm-trivial-image'" >&2; exit 2; }

# Protocol: raw HTTP ping and version through the proxy.
out="$(curl -s --max-time 10 --unix-socket "${SOCKET}" http://localhost/_ping 2>&1)"
[ "${out}" = "OK" ] && record api_ping true "GET /_ping -> OK" || record api_ping false "GET /_ping -> ${out}"
out="$(curl -s --max-time 10 --unix-socket "${SOCKET}" http://localhost/version 2>&1 | python3 -c 'import json,sys; v=json.load(sys.stdin); print(v["Version"], v["ApiVersion"], v["Os"]+"/"+v["Arch"])' 2>&1)"; rc=$?
[ $rc -eq 0 ] && record api_version true "GET /version -> ${out}" || record api_version false "${out}"

# Context management: a named context pointing at the ingress works without DOCKER_HOST.
out="$(env -u DOCKER_HOST "${DOCKER}" context create vesper-conformance --docker "host=unix://${SOCKET}" 2>&1 && env -u DOCKER_HOST "${DOCKER}" --context vesper-conformance version --format '{{.Server.Version}}' 2>&1)"; rc=$?
[ $rc -eq 0 ] && record context_management true "context vesper-conformance -> server $(echo "${out}" | tail -1)" || record context_management false "${out}"

# Images: import, list, inspect (architecture metadata), remove later.
out="$("${DOCKER}" import "${TARBALL}" "${IMG}" 2>&1)"; rc=$?
[ $rc -eq 0 ] && record images_import true "${out}" || record images_import false "${out}"
out="$("${DOCKER}" images --format '{{.Repository}}:{{.Tag}}' 2>&1 | grep -c "^${IMG}$")"
[ "${out}" = 1 ] && record images_list true "docker images lists ${IMG}" || record images_list false "docker images: ${out}"
out="$("${DOCKER}" image inspect --format '{{.Os}}/{{.Architecture}}' "${IMG}" 2>&1)"; rc=$?
[ $rc -eq 0 ] && [ "${out}" = "linux/arm64" ] && record images_inspect_arch true "${out}" || record images_inspect_arch false "${out}"

# run: attached, exit code and separated streams.
errf="$(mktemp)"; out="$("${DOCKER}" run --rm "${IMG}" /bin/sh -c 'echo to-stdout; echo to-stderr >&2; exit 7' 2>"${errf}")"; rc=$?
if [ $rc -eq 7 ] && [ "${out}" = "to-stdout" ] && [ "$(cat "${errf}")" = "to-stderr" ]; then record run_exit_code_and_streams true "rc=7 stdout=to-stdout stderr=to-stderr"; else record run_exit_code_and_streams false "rc=${rc} out=${out} err=$(cat "${errf}")"; fi
rm -f "${errf}"

# run -d, ps, exec, stop (SIGTERM reaches the process), logs, kill, rm.
out="$("${DOCKER}" run -d --name vc1 "${IMG}" /bin/sh -c 'trap "echo got-term; exit 0" TERM; echo started; while :; do /bin/busybox sleep 1; done' 2>&1)"; rc=$?
[ $rc -eq 0 ] && record run_detached true "vc1 ${out:0:12}" || record run_detached false "${out}"
out="$("${DOCKER}" ps --filter name=vc1 --format '{{.Names}} {{.Status}}' 2>&1)"
case "${out}" in "vc1 Up"*) record ps_running true "${out}" ;; *) record ps_running false "${out}" ;; esac
out="$("${DOCKER}" exec vc1 /bin/sh -c 'echo exec-ok; exit 3' 2>&1)"; rc=$?
[ $rc -eq 3 ] && [ "${out}" = "exec-ok" ] && record exec_non_tty true "output exec-ok, rc=3" || record exec_non_tty false "rc=${rc} ${out}"
start=$(date +%s); out="$("${DOCKER}" stop -t 10 vc1 2>&1)"; rc=$?; took=$(( $(date +%s) - start ))
code="$("${DOCKER}" inspect --format '{{.State.ExitCode}}' vc1 2>&1)"; logs="$("${DOCKER}" logs vc1 2>&1 | tr '\n' ' ')"
if [ $rc -eq 0 ] && [ "${code}" = 0 ] && case "${logs}" in *got-term*) true;; *) false;; esac; then record stop_sigterm_propagates true "stop in ${took}s, exit 0, logs: ${logs}"; else record stop_sigterm_propagates false "rc=${rc} exit=${code} took=${took}s logs=${logs}"; fi
"${DOCKER}" run -d --name vc2 "${IMG}" /bin/sh -c 'while :; do /bin/busybox sleep 1; done' >/dev/null 2>&1
out="$("${DOCKER}" kill vc2 2>&1)"; rc=$?; code="$("${DOCKER}" inspect --format '{{.State.ExitCode}}' vc2 2>&1)"
[ $rc -eq 0 ] && [ "${code}" = 137 ] && record kill_sigkill true "exit ${code}" || record kill_sigkill false "rc=${rc} exit=${code} ${out}"
out="$("${DOCKER}" rm vc1 vc2 2>&1)"; rc=$?; left="$("${DOCKER}" ps -a --filter name=vc --format '{{.Names}}' | grep -cE '^vc[12]$')"
[ $rc -eq 0 ] && [ "${left}" = 0 ] && record rm_removes true "vc1 vc2 removed" || record rm_removes false "rc=${rc} left=${left} ${out}"

# pull: registry access through the guest's NAT with the CLI's anonymous X-Registry-Auth header.
start=$(date +%s); out="$("${DOCKER}" pull "${PULL_IMAGE}" 2>&1 | tail -1)"; rc=${PIPESTATUS[0]}; took=$(( $(date +%s) - start ))
if [ $rc -eq 0 ]; then record pull_from_registry true "${PULL_IMAGE} in ${took}s: ${out}"; "${DOCKER}" rmi "${PULL_IMAGE}" >/dev/null 2>&1; else record pull_from_registry false "rc=${rc} after ${took}s: ${out} (needs outbound network from the System VM)"; fi

# Auth and unsupported operations: dockerd's errors arrive intact and readable.
out="$("${DOCKER}" login nope.invalid -u x -p y 2>&1 | tail -1)"; rc=${PIPESTATUS[0]}
[ $rc -ne 0 ] && [ -n "${out}" ] && record login_error_passthrough true "rc=${rc}: ${out}" || record login_error_passthrough false "rc=${rc}: ${out}"
"${DOCKER}" run -d --name vc3 "${IMG}" /bin/sh -c 'while :; do /bin/busybox sleep 1; done' >/dev/null 2>&1
out="$("${DOCKER}" checkpoint create vc3 cp0 2>&1 | tail -1)"; rc=${PIPESTATUS[0]}
[ $rc -ne 0 ] && case "${out}" in *xperimental*|*not\ supported*|*unknown*) true;; *) false;; esac && record unsupported_checkpoint_error true "rc=${rc}: ${out}" || record unsupported_checkpoint_error false "rc=${rc}: ${out}"
"${DOCKER}" rm -f vc3 >/dev/null 2>&1
out="$("${DOCKER}" run --rm --runtime nosuch-runtime "${IMG}" /bin/true 2>&1 | grep -i -m1 runtime)"; rc=${PIPESTATUS[0]}
[ $rc -ne 0 ] && [ -n "${out}" ] && record unsupported_runtime_error true "rc=${rc}: ${out}" || record unsupported_runtime_error false "rc=${rc}: ${out}"
"${DOCKER}" rmi "${IMG}" >/dev/null 2>&1

# Attached streams survive silence and a closed stdin (GH #167): the relays used to drop the connection 0.5 s
# after the client half-closed its write side.
out="$("${DOCKER}" run --rm "${IMG}" /bin/sh -c 'echo a; sleep 3; echo b' 2>&1 | tr '\n' ' ')"
[ "${out}" = "a b " ] && record attach_output_after_pause true "attached non-TTY run printed '${out}' across a 3 s pause" || record attach_output_after_pause false "got '${out}' (expected 'a b ')"
out="$(echo x | "${DOCKER}" run --rm -i "${IMG}" /bin/sh -c 'cat; sleep 2; echo done' 2>&1 | tr '\n' ' ')"
[ "${out}" = "x done " ] && record attach_output_after_stdin_closed true "stdin closed early, output 2 s later still arrived: '${out}'" || record attach_output_after_stdin_closed false "got '${out}' (expected 'x done ')"
n="$("${DOCKER}" run --rm "${IMG}" /bin/sh -c 'i=0; while [ $i -lt 7 ]; do echo tick$i; sleep 1; i=$((i+1)); done' 2>&1 | grep -c '^tick')"
[ "${n}" = 7 ] && record attach_long_running_output true "7 lines over 7 s all arrived" || record attach_long_running_output false "${n} of 7 lines arrived"

# Ingress boundary: owner-only socket, no TCP listener, no other host sockets, raw guest ports refused.
mode="$(stat -f '%Lp' "${SOCKET}" 2>/dev/null || stat -c '%a' "${SOCKET}")"
[ "${mode}" = "600" ] && record ingress_socket_owner_only true "${SOCKET} mode ${mode}" || record ingress_socket_owner_only false "mode ${mode}"
if nc -z -w 1 127.0.0.1 2375 2>/dev/null || nc -z -w 1 127.0.0.1 2376 2>/dev/null; then record no_tcp_docker_listener false "a TCP listener answers on 2375/2376"; else record no_tcp_docker_listener true "no TCP Docker listener on localhost 2375/2376"; fi
others="$(find "${ROOT_DIR}/run" -type s ! -name docker.sock ! -name 3.sock ! -name vesperd.sock 2>/dev/null)"
[ -z "${others}" ] && record no_raw_host_sockets true "host sockets: $(find "${ROOT_DIR}/run" -type s | sed "s|${ROOT_DIR}/||" | tr '\n' ' ')" || record no_raw_host_sockets false "unexpected: ${others}"
out="$(cargo run -q --bin systemvmctl -- control --port 2376 handshake 2>&1)"; rc=$?
[ $rc -ne 0 ] && record raw_guest_port_denied true "guest port 2376 refused: ${out:0:120}" || record raw_guest_port_denied false "unexpected success: ${out}"
out="$(cargo run -q --bin systemvmctl -- control --port 10010 handshake 2>&1)"; rc=$?
[ $rc -ne 0 ] && record no_containerd_relay true "no vsock relay to containerd (port 10010 refused)" || record no_containerd_relay false "unexpected success: ${out}"

cleanup
passed=true; for s in "${steps[@]}"; do case "$s" in *'"ok":false'*) passed=false;; esac; done
printf '{"socket":"%s","steps":[%s],"passed":%s}\n' "${SOCKET}" "$(IFS=,; echo "${steps[*]}")" "${passed}" > "${REPORT}"
echo "docker conformance: passed=${passed} (report: ${REPORT})"
[ "${passed}" = true ]
