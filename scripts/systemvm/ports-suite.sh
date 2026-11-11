#!/usr/bin/env bash
# GH #28 checklist: `-p` ports reachable on the host loopback through vesper-proxy's publisher, multiple
# mappings per container, dockerd-level and host-level conflicts reported clearly, teardown on stop, and
# republishing after a proxy restart. Needs `make systemvm-up && make docker-up` and outbound network for
# the test image. Writes run/systemvm/ports-report.json.
set -uo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SOCKET="${DOCKER_SOCKET:-${ROOT_DIR}/run/vesper/docker.sock}"
STATUS="${ROOT_DIR}/run/vesper/ports.json"
REPORT="${ROOT_DIR}/run/systemvm/ports-report.json"
IMAGE="${PORTS_IMAGE:-alpine:3.22}"
DOCKER="${DOCKER_BIN:-docker}"
export DOCKER_HOST="unix://${SOCKET}"
cd "${ROOT_DIR}"; mkdir -p "$(dirname "${REPORT}")"
steps=()
record() { local d; d="$(printf '%s' "$3" | tr '\n' ' ' | cut -c1-300 | sed 's/\\/\\\\/g; s/"/\\"/g')"; steps+=("{\"name\":\"$1\",\"ok\":$2,\"detail\":\"${d}\"}"); if [ "$2" = true ]; then echo "ok   $1: ${d:0:140}"; else echo "FAIL $1: ${d:0:200}"; fi; }
SERVE='serve() { while true; do printf "HTTP/1.1 200 OK\r\nContent-Length: %s\r\nConnection: close\r\n\r\nport-%s\r\n" "$(( ${#1} + 7 ))" "$1" | nc -l -p "$1" >/dev/null; done; }; for p in "$@"; do serve "$p" & done; wait'
http() { curl -s --max-time 3 "http://127.0.0.1:$1/" 2>/dev/null | tr -d '\r'; }
wait_http() { for _ in $(seq 1 40); do out="$(http "$1")"; [ -n "${out}" ] && { echo "${out}"; return 0; }; sleep 0.25; done; return 1; }
wait_closed() { for _ in $(seq 1 40); do http "$1" >/dev/null || return 0; sleep 0.25; done; return 1; }
mappings() {
  python3 - "${STATUS}" 2>/dev/null <<'PY'
import json, sys
s = json.load(open(sys.argv[1]))
for m in s["mappings"]:
    print(m["name"], m["host"], m["state"], m.get("error", ""))
PY
}
cleanup() { "${DOCKER}" rm -f pp1 pp2 pp3 pp4 >/dev/null 2>&1; [ -n "${NCPID:-}" ] && kill "${NCPID}" 2>/dev/null; }
cleanup
[ -S "${SOCKET}" ] || { echo "ports-suite: ${SOCKET} missing; run 'make docker-up'" >&2; exit 2; }
"${DOCKER}" image inspect "${IMAGE}" >/dev/null 2>&1 || "${DOCKER}" pull -q "${IMAGE}" >/dev/null 2>&1 || { echo "ports-suite: cannot pull ${IMAGE}" >&2; exit 2; }

for _ in $(seq 1 40); do [ -f "${STATUS}" ] && break; sleep 0.25; done
[ -f "${STATUS}" ] && record publisher_active true "status file ${STATUS} present: $(python3 -c 'import json,sys; s=json.load(open(sys.argv[1])); print(s["path"], "gateway", s["gateway"])' "${STATUS}")" || record publisher_active false "no ${STATUS}; was vesper-proxy started with --publish-ports (make docker-up)?"

"${DOCKER}" run -d --name pp1 -p 18080:80 -p 18081:81 "${IMAGE}" sh -c "${SERVE}" sh 80 81 >/dev/null 2>&1
out="$(wait_http 18080)"; [ "${out}" = "port-80" ] && record localhost_port_reachable true "127.0.0.1:18080 -> ${out}" || record localhost_port_reachable false "127.0.0.1:18080 -> ${out:-no answer}"
out="$(wait_http 18081)"; [ "${out}" = "port-81" ] && record multiple_mappings true "127.0.0.1:18081 -> ${out} (second mapping of pp1)" || record multiple_mappings false "127.0.0.1:18081 -> ${out:-no answer}"
m="$(mappings | grep -c '^pp1 .* listening')"; [ "${m}" = 2 ] && record status_lists_mappings true "$(mappings | grep '^pp1' | tr '\n' ';')" || record status_lists_mappings false "$(mappings | tr '\n' ';')"

