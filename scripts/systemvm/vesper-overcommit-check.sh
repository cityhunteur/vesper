#!/usr/bin/env bash
# Bounded overcommit (D-107): what happens when `vesper` guests without a memory limit together outgrow the System VM.
#   fill_to_admission    idle `vesper` containers (no -m) start until admission refuses one (a refusal must be seen
#                        within OVERCOMMIT_MAX); at least OVERCOMMIT_MIN must fit and all must run at once (the G2
#                        gate is 10 on the default System VM)
#   daemons_protected    dockerd and containerd run with oom_score_adj -900, Firecracker with 0
#   pressure_kills_only_guests
#                        every guest fills OVERCOMMIT_FILL_MB (60) of its /dev/shm at once; the parent cgroup's OOM count rises;
#                        the kernel kills guests only (every exit is 137, each with its own VESPER-MICROVM-OOM line
#                        naming the shared cap), every survivor holds its fill, at least one runs on, and dockerd and
#                        containerd keep their pids and answer
#   bookings_released    after the containers are removed, the memory budget holds no booking
# Needs `make systemvm-up docker-up` and no other microVM running; removes only what it made. Writes
# run/systemvm/vesper-overcommit-report.json.
set -uo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"
SOCKET="${DOCKER_SOCKET:-${ROOT_DIR}/run/vesper/docker.sock}"
REPORT="${ROOT_DIR}/run/systemvm/vesper-overcommit-report.json"
IMAGE="${OVERCOMMIT_IMAGE:-alpine:3.22}"
MAXN="${OVERCOMMIT_MAX:-16}"
MINN="${OVERCOMMIT_MIN:-10}"
# The fill is written to the guest's /dev/shm (tmpfs, 64 MiB in a default container), so its pages stay held
# without a copy: a shell variable needed twice its size and the guest's own OOM killer ended it. 60 MB on top of an
# idle guest's 113 MiB, ten times, passes the 1333 MiB cap.
FILL="${OVERCOMMIT_FILL_MB:-60}"
export DOCKER_HOST="unix://${SOCKET}"
mkdir -p "$(dirname "${REPORT}")"
cargo build -q --bin systemvmctl || exit 2
CTL="${ROOT_DIR}/target/debug/systemvmctl"
ctl() { "${CTL}" control "$@" </dev/null 2>&1; }
steps=()
record() { local d; d="$(printf '%s' "$3" | tr '\n' ' ' | cut -c1-900 | sed 's/\\/\\\\/g; s/"/\\"/g')"; steps+=("{\"name\":\"$1\",\"ok\":$2,\"detail\":\"${d}\"}"); if [ "$2" = true ]; then echo "ok   $1: ${d:0:240}"; else echo "FAIL $1: ${d:0:400}"; fi; }
# This run's own label and names: a sweep removes only what this run made (council D-108 review).
RUN="oc$$-$(date +%s)"
LABEL="vesper.overcommit=${RUN}"
sweep() { docker ps -aq --filter "label=${LABEL}" </dev/null 2>/dev/null | xargs docker rm -f >/dev/null 2>&1; }
trap sweep EXIT
pid_of() { ctl fcparity memparts | sed -n "s/^OOMADJ $1 pid=\([0-9]*\) .*/\1/p" | head -1; }
adj_of() { ctl fcparity memparts | sed -n "s/^OOMADJ $1 pid=[0-9]* adj=\(-*[0-9]*\).*/\1/p" | sort -u | tr '\n' ' '; }
parent_oom() { ctl fcparity memparts | sed -n 's/^PARENT .* oom=\([0-9]*\) .*/\1/p' | head -1; }
committed() { ctl vesperd mem-budget | sed -n 's/.* committed=\([0-9]*\).*/\1/p' | head -1; }
docker image inspect "${IMAGE}" >/dev/null 2>&1 || docker pull -q "${IMAGE}" >/dev/null 2>&1

# --- fill_to_admission ------------------------------------------------------------------------------------------
n=0; why=""
for i in $(seq 1 "${MAXN}"); do
  out="$(docker run -d --runtime vesper --label "${LABEL}" --name "${RUN}-${i}" "${IMAGE}" sleep 3600 </dev/null 2>&1)" || { why="${out}"; break; }
  n="${i}"
done
parent="$(ctl fcparity memparts | grep '^PARENT' | head -1)"
up="$(docker ps -q --filter "label=${LABEL}" --filter status=running </dev/null 2>/dev/null | wc -l | tr -d ' ')"
case "${why}" in
  *"ERR memory"*)
    if [ "${n}" -ge "${MINN}" ] && [ "${up}" = "${n}" ]; then
      record fill_to_admission true "${n} idle containers running at once (at least ${MINN}); next: ${why:0:200}; ${parent}"
    else
      record fill_to_admission false "${n} started, ${up} running at once (at least ${MINN} needed); next: ${why:0:300}; ${parent}"
    fi ;;
  "") record fill_to_admission false "all ${MAXN} started and admission refused none (raise OVERCOMMIT_MAX); ${parent}" ;;
  *) record fill_to_admission false "${n} started; the next failed without a memory reason: ${why:0:300}" ;;
