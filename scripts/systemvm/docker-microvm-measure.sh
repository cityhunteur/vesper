#!/usr/bin/env bash
# ADR 0011 slice 4c: measurements of `vesper` containers beside runc ones, from macOS through vesper-proxy.
#   run_latency     p50 and p95 of `docker run --rm IMAGE true`, N runs per runtime
#   stop_latency    p50 and p95 of `docker stop` of a container that exits on SIGTERM, N per runtime
#   block_io        MiB/s of a dd write with fsync and a read back through the root filesystem, runc and vesper (TD-033)
#   concurrent_start, memory_per_microvm   K vesper containers started at once; the System VM's available memory
#                   and Firecracker's resident memory per idle microVM (TD-033)
#   density         how many `vesper` containers running `sleep` start on this System VM before one is refused, and
#                   what the refusal says (memory admission, vesper-memadmit)
#   containerd_restart_keeps_task   containerd is SIGKILLed and respawned while a `vesper` container runs; the
#                   container still runs, `docker exec` works, and `docker stop` stops it
#   exit_while_containerd_down      a container exits inside containerd's restart window, with runc and with
#                   vesper: whether dockerd reports the exit within 30 s of containerd's return, and that `docker
#                   stop` leaves no microVM. dockerd misses such an exit with either runtime, and what it reports after
#                   stop varies (TD-098); the step passes when vesper answers as runc does in the same run
# Needs `make systemvm-up && make docker-up`. MEASURE_RUNS (default 10), MEASURE_MAX_DENSITY (default 40). Writes
# run/systemvm/docker-microvm-measure-report.json. Restarts containerd twice.
set -uo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SOCKET="${DOCKER_SOCKET:-${ROOT_DIR}/run/vesper/docker.sock}"
REPORT="${ROOT_DIR}/run/systemvm/docker-microvm-measure-report.json"
IMAGE="${MEASURE_IMAGE:-alpine:3.22}"
RUNS="${MEASURE_RUNS:-10}"
MAXD="${MEASURE_MAX_DENSITY:-40}"
export DOCKER_HOST="unix://${SOCKET}"
cd "${ROOT_DIR}"; mkdir -p "$(dirname "${REPORT}")"
cargo build -q --bin systemvmctl || exit 2
CTL="${ROOT_DIR}/target/debug/systemvmctl"
steps=()
record() { local d; d="$(printf '%s' "$3" | tr '\n' ' ' | cut -c1-600 | sed 's/\\/\\\\/g; s/"/\\"/g')"; steps+=("{\"name\":\"$1\",\"ok\":$2,\"detail\":\"${d}\"}"); if [ "$2" = true ]; then echo "ok   $1: ${d:0:220}"; else echo "FAIL $1: ${d:0:300}"; fi; }
now_ms() { python3 -I -c 'import time; print(int(time.time() * 1000))'; }
pct() { # P values... -> the P-th percentile (nearest rank)
  python3 -I - "$@" <<'PY'
import math, sys
p, xs = float(sys.argv[1]), sorted(int(x) for x in sys.argv[2:])
print(xs[max(0, math.ceil(p / 100 * len(xs)) - 1)] if xs else "none")
PY
}
healthy() { "${CTL}" control health 2>/dev/null | grep -q '"containerd_ok":true'; }
cleanup() { docker ps -aq --filter label=vesper.measure=1 | xargs docker rm -f >/dev/null 2>&1; }
trap cleanup EXIT
[ -S "${SOCKET}" ] || { echo "docker-microvm-measure: ${SOCKET} missing; run 'make docker-up'" >&2; exit 2; }
docker image inspect "${IMAGE}" >/dev/null 2>&1 || docker pull -q "${IMAGE}" >/dev/null 2>&1 || { echo "cannot pull ${IMAGE}" >&2; exit 2; }
cleanup

# --- run and stop latency, both runtimes ---------------------------------------------------------------------------
TERMSAFE='trap "exit 0" TERM; while :; do sleep 0.1; done'
for rt in runc vesper; do
  runs=(); stops=(); errs=""
  for _ in $(seq 1 "${RUNS}"); do
    t0=$(now_ms); out="$(docker run --rm --label vesper.measure=1 --runtime "${rt}" "${IMAGE}" true 2>&1)" || errs="${errs} run: ${out:0:80}"
    runs+=($(( $(now_ms) - t0 )))
    cid="$(docker run -d --label vesper.measure=1 --runtime "${rt}" "${IMAGE}" sh -c "${TERMSAFE}" 2>&1)" || { errs="${errs} start: ${cid:0:80}"; continue; }
    t0=$(now_ms); docker stop -t 10 "${cid}" >/dev/null 2>&1 || errs="${errs} stop"; stops+=($(( $(now_ms) - t0 )))
    docker rm -f "${cid}" >/dev/null 2>&1
  done
  detail="${rt}: run p50 $(pct 50 "${runs[@]}") ms p95 $(pct 95 "${runs[@]}") ms; stop p50 $(pct 50 "${stops[@]}") ms p95 $(pct 95 "${stops[@]}") ms; n=${RUNS}"
  [ -z "${errs}" ] && record "latency_${rt}" true "${detail}" || record "latency_${rt}" false "${detail};${errs}"
