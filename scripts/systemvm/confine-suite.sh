#!/usr/bin/env bash
# GH #194 / ADR 0010 checklist: no container is root on the System VM, whatever flags started it.
#
# Every container is remapped; the flags that need the System VM's root are refused; a container with every
# capability Docker still grants, no seccomp filter and the whole System VM bound at /host cannot read root-only
# Vesper state, enter a microVM's share namespace, reach its share server (ADR 0009, D-034), open the disk, KVM or
# vsock devices, talk to Docker, containerd or the agent, or load a module; the authorisation plugin refuses
# requests it cannot read and routes it does not list; dockerd refuses everything while the plugin is down; and a
# data disk that normal boots use refuses a boot that asks for unmapped containers.
#
# Needs `make systemvm-up docker-up` on the default data disk. The last two steps restart the System VM twice
# (CONFINE_REBOOT=0 skips them). Report: run/systemvm/confine-report.json.
set -uo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SOCKET="${DOCKER_SOCKET:-${ROOT_DIR}/run/vesper/docker.sock}"
REPORT="${ROOT_DIR}/run/systemvm/confine-report.json"
IMAGE="${CONFINE_IMAGE:-alpine:3.22}"
DOCKER="${DOCKER_BIN:-docker}"
CTL=(cargo run -q --bin systemvmctl -- control)
CFG="${ROOT_DIR}/build/systemvm/launch-config.json"
SAVED="${ROOT_DIR}/build/systemvm/launch-config.confine-saved.json" # not in any shared directory
ID=confine-a
SRV=169.254.77.1
PORT=2049
export DOCKER_HOST="unix://${SOCKET}"
cd "${ROOT_DIR}"; mkdir -p "$(dirname "${REPORT}")"
steps=()
record() { local d; d="$(printf '%s' "$3" | tr '\n' ' ' | cut -c1-600 | sed 's/\\/\\\\/g; s/"/\\"/g')"; steps+=("{\"name\":\"$1\",\"ok\":$2,\"detail\":\"${d}\"}"); if [ "$2" = true ]; then echo "ok   $1"; else echo "FAIL $1: ${d}"; fi; }
ctl() { "${CTL[@]}" "$@" 2>&1; }
health() { ctl health | sed -n "s/.*\"$1\":\"\\([^\"]*\\)\".*/\\1/p"; }
[ -S "${SOCKET}" ] || { echo "confine-suite: ${SOCKET} missing; run 'make docker-up'" >&2; exit 2; }
"${DOCKER}" image inspect "${IMAGE}" >/dev/null 2>&1 || "${DOCKER}" pull -q "${IMAGE}" >/dev/null 2>&1 || { echo "confine-suite: cannot pull ${IMAGE}" >&2; exit 2; }
cleanup() {
  ctl microvm test-share-listen "${ID}" stop >/dev/null; ctl microvm -- cleanup "${ID}" --logs >/dev/null
  "${DOCKER}" rm -f confine-probe >/dev/null 2>&1; "${DOCKER}" volume rm -f confine-vol >/dev/null 2>&1
  if [ -f "${SAVED}" ]; then # a reboot step was interrupted: put the launch config back
    cp "${SAVED}" "${CFG}" && rm -f "${SAVED}"
    bash scripts/systemvm/docker-down.sh >/dev/null 2>&1; bash scripts/systemvm/down.sh >/dev/null 2>&1
    bash scripts/systemvm/up.sh >/dev/null 2>&1; bash scripts/systemvm/docker-up.sh >/dev/null 2>&1
  fi
}
trap cleanup EXIT

# --- 1. Every container is remapped ----------------------------------------------------------------------------
sec="$("${DOCKER}" info --format '{{json .SecurityOptions}} {{json .Plugins.Authorization}}' 2>&1)"
map="$("${DOCKER}" run --rm "${IMAGE}" cat /proc/self/uid_map 2>&1 | tr -s ' ')"
mode="$(health containers)"; authz="$(health authz)"
if [ "${map}" = " 0 100000 65536" ] && [ "${mode}" = mapped ] && [ "${authz}" = started ] && case "${sec}" in *name=userns*vesper-authz*) true ;; *) false ;; esac; then
  record every_container_is_remapped true "uid_map:${map}; HEALTH containers=${mode} authz=${authz}; docker info: ${sec}"
