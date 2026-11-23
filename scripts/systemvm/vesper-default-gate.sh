#!/usr/bin/env bash
# The owner's gate for `vesper` as dockerd's default runtime (D-099), measured with `vesper` as the real default:
# the System VM is restarted with `vesper.default-runtime=vesper` on its kernel command line, and every container
# below names no runtime. The launch config is restored and the System VM restarted on every exit.
#   default_is_vesper   `docker info` reports vesper, and a create that names no runtime is checked as vesper's
#                       (the plugin refuses --read-only by ADR 0011)
#   g1_start            p50 and p95 of `docker run --rm IMAGE true`, GATE_RUNS runs after one warm-up (gate: p50
#                       under 1500 ms)
#   g2_density          idle containers (`sleep`) started before one is refused, up to GATE_MAX_DENSITY (gate: 10)
#   g3_foreign_arch     an amd64 image is refused at create, naming --runtime vesper-runc, and runs with it
# G4 (the deviations reviewed) is the owner's and is not measured. A step is ok when its measurement was taken; the
# report's "gate" says, for G1 to G3, whether the target is met. Needs the launch config of `make systemvm-up` (a data
# disk) and outbound network for the images. Writes run/systemvm/vesper-default-gate-report.json.
set -uo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"
SOCKET="${DOCKER_SOCKET:-${ROOT_DIR}/run/vesper/docker.sock}"
REPORT="${ROOT_DIR}/run/systemvm/vesper-default-gate-report.json"
CFG="${ROOT_DIR}/build/systemvm/launch-config.json"
SAVED="${ROOT_DIR}/build/systemvm/launch-config.gate-saved.json"
IMAGE="${GATE_IMAGE:-alpine:3.22}"
A64="${GATE_AMD64_IMAGE:-busybox:1.37}"
RUNS="${GATE_RUNS:-10}"
MAXD="${GATE_MAX_DENSITY:-20}"
export DOCKER_HOST="unix://${SOCKET}"
mkdir -p "$(dirname "${REPORT}")"
cargo build -q --bin systemvmctl || exit 2
[ -f "${CFG}" ] || { echo "vesper-default-gate: ${CFG} missing; run 'make systemvm-up' first" >&2; exit 2; }
# A run that was interrupted left its saved config: put it back before anything else.
if [ -f "${SAVED}" ]; then cp "${SAVED}" "${CFG}" && rm -f "${SAVED}"; fi

restart() { bash scripts/systemvm/docker-down.sh >/dev/null 2>&1; bash scripts/systemvm/down.sh >/dev/null 2>&1
  bash scripts/systemvm/up.sh >/dev/null 2>&1 && bash scripts/systemvm/docker-up.sh >/dev/null 2>&1; }
restore() {
  trap - EXIT INT TERM
  docker ps -aq --filter label=vesper.gate=1 2>/dev/null | xargs docker rm -f >/dev/null 2>&1
  [ -f "${SAVED}" ] || return 0
  cp "${SAVED}" "${CFG}" && rm -f "${SAVED}"
  restart || echo "vesper-default-gate: the restart with the restored launch config failed; run 'make systemvm-up docker-up'" >&2
}
cp "${CFG}" "${SAVED}"
trap restore EXIT
trap 'exit 130' INT TERM
python3 -I -c 'import json, sys
c = json.load(open(sys.argv[1]))
words = [w for w in c.get("kernel_cmdline", "").split() if not w.startswith("vesper.default-runtime=")]
c["kernel_cmdline"] = " ".join(words + ["vesper.default-runtime=vesper"])
json.dump(c, open(sys.argv[1], "w"), indent=2)' "${CFG}"
restart || { echo "vesper-default-gate: the System VM did not start with vesper as the default" >&2; exit 2; }

steps=()
record() { local d; d="$(printf '%s' "$3" | tr '\n' ' ' | cut -c1-600 | sed 's/\\/\\\\/g; s/"/\\"/g')"; steps+=("{\"name\":\"$1\",\"ok\":$2,\"detail\":\"${d}\"}"); if [ "$2" = true ]; then echo "ok   $1: ${d:0:240}"; else echo "FAIL $1: ${d:0:300}"; fi; }
now_ms() { python3 -I -c 'import time; print(int(time.time() * 1000))'; }
pct() { python3 -I - "$@" <<'PY'
import math, sys
p, xs = float(sys.argv[1]), sorted(int(x) for x in sys.argv[2:])
print(xs[max(0, math.ceil(p / 100 * len(xs)) - 1)] if xs else "none")
PY
}
docker image inspect "${IMAGE}" >/dev/null 2>&1 || docker pull -q "${IMAGE}" >/dev/null 2>&1