done

# --- density ----------------------------------------------------------------------------------------------------
n=0; why=""
for i in $(seq 1 "${MAXD}"); do
  out="$(docker run -d --label vesper.measure=1 --runtime vesper --name "density-${i}" "${IMAGE}" sleep 600 2>&1)" || { why="${out}"; break; }
  n=${i}
done
mem="$("${CTL}" control microvm -- limits 2>/dev/null | head -1); System VM memory_mib $(python3 -I -c 'import json,sys; print(json.load(open(sys.argv[1]))["memory_mib"])' build/systemvm/launch-config.json 2>/dev/null)"
docker ps -aq --filter label=vesper.measure=1 | xargs docker rm -f >/dev/null 2>&1
if [ -n "${why}" ]; then
  case "${why}" in *memory*|*"does not fit"*) record density true "${n} vesper containers running sleep started; the next was refused: ${why:0:300}; ${mem}" ;;
    *) record density false "${n} started; the next failed without a memory reason: ${why:0:300}; ${mem}" ;; esac
else
  record density true "all ${MAXD} vesper containers started (MEASURE_MAX_DENSITY); ${mem}"
fi

# --- TD-033: block I/O, concurrent start, memory per microVM ---------------------------------------------------
IOSZ="${MEASURE_IO_MIB:-256}"
IOCMD="ms() { awk '{print int(\$1 * 1000)}' /proc/uptime; }; t0=\$(ms); dd if=/dev/zero of=/io.bin bs=1M count=${IOSZ} conv=fsync 2>/dev/null; t1=\$(ms); dd if=/io.bin of=/dev/null bs=1M 2>/dev/null; t2=\$(ms); echo \$(( ${IOSZ} * 1000 / (t1 - t0 + 1) )) \$(( ${IOSZ} * 1000 / (t2 - t1 + 1) ))"
io=""; for rt in runc vesper; do
  r="$(docker run --rm --label vesper.measure=1 --runtime "${rt}" "${IMAGE}" sh -c "${IOCMD}" 2>&1 | tail -1)"
  io="${io}${rt}: write ${r% *} MiB/s (fsync), read ${r#* } MiB/s (page cache); "
done
case "${io}" in *"write "[0-9]*"read "[0-9]*"vesper: write "[0-9]*) record block_io true "${io}${IOSZ} MiB through the container's root filesystem" ;; *) record block_io false "${io}" ;; esac
# Concurrent start: K vesper containers at once (K defaults to 4, the default System VM's density).
K="${MEASURE_CONCURRENT:-4}"; t0=$(now_ms); pids=()
for i in $(seq 1 "${K}"); do docker run -d --label vesper.measure=1 --runtime vesper --name "conc-${i}" "${IMAGE}" sleep 600 >/dev/null 2>&1 & pids+=($!); done
okn=0; for p in "${pids[@]}"; do wait "${p}" && okn=$((okn + 1)); done; cwall=$(( $(now_ms) - t0 ))
mem_all="$("${CTL}" control fcparity mem 2>/dev/null | grep '^MEM' | head -1)"
docker ps -aq --filter label=vesper.measure=1 | xargs docker rm -f >/dev/null 2>&1; sleep 3
mem_none="$("${CTL}" control fcparity mem 2>/dev/null | grep '^MEM' | head -1)"
kv() { printf '%s\n' "$1" | tr ' ' '\n' | sed -n "s/^$2=//p"; }
a0=$(kv "${mem_none}" avail_kib); a1=$(kv "${mem_all}" avail_kib); fcn=$(kv "${mem_all}" firecracker); rss=$(kv "${mem_all}" firecracker_rss_kib)
if [ "${okn}" = "${K}" ] && [ -n "${a0}" ] && [ -n "${a1}" ] && [ "${fcn:-0}" -gt 0 ]; then
  record concurrent_start true "${K} vesper containers started at once in ${cwall} ms ($(( okn * 60000 / cwall )) a minute)"
  record memory_per_microvm true "System VM MemAvailable fell by $(( (a0 - a1) / K / 1024 )) MiB per idle vesper container; Firecracker resident $(( rss / fcn / 1024 )) MiB each (${fcn} running); ${mem_none} / ${mem_all}"
else
  record concurrent_start false "${okn} of ${K} started in ${cwall} ms"
  record memory_per_microvm false "${mem_none} / ${mem_all}"
fi