else
  record every_container_is_remapped false "uid_map:${map}; HEALTH containers=${mode} authz=${authz}; docker info: ${sec}"
fi

# --- 2. The flags that need the System VM's root are refused ---------------------------------------------------
bad=""; seen=""
for flag in --privileged --pid=host --network=host --ipc=host --userns=host; do
  out="$("${DOCKER}" run --rm "${flag}" "${IMAGE}" cat /proc/self/uid_map 2>&1)"; rc=$?
  seen="${seen} ${flag}:rc${rc}"
  # Refused or unable to start (125), and never a container that printed its uid map.
  { [ "${rc}" = 125 ] && ! printf '%s' "${out}" | grep -q '4294967295\|100000'; } || bad="${bad} ${flag}(rc ${rc}: $(printf '%s' "${out}" | head -1))"
done
[ -z "${bad}" ] && record host_root_flags_refused true "${seen}" || record host_root_flags_refused false "not refused:${bad}"

# --- 3. The broadest container Docker still starts is not root on the System VM --------------------------------
ctl microvm -- cleanup "${ID}" --logs >/dev/null
launched="$(ctl microvm -- launch "${ID}" --share-net)"
listening="$(ctl microvm test-share-listen "${ID}" "${PORT}")"
REC="/run/vesper/microvm/records/${ID}"; NS="/run/vesper/microvm/netns/${ID}"; LOG="/var/log/vesper-microvm-${ID}.log"
BROAD=(--cap-add ALL --security-opt seccomp=unconfined --security-opt apparmor=unconfined
  --security-opt systempaths=unconfined --uts=host --cgroupns=host --device /dev/kvm --device /dev/vsock -v /:/host)
# One line per attempt: GOT when the command succeeded (the container did what it must not), NO when it failed.
PROBE='p() { if out=$(sh -c "$1" 2>&1); then echo "GOT $1 :: $(printf "%s" "$out" | head -c 80 | tr "\n" " ")"; else echo "NO $1"; fi; }
p "cat /host'"${REC}"'"
p "cat /host'"${LOG}"'"
p "cat /host/var/log/vesper-authz.log"
p "cat /host/var/log/docker.log"
p "ls /host/run/vesper/microvm"
p "ls /host/var/lib/vesper/workloads"
p "ls /host/srv/jailer"
p "cat /host/etc/shadow"
p "nsenter --net=/host'"${NS}"' true"
p "nc -z -w 2 '"${SRV} ${PORT}"'"
p "head -c1 /host/dev/vda"
p "head -c0 /host/dev/kvm"
p "head -c0 /dev/kvm"
p "head -c0 /host/dev/vsock"
p "head -c0 /dev/vsock"
p "chroot /host docker ps"
p "chroot /host ctr -a /run/containerd/containerd.sock version"
p "chroot /host /sbin/modprobe nbd"
p "hostname confine-pwned"
p "mkdir /host/sys/fs/cgroup/confine-pwned"
p "mknod /tmp/vda b 253 0"'
if case "${launched}" in *"LAUNCHED ${ID} "*) true ;; *) false ;; esac && case "${listening}" in *"LISTEN ${ID} tcp=${SRV}:${PORT}"*) true ;; *) false ;; esac; then
  out="$("${DOCKER}" run --rm "${BROAD[@]}" "${IMAGE}" sh -c "${PROBE}" 2>&1)"
  got="$(printf '%s\n' "${out}" | grep '^GOT ' | tr '\n' ';')"; n="$(printf '%s\n' "${out}" | grep -c '^NO ')"
  [ -z "${got}" ] && [ "${n}" = 21 ] && record broadest_container_is_not_root true "${n} attempts failed with ${BROAD[*]}: root-only record, console log, plugin and Docker logs, share namespace, share server ${SRV}:${PORT}, disk, kvm, vsock, Docker and containerd sockets, modprobe, hostname, cgroup, mknod" \
    || record broadest_container_is_not_root false "${n} of 21 failed; succeeded: ${got:-none}; output: $(printf '%s' "${out}" | tail -3)"
  # The fixture is real: root in the System VM does reach the stand-in server inside the namespace.
  reach="$(ctl microvm test-share-reach "${PORT}")"
  out="$("${DOCKER}" run --rm -v "${LOG}:/x:ro" "${IMAGE}" cat /x 2>&1)"; rc1=$?
  out2="$("${DOCKER}" run --rm -v "${REC}:/x:ro" "${IMAGE}" cat /x 2>&1)"; rc2=$?
  [ "${rc1}" != 0 ] && [ "${rc2}" != 0 ] && record single_file_binds_unreadable true "a console log and a launch record bound as single files: cat failed (rc ${rc1}, ${rc2}): $(printf '%s' "${out}" | head -1)" \
    || record single_file_binds_unreadable false "console log rc ${rc1}: ${out}; record rc ${rc2}: ${out2}"
  case "${reach}" in *"REACH container-bridge closed"*"REACH container-host refused"*) record share_server_closed_to_containers true "$(printf '%s' "${reach}" | tr '\n' ';')" ;;
    *) record share_server_closed_to_containers false "$(printf '%s' "${reach}" | tr '\n' ';')" ;; esac