# --- default_is_vesper ----------------------------------------------------------------------------------------------
dr="$(docker info --format '{{.DefaultRuntime}}' 2>&1)"
ro="$(docker create --label vesper.gate=1 --read-only "${IMAGE}" true 2>&1)"
case "${dr}|${ro}" in "vesper|"*"(ADR 0011)"*) record default_is_vesper true "DefaultRuntime ${dr}; --read-only without --runtime: ${ro}" ;;
  *) record default_is_vesper false "DefaultRuntime ${dr}; --read-only without --runtime: ${ro}"; exit 1 ;; esac

# --- g1_start -------------------------------------------------------------------------------------------------------
docker run --rm "${IMAGE}" true >/dev/null 2>&1
times=(); bad=""
for _ in $(seq 1 "${RUNS}"); do
  t0="$(now_ms)"; out="$(docker run --rm "${IMAGE}" true 2>&1)" || bad="${bad} ${out:0:120}"; times+=("$(( $(now_ms) - t0 ))")
done
p50="$(pct 50 "${times[@]}")"; p95="$(pct 95 "${times[@]}")"
[ -z "${bad}" ] && record g1_start true "p50 ${p50} ms, p95 ${p95} ms over ${RUNS} runs: ${times[*]}" || record g1_start false "failed runs:${bad}"
g1=false; [ -z "${bad}" ] && [ "${p50}" -lt 1500 ] && g1=true

# --- g2_density -----------------------------------------------------------------------------------------------------
n=0; why=""
for i in $(seq 1 "${MAXD}"); do
  out="$(docker run -d --label vesper.gate=1 --name "gate-${i}" "${IMAGE}" sleep 600 2>&1)" || { why="${out}"; break; }
  n="${i}"
done
mem="$("${ROOT_DIR}/target/debug/systemvmctl" control microvm -- limits 2>/dev/null | head -1)"
docker ps -aq --filter label=vesper.gate=1 | xargs docker rm -f >/dev/null 2>&1
case "${why}" in
  "") record g2_density true "all ${MAXD} started (GATE_MAX_DENSITY); ${mem}" ;;
  *memory*|*"does not fit"*) record g2_density true "${n} idle containers started; the next was refused: ${why:0:300}; ${mem}" ;;
  *) record g2_density false "${n} started; the next failed without a memory reason: ${why:0:300}" ;;
esac
g2=false; [ "${n}" -ge 10 ] && g2=true

# --- g3_foreign_arch ------------------------------------------------------------------------------------------------
docker pull -q --platform linux/amd64 "${A64}" >/dev/null 2>&1
ia="$(docker image inspect --format '{{.Architecture}}' "${A64}" 2>&1)"
na="$(docker run --rm "${A64}" true 2>&1 | grep -v '^WARNING' | tr '\n' ' ')"
fb="$(docker run --rm --runtime vesper-runc "${A64}" uname -m 2>&1 | tail -1)"
detail="image ${ia}; no --runtime: ${na:0:300}; --runtime vesper-runc: ${fb}"
case "${ia}|${na}|${fb}" in "amd64|"*"--runtime vesper-runc"*"|x86_64") record g3_foreign_arch true "${detail}"; g3=true ;;
  *) record g3_foreign_arch false "${detail}"; g3=false ;; esac

passed=true; for s in "${steps[@]}"; do case "$s" in *'"ok":false'*) passed=false ;; esac; done
printf '{"steps":[%s],"passed":%s,"gate":{"g1_start_p50_ms":%s,"g1_met":%s,"g2_density":%s,"g2_met":%s,"g3_met":%s,"g4":"owner"}}\n' \
  "$(IFS=,; echo "${steps[*]}")" "${passed}" "${p50:-null}" "${g1}" "${n}" "${g2}" "${g3}" > "${REPORT}"
echo "vesper-default-gate: measured=${passed}; G1 p50 ${p50} ms met=${g1}; G2 ${n} containers met=${g2}; G3 met=${g3}; G4 is the owner's (report: ${REPORT})"
[ "${passed}" = true ]