# --- containerd restart with a running vesper container ----------------------------------------------------------
cid="$(docker run -d --label vesper.measure=1 --runtime vesper --name restart-1 "${IMAGE}" sleep 600 2>&1)"
kill="$("${CTL}" control kill containerd 2>&1 | tail -1)"
up=no; for _ in $(seq 1 600); do healthy && { up=yes; break; }; sleep 0.1; done
# containerd answers healthy before dockerd has reconnected to its socket; exec is tried until it works (30 s).
st="$(docker inspect -f '{{.State.Status}}' restart-1 2>&1)"; t0=$(now_ms)
for _ in $(seq 1 60); do ex="$(docker exec restart-1 echo exec-ok 2>&1)"; [ "${ex}" = exec-ok ] && break; sleep 0.5; done
ex="${ex} after $(( $(now_ms) - t0 )) ms"
t0=$(now_ms); docker stop -t 2 restart-1 >/dev/null 2>&1; stop_ms=$(( $(now_ms) - t0 )); after="$(docker inspect -f '{{.State.Status}}' restart-1 2>&1)"
case "${up}|${st}|${ex}|${after}" in "yes|running|exec-ok after "*"|exited")
    record containerd_restart_keeps_task true "${kill}; containerd healthy again; container ${st}; exec: ${ex}; stopped in ${stop_ms} ms (${after})" ;;
  *) record containerd_restart_keeps_task false "${kill}; healthy again: ${up}; container ${st}; exec: ${ex}; after stop: ${after}" ;; esac
docker rm -f restart-1 >/dev/null 2>&1

# --- an exit while containerd is down, both runtimes -----------------------------------------------------------
LIFE="${MEASURE_LIFE_MS:-4000}"; LEAD="${MEASURE_KILL_LEAD_MS:-500}"
declare -A seen
detail=""
for rt in runc vesper; do
  # The kill must find the container running, with containerd fully back from the step before; else try again.
  for attempt in 1 2 3; do
    for _ in $(seq 1 60); do docker run --rm --runtime "${rt}" "${IMAGE}" true >/dev/null 2>&1 && break; sleep 1; done
    docker rm -f exit-1 >/dev/null 2>&1
    docker run -d --label vesper.measure=1 --runtime "${rt}" --name exit-1 "${IMAGE}" sh -c "sleep $(python3 -I -c "print(${LIFE}/1000)"); exit 7" >/dev/null 2>&1
    cid="$(docker inspect -f '{{.Id}}' exit-1 2>/dev/null)"
    sleep "$(python3 -I -c "print(max(0, (${LIFE} - ${LEAD}) / 1000))")"
    before="$(docker inspect -f '{{.State.Status}}' exit-1 2>/dev/null)"
    [ "${before}" = running ] && break
    detail="${detail}(${rt} attempt ${attempt}: the container was ${before:-gone} before the kill) "
  done
  "${CTL}" control kill containerd >/dev/null 2>&1
  for _ in $(seq 1 600); do healthy && break; sleep 0.1; done
  reported=no; for _ in $(seq 1 60); do case "$(docker inspect -f '{{.State.Status}}' exit-1 2>/dev/null)" in exited) reported=yes; break ;; esac; sleep 0.5; done
  vm="$("${CTL}" control microvm status "${cid}" 2>/dev/null | head -1 | cut -d' ' -f1)"
  docker stop -t 2 exit-1 >/dev/null 2>&1
  code="$(docker inspect -f '{{.State.ExitCode}}' exit-1 2>&1)"; vm_after="$("${CTL}" control microvm status "${cid}" 2>/dev/null | head -1 | cut -d' ' -f1)"
  seen[${rt}]="${reported} code ${code}"
  detail="${detail}${rt}: exit reported within 30 s: ${reported}; microVM before stop: ${vm:-none}; after docker stop: code ${code}, microVM ${vm_after:-none}. "
  [ "${rt}" = vesper ] && [ "${vm_after}" != ABSENT ] && detail="${detail}(the microVM outlived docker stop) "
  docker rm -f exit-1 >/dev/null 2>&1
done
# dockerd's answer after the missed exit varies from run to run (code 7 or 0, TD-098); the step asks that vesper
# answers as runc does in the same run and that no microVM outlives docker stop.
case "${detail}" in *"outlived"*) ok=false ;; *) ok=true ;; esac
[ "${seen[runc]}" = "${seen[vesper]}" ] || ok=false
record exit_while_containerd_down "${ok}" "${detail}(TD-098: dockerd misses an exit while containerd restarts; with vesper the microVM keeps its memory until docker stop)"

passed=true; for s in "${steps[@]}"; do case "$s" in *'"ok":false'*) passed=false ;; esac; done
printf '{"image":"%s","runs":%s,"steps":[%s],"passed":%s}\n' "${IMAGE}" "${RUNS}" "$(IFS=,; echo "${steps[*]}")" "${passed}" > "${REPORT}"
echo "docker-microvm measure: passed=${passed} (report: ${REPORT})"
[ "${passed}" = true ]