else
  record broadest_container_is_not_root false "fixture: launch: $(printf '%s' "${launched}" | tail -1); listen: ${listening}"
fi

# --- 4. The vsock listeners serve the macOS host only -----------------------------------------------------------
cid="$(python3 -c 'import json,os,sys; print(os.path.basename(json.load(open(sys.argv[1])).get("vsock_bridge_path", "3.sock")).split(".")[0])' "${CFG}")"
out="$("${DOCKER}" run --rm "${BROAD[@]}" "${IMAGE}" sh -c '
  for c in 1 2 '"${cid}"'; do for port in 1024 2375; do
    r=$(printf "PING\nGET /_ping HTTP/1.0\r\n\r\n" | chroot /host timeout 4 socat -T3 - VSOCK-CONNECT:$c:$port 2>&1 | grep -v "W open" | sed "s/^[0-9/: ]*socat\[[0-9]*\] //" | head -c 160 | tr "\n" " ")
    echo "cid $c port $port: $r"
  done; done' 2>&1)"
if printf '%s' "${out}" | grep -q 'VESPER-AGENT\|PONG\|HTTP/1'; then record vsock_listeners_serve_the_host_only false "${out}"; else record vsock_listeners_serve_the_host_only true "${out}"; fi

# --- 5. The plugin refuses what it cannot read and what it does not list ----------------------------------------
out="$(python3 -I "${ROOT_DIR}/scripts/systemvm/confine-requests.py" "${SOCKET}" "${IMAGE}" 2>&1)"; rc=$?
[ "${rc}" = 0 ] && record plugin_refuses_unreadable_and_unlisted true "${out}" || record plugin_refuses_unreadable_and_unlisted false "${out}"
bad=""
o="$("${DOCKER}" volume create -o type=none -o o=bind -o device=/run confine-vol 2>&1)" && bad="${bad} volume-options"
o2="$("${DOCKER}" run --rm --runtime io.containerd.vesper.v2 "${IMAGE}" true 2>&1)" && bad="${bad} runtime"
o3="$("${DOCKER}" run --rm --mount 'type=volume,dst=/v,volume-driver=local,volume-opt=type=tmpfs,volume-opt=device=tmpfs' "${IMAGE}" true 2>&1)" && bad="${bad} mount-options"
o4="$("${DOCKER}" volume create confine-vol 2>&1)" || bad="${bad} plain-volume-refused"
[ -z "${bad}" ] && record driver_options_and_runtimes_refused true "volume options: ${o}; runtime: $(printf '%s' "${o2}" | head -1); a plain volume is created" \
  || record driver_options_and_runtimes_refused false "wrong:${bad}; ${o}; ${o2}; ${o3}; ${o4}"

# --- 6. Cost of the plugin on every call ------------------------------------------------------------------------
ms="$(python3 -I -c 'import subprocess, sys, time
t = time.time()
for _ in range(50): subprocess.run([sys.argv[1], "version", "--format", "{{.Server.Version}}"], capture_output=True)
print(int((time.time() - t) * 1000 / 50))' "${DOCKER}")"
[ -n "${ms}" ] && [ "${ms}" -le 200 ] && record plugin_latency_is_small true "docker version: ${ms} ms per call over 50 calls (client start, proxy, relay, plugin request and response hooks)" \
  || record plugin_latency_is_small false "docker version: ${ms:-?} ms per call (budget 200)"

