#!/usr/bin/env bash
# GH #31 developer workflow suite from macOS through the ingress: compose up of a multi-service stack,
# docker build (BuildKit) from a host context, `docker logs -f` streaming, an interactive `docker exec -it`
# driven through a pty by expect, stop/kill signal propagation, and the `--platform linux/amd64` probe
# (a known gap until GH #39). The report carries the baseline lock revisions. Needs `make systemvm-up &&
# make docker-up`, docker compose, expect and outbound network. Writes run/systemvm/workflow-report.json.
set -uo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SOCKET="${DOCKER_SOCKET:-${ROOT_DIR}/run/vesper/docker.sock}"
REPORT="${ROOT_DIR}/run/systemvm/workflow-report.json"
STACK="${ROOT_DIR}/scripts/systemvm/fixtures/dns-stack.yml"
BUILD_CTX="${ROOT_DIR}/scripts/systemvm/fixtures/wf-build"
IMAGE="${WF_IMAGE:-alpine:3.22}"
DOCKER="${DOCKER_BIN:-docker}"
export DOCKER_HOST="unix://${SOCKET}" DNS_IMAGE="${IMAGE}"
cd "${ROOT_DIR}"; mkdir -p "$(dirname "${REPORT}")"
steps=()
record() { local d; d="$(printf '%s' "$3" | tr '\n' ' ' | cut -c1-300 | sed 's/\\/\\\\/g; s/"/\\"/g')"; steps+=("{\"name\":\"$1\",\"ok\":$2,\"detail\":\"${d}\"}"); if [ "$2" = true ]; then echo "ok   $1: ${d:0:150}"; else echo "FAIL $1: ${d:0:200}"; fi; }
compose() { "${DOCKER}" compose -f "${STACK}" -p vesper-wf "$@"; }
cleanup() { compose down --remove-orphans -t 1 >/dev/null 2>&1; "${DOCKER}" rm -f wf-logs wf-exec wf-sig >/dev/null 2>&1; "${DOCKER}" rmi -f vesper/wf:1 >/dev/null 2>&1; }
cleanup
[ -S "${SOCKET}" ] || { echo "workflow-suite: ${SOCKET} missing; run 'make docker-up'" >&2; exit 2; }
command -v expect >/dev/null || { echo "workflow-suite: expect is required for the interactive step" >&2; exit 2; }
# The test image must be the native arm64 variant: a `--platform linux/amd64` pull re-tags the same reference.
arch="$("${DOCKER}" image inspect --format '{{.Architecture}}' "${IMAGE}" 2>/dev/null)"
if [ "${arch}" != "arm64" ]; then "${DOCKER}" pull -q --platform linux/arm64 "${IMAGE}" >/dev/null 2>&1 || { echo "workflow-suite: cannot pull ${IMAGE} for linux/arm64" >&2; exit 2; }; fi

# 1. compose up: both services running, the client reaches the web service.
compose up -d >/dev/null 2>&1; sleep 1
running="$(compose ps --status running --format '{{.Service}}' 2>/dev/null | sort | tr '\n' ' ')"
out="$(compose exec -T client sh -c 'wget -qO- -T 5 http://web/ | tr -d "\r"' 2>&1)"
[ "${running}" = "client web " ] && [ "${out}" = "web-ok" ] && record compose_up_multi_service true "services running: ${running}; client -> http://web/ -> ${out}" || record compose_up_multi_service false "running: ${running}; web: ${out}"
compose down -t 1 >/dev/null 2>&1

# 2. docker build with a host context through the proxy, then run the result.
start=$(date +%s); out="$("${DOCKER}" build -q --build-arg BASE="${IMAGE}" -t vesper/wf:1 "${BUILD_CTX}" 2>&1 | tail -1)"; rc=${PIPESTATUS[0]}; took=$(( $(date +%s) - start ))
run_out="$("${DOCKER}" run --rm vesper/wf:1 2>&1 | tr '\n' ' ')"
[ $rc -eq 0 ] && [ "${run_out}" = "built-in-system-vm payload-from-host " ] && record docker_build_from_host_context true "built ${out} in ${took}s; run -> ${run_out}" || record docker_build_from_host_context false "rc=${rc} ${out}; run -> ${run_out}"
layers="$("${DOCKER}" image inspect --format '{{len .RootFS.Layers}}' vesper/wf:1 2>/dev/null)"
[ "${layers:-0}" -ge 3 ] && record build_layers_present true "${layers} layers (base + RUN + COPY)" || record build_layers_present false "layers=${layers}"

# 3. logs -f streams incrementally: a line per second, three lines must arrive over at least two seconds.
"${DOCKER}" run -d --name wf-logs "${IMAGE}" sh -c 'i=0; while true; do i=$((i+1)); echo "tick-$i"; sleep 1; done' >/dev/null 2>&1
lf="$(mktemp)"; ( "${DOCKER}" logs -f wf-logs 2>/dev/null | while IFS= read -r line; do echo "$(date +%s) ${line}"; done > "${lf}" ) & LP=$!
sleep 4; kill "${LP}" 2>/dev/null; pkill -P "${LP}" 2>/dev/null
n="$(wc -l < "${lf}" | tr -d ' ')"; first="$(head -1 "${lf}" | awk '{print $1}')"; last="$(tail -1 "${lf}" | awk '{print $1}')"
if [ "${n:-0}" -ge 3 ] && [ $(( ${last:-0} - ${first:-0} )) -ge 2 ]; then record logs_follow_streams true "${n} lines over $(( last - first ))s: $(awk '{print $2}' "${lf}" | tr '\n' ' ')"; else record logs_follow_streams false "${n} lines, spread $(( ${last:-0} - ${first:-0} ))s"; fi
rm -f "${lf}"; "${DOCKER}" rm -f wf-logs >/dev/null 2>&1