out="$("${DOCKER}" run -d --name pp2 -p 18080:80 "${IMAGE}" sh -c "${SERVE}" sh 80 2>&1 | grep -i -m1 -E 'allocated|in use|address')"; rc=${PIPESTATUS[0]}
case "${out}" in *"already allocated"*|*"address already in use"*) record docker_level_conflict_error true "rc=${rc}: ${out}" ;; *) record docker_level_conflict_error false "rc=${rc}: ${out}" ;; esac
"${DOCKER}" rm -f pp2 >/dev/null 2>&1

python3 -c 'import socket,time; s=socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1); s.bind(("127.0.0.1",18090)); s.listen(1); time.sleep(120)' & NCPID=$!
sleep 0.5
"${DOCKER}" run -d --name pp3 -p 18090:80 "${IMAGE}" sh -c "${SERVE}" sh 80 >/dev/null 2>&1
ok=false; for _ in $(seq 1 40); do mappings | grep -q '^pp3 .* conflict' && { ok=true; break; }; sleep 0.25; done
[ "${ok}" = true ] && record host_level_conflict_reported true "$(mappings | grep '^pp3')" || record host_level_conflict_reported false "$(mappings | tr '\n' ';')"
kill "${NCPID}" 2>/dev/null; NCPID=""; "${DOCKER}" rm -f pp3 >/dev/null 2>&1

"${DOCKER}" stop -t 1 pp1 >/dev/null 2>&1
wait_closed 18080 && wait_closed 18081 && record cleanup_on_stop true "18080 and 18081 closed after docker stop" || record cleanup_on_stop false "a port still answers after stop: $(http 18080) $(http 18081)"
for _ in $(seq 1 40); do mappings | grep -q '^pp1' || break; sleep 0.25; done
mappings | grep -q '^pp1' && record status_cleared_on_stop false "$(mappings | tr '\n' ';')" || record status_cleared_on_stop true "no pp1 mappings in ${STATUS}"
"${DOCKER}" rm -f pp1 >/dev/null 2>&1

"${DOCKER}" run -d --name pp4 -p 18091:80 "${IMAGE}" sh -c "${SERVE}" sh 80 >/dev/null 2>&1
out="$(wait_http 18091)"; [ "${out}" = "port-80" ] || record restart_precondition false "18091 not reachable before restart"
LPID="$(pgrep -f 'vesper-vmlauncher --config' | head -1)"
make -s docker-down >/dev/null 2>&1; make -s docker-up >/dev/null 2>&1
sleep 1
if [ -n "${LPID}" ] && kill -0 "${LPID}" 2>/dev/null; then record launcher_survives_proxy_restart true "vesper-vmlauncher pid ${LPID} still running after the proxy dropped a live event stream"; else record launcher_survives_proxy_restart false "launcher pid ${LPID:-unknown} gone after the proxy restart (SIGPIPE regression)"; fi
out="$(wait_http 18091)"; [ "${out}" = "port-80" ] && record republished_after_proxy_restart true "127.0.0.1:18091 -> ${out} after docker-down/docker-up (reconciliation)" || record republished_after_proxy_restart false "127.0.0.1:18091 -> ${out:-no answer} after proxy restart"
"${DOCKER}" rm -f pp4 >/dev/null 2>&1
wait_closed 18091 && record cleanup_on_rm true "18091 closed after docker rm -f" || record cleanup_on_rm false "18091 still answers"

# A client that half-closes after its request still gets a response that comes later (GH #167).
"${DOCKER}" rm -f pp5 >/dev/null 2>&1
"${DOCKER}" run -d --name pp5 -p 18095:80 "${IMAGE}" sh -c 'while true; do nc -l -p 80 -e sh -c "read -r _; sleep 2; echo late-reply"; done' >/dev/null 2>&1
sleep 2
out="$(printf 'hello\n' | python3 -c 'import socket,sys; s=socket.create_connection(("127.0.0.1",18095),5); s.sendall(sys.stdin.buffer.read()); s.shutdown(socket.SHUT_WR); s.settimeout(10); print(s.recv(100).decode().strip())' 2>&1)"
[ "${out}" = late-reply ] && record half_closed_client_gets_late_reply true "reply 2 s after the client shut its write side: ${out}" || record half_closed_client_gets_late_reply false "got '${out}'"
"${DOCKER}" rm -f pp5 >/dev/null 2>&1

cleanup
passed=true; for s in "${steps[@]}"; do case "$s" in *'"ok":false'*) passed=false;; esac; done
printf '{"socket":"%s","status_file":"%s","steps":[%s],"passed":%s}\n' "${SOCKET}" "${STATUS}" "$(IFS=,; echo "${steps[*]}")" "${passed}" > "${REPORT}"
echo "ports suite: passed=${passed} (report: ${REPORT})"
[ "${passed}" = true ]