esac

# --- daemons_protected ------------------------------------------------------------------------------------------
dd_pid="$(pid_of dockerd)"; cd_pid="$(pid_of containerd)"
a_dd="$(adj_of dockerd)"; a_cd="$(adj_of containerd)"; a_fc="$(adj_of firecracker)"
if [ "${a_dd}" = "-900 " ] && [ "${a_cd}" = "-900 " ] && [ "${a_fc}" = "0 " ]; then
  record daemons_protected true "dockerd ${a_dd}(pid ${dd_pid}); containerd ${a_cd}(pid ${cd_pid}); firecracker ${a_fc}"
else
  record daemons_protected false "dockerd '${a_dd}'; containerd '${a_cd}'; firecracker '${a_fc}'"
fi

# --- pressure_kills_only_guests ---------------------------------------------------------------------------------
p0="$(parent_oom)"
for i in $(seq 1 "${n}"); do
  # The marker says the fill is held; a survivor without it never applied its pressure.
  docker exec -d "${RUN}-${i}" sh -c "dd if=/dev/zero of=/dev/shm/fill bs=1M count=${FILL} 2>/dev/null && touch /tmp/filled; sleep 600" </dev/null >/dev/null 2>&1 &
done
wait
sleep 40
st="$(docker ps -a --filter "label=${LABEL}" --format '{{.Names}} {{.Status}}' </dev/null 2>&1 | sort -V | tr '\n' ';')"
killed="$(printf '%s' "${st}" | tr ';' '\n' | grep -c 'Exited (137)')"
running="$(printf '%s' "${st}" | tr ';' '\n' | grep -c ' Up ')"
other="$(printf '%s' "${st}" | tr ';' '\n' | grep 'Exited' | grep -vc 'Exited (137)')"
# Each killed guest has its own line naming the shared cap; each survivor holds its fill.
log="$(ctl logs messages --lines 4000)"
unmatched=""; unfilled=""
for c in $(docker ps -aq --no-trunc --filter "label=${LABEL}" --filter status=exited </dev/null 2>/dev/null); do
  printf '%s' "${log}" | grep -q "VESPER-MICROVM-OOM ${c}: .*shared memory cap" || unmatched="${unmatched} ${c:0:12}"
done
for c in $(docker ps -q --filter "label=${LABEL}" --filter status=running </dev/null 2>/dev/null); do
  docker exec "${c}" test -f /tmp/filled </dev/null >/dev/null 2>&1 || unfilled="${unfilled} ${c}"
done
p1="$(parent_oom)"
parent="$(ctl fcparity memparts | grep '^PARENT' | head -1)"
info="$(docker info --format '{{.ContainersRunning}}' </dev/null 2>&1)"
dd_after="$(pid_of dockerd)"; cd_after="$(pid_of containerd)"
detail="killed ${killed}, running ${running}, other exits ${other}; kills without their shared-cap line:${unmatched:- none}; survivors without their fill:${unfilled:- none}; parent oom ${p0}->${p1}; ${parent}; dockerd ${dd_pid}->${dd_after}, containerd ${cd_pid}->${cd_after}, docker info running=${info}; ${st}"
if [ "${killed}" -ge 1 ] && [ "${running}" -ge 1 ] && [ "${other}" = 0 ] && [ -z "${unmatched}" ] && [ -z "${unfilled}" ] \
  && [ -n "${p0}" ] && [ -n "${p1}" ] && [ "${p1}" -gt "${p0}" ] \
  && [ -n "${dd_pid}" ] && [ "${dd_pid}" = "${dd_after}" ] && [ -n "${cd_pid}" ] && [ "${cd_pid}" = "${cd_after}" ] && [ "${info}" = "${running}" ]; then
  record pressure_kills_only_guests true "${detail}"
else
  record pressure_kills_only_guests false "${detail}"
fi

# --- bookings_released ------------------------------------------------------------------------------------------
sweep; sleep 2
c="$(committed)"
[ "${c}" = 0 ] && record bookings_released true "committed=${c}" || record bookings_released false "committed=${c:-unread}"

passed=true; for s in "${steps[@]}"; do case "$s" in *'"ok":false'*) passed=false ;; esac; done
printf '{"steps":[%s],"passed":%s}\n' "$(IFS=,; echo "${steps[*]}")" "${passed}" > "${REPORT}"
echo "vesper-overcommit-check: passed=${passed} (report: ${REPORT})"
[ "${passed}" = true ]