# --- 7. Without the plugin dockerd refuses every request, and recovers ------------------------------------------
stopped="$(ctl containers test-authz stop)"
out="$(python3 -I -c 'import subprocess, sys
try:
    r = subprocess.run([sys.argv[1], "version", "--format", "{{.Server.Version}}"], capture_output=True, text=True, timeout=90)
    print(r.returncode, (r.stdout + r.stderr).strip().replace("\n", " ")[:200])
except subprocess.TimeoutExpired:
    print("timeout")' "${DOCKER}")"
h="$(health authz)"
started="$(ctl containers test-authz start)"
back=""; for _ in $(seq 1 30); do back="$("${DOCKER}" version --format '{{.Server.Version}}' 2>/dev/null)"; [ -n "${back}" ] && break; sleep 1; done
if case "${out}" in 0\ *|timeout) false ;; *) true ;; esac && [ -n "${back}" ] && case "${started}" in *"AUTHZ started"*) true ;; *) false ;; esac; then
  record plugin_loss_fails_closed true "${stopped}; docker version -> ${out}; HEALTH authz=${h}; ${started}; docker version -> ${back}"
else
  record plugin_loss_fails_closed false "${stopped}; docker version -> ${out}; HEALTH authz=${h}; ${started}; after: ${back:-no answer}"
fi

# --- 8. A normal data disk refuses an unmapped boot, and the next normal boot is mapped -------------------------
if [ "${CONFINE_REBOOT:-1}" = 1 ]; then
  cleanup_fixture() { ctl microvm test-share-listen "${ID}" stop >/dev/null; ctl microvm -- cleanup "${ID}" --logs >/dev/null; }
  cleanup_fixture
  cp "${CFG}" "${SAVED}"
  python3 -I -c 'import json, sys
c = json.load(open(sys.argv[1]))
c["kernel_cmdline"] = c["kernel_cmdline"] + " vesper.containers=unmapped"
json.dump(c, open(sys.argv[1], "w"), indent=2)' "${CFG}"
  bash scripts/systemvm/docker-down.sh >/dev/null 2>&1; bash scripts/systemvm/down.sh >/dev/null 2>&1
  # dockerd does not start, so the launcher's boot probe fails and `make systemvm-up` fails with it. The System
  # VM's console says why.
  up="$(bash scripts/systemvm/up.sh 100 2>&1 | tail -1)"; uprc=$?
  con="${ROOT_DIR}/run/systemvm/console.log"
  why="$(grep -a 'vesper: CONTAINERS ' "${con}" | tail -1 | tr -d '\r')"
  if [ "${uprc}" != 0 ] && [ "${why}" = "vesper: CONTAINERS refused disk-is-mapped:boot-asked-unmapped" ] && ! grep -aq 'vesper: service docker started' "${con}"; then
    record mapped_disk_refuses_unmapped_boot true "console: ${why}; dockerd was not started; systemvm-up failed (rc ${uprc}): ${up}"
  else
    record mapped_disk_refuses_unmapped_boot false "console: ${why:-no CONTAINERS line}; systemvm-up rc ${uprc}: ${up}"
  fi
  cp "${SAVED}" "${CFG}" && rm -f "${SAVED}"
  bash scripts/systemvm/down.sh >/dev/null 2>&1
  bash scripts/systemvm/up.sh >/dev/null 2>&1
  up="$(bash scripts/systemvm/docker-up.sh 2>&1 | grep 'containers ' | head -1)"
  map="$("${DOCKER}" run --rm "${IMAGE}" cat /proc/self/uid_map 2>&1 | tr -s ' ')"
  [ "${map}" = " 0 100000 65536" ] && case "${up}" in *"containers mapped"*) true ;; *) false ;; esac \
    && record next_normal_boot_is_mapped true "${up}; uid_map:${map}" || record next_normal_boot_is_mapped false "${up}; uid_map:${map}"
fi

passed=true; for s in "${steps[@]}"; do case "$s" in *'"ok":false'*) passed=false ;; esac; done
printf '{"steps":[%s],"passed":%s}\n' "$(IFS=,; echo "${steps[*]}")" "${passed}" > "${REPORT}"
echo "confine suite: passed=${passed} (report: ${REPORT})"
[ "${passed}" = true ]