# 4. exec -it: an interactive shell through a pty (expect drives it), with a tty and a working prompt loop.
"${DOCKER}" run -d --name wf-exec "${IMAGE}" sleep 300 >/dev/null 2>&1
out="$(expect -c "
set timeout 15
spawn ${DOCKER} exec -it wf-exec sh
expect \"# \"
send \"tty; echo interactive-\\\$((20+3))\r\"
expect \"interactive-23\"
send \"exit\r\"
expect eof
" 2>&1 | tr -d '\r')"
case "${out}" in *"/dev/pts/"*"interactive-23"*) record exec_it_interactive_tty true "pty inside: $(echo "${out}" | grep -o '/dev/pts/[0-9]*' | head -1); prompt loop answered interactive-23" ;; *) record exec_it_interactive_tty false "$(echo "${out}" | tail -3 | tr '\n' ' ')" ;; esac
"${DOCKER}" rm -f wf-exec >/dev/null 2>&1

# 5. signals: stop delivers SIGTERM (trap exits 0), kill -s HUP reaches the process, SIGKILL gives 137.
"${DOCKER}" run -d --name wf-sig "${IMAGE}" sh -c 'trap "echo got-hup" HUP; trap "echo got-term; exit 0" TERM; echo ready; while true; do sleep 1; done' >/dev/null 2>&1; sleep 1
"${DOCKER}" kill -s HUP wf-sig >/dev/null 2>&1; sleep 2
start=$(date +%s); "${DOCKER}" stop -t 10 wf-sig >/dev/null 2>&1; took=$(( $(date +%s) - start ))
code="$("${DOCKER}" inspect --format '{{.State.ExitCode}}' wf-sig 2>/dev/null)"; logs="$("${DOCKER}" logs wf-sig 2>&1 | tr '\n' ' ')"
case "${logs}" in *got-hup*got-term*) [ "${code}" = 0 ] && record stop_signals_propagate true "HUP then TERM reached the process; stop in ${took}s, exit ${code}; logs: ${logs}" || record stop_signals_propagate false "exit ${code}: ${logs}" ;; *) record stop_signals_propagate false "exit ${code}: ${logs}" ;; esac
"${DOCKER}" rm -f wf-sig >/dev/null 2>&1
"${DOCKER}" run -d --name wf-sig "${IMAGE}" sleep 300 >/dev/null 2>&1; "${DOCKER}" kill wf-sig >/dev/null 2>&1; code="$("${DOCKER}" inspect --format '{{.State.ExitCode}}' wf-sig 2>/dev/null)"
[ "${code}" = 137 ] && record sigkill_exit_code true "docker kill -> exit ${code}" || record sigkill_exit_code false "exit ${code}"
"${DOCKER}" rm -f wf-sig >/dev/null 2>&1

# 6. --platform linux/amd64: needs binfmt_misc + qemu-user in the System VM (GH #39); recorded, not required.
# A separate image reference keeps the amd64 pull from re-tagging the native test image.
out="$("${DOCKER}" run --rm --platform linux/amd64 "${WF_AMD64_IMAGE:-busybox:1.37}" uname -m 2>&1 | tail -1)"; "${DOCKER}" rmi -f "${WF_AMD64_IMAGE:-busybox:1.37}" >/dev/null 2>&1
if [ "${out}" = "x86_64" ]; then record platform_amd64 true "uname -m -> ${out}"; else
  if [ "${WF_REQUIRE_AMD64:-0}" = 1 ]; then record platform_amd64 false "${out}"; else record platform_amd64_known_gap true "not available yet (GH #39, cross-architecture mode): ${out}"; fi; fi

cleanup
cargo run -q --bin baselinectl -- --mode generate --backend backend-a --host-os macos --host-cpu "$(sysctl -n machdep.cpu.brand_string 2>/dev/null | tr ' ' '-' | tr '[:upper:]' '[:lower:]')" --metadata "${ROOT_DIR}/run/systemvm/workflow-metadata.json" >/dev/null 2>&1
meta="$(python3 -c 'import json,sys; print(json.dumps(json.load(open(sys.argv[1]))["metadata"]))' "${ROOT_DIR}/run/systemvm/workflow-metadata.json" 2>/dev/null || echo '[]')"
passed=true; for s in "${steps[@]}"; do case "$s" in *'"ok":false'*) passed=false;; esac; done
printf '{"socket":"%s","image":"%s","baseline_metadata":%s,"steps":[%s],"passed":%s}\n' "${SOCKET}" "${IMAGE}" "${meta}" "$(IFS=,; echo "${steps[*]}")" "${passed}" > "${REPORT}"
echo "workflow suite: passed=${passed} (report: ${REPORT})"
[ "${passed}" = true ]
