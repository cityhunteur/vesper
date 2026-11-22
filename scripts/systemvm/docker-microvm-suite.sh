#!/usr/bin/env bash
# ADR 0011 slice 2 from macOS: `docker run --runtime vesper` on the default bridge runs the container in a jailed
# Firecracker microVM that owns the container's address. Steps: the runtime is registered; the container runs in a
# microVM; the image's owners are the container's own; dockerd's hostname, hosts and resolv.conf reach it; it reaches
# a runc peer by address and the internet and serves a published port; its writes survive stop and start; frames
# from a spoofed address or MAC go nowhere; every option ADR 0011 refuses is refused, and a runc container cannot
# join a vesper container's namespaces; on a user-defined network it reaches a peer by name and external names
# through dockerd's resolver (slice 3); after `docker rm` nothing is left and the sweep removes the drive.
# Needs `make systemvm-up && make docker-up` and outbound network for the image. Writes
# run/systemvm/docker-microvm-report.json.
set -uo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SOCKET="${DOCKER_SOCKET:-${ROOT_DIR}/run/vesper/docker.sock}"
REPORT="${ROOT_DIR}/run/systemvm/docker-microvm-report.json"
IMAGE="${DOCKER_MICROVM_IMAGE:-alpine:3.22}"
PORT="${DOCKER_MICROVM_PORT:-18091}"
export DOCKER_HOST="unix://${SOCKET}"
CTL=(cargo run -q --bin systemvmctl -- control)
cd "${ROOT_DIR}"; mkdir -p "$(dirname "${REPORT}")"
steps=()
record() { local d; d="$(printf '%s' "$3" | tr '\n' ' ' | cut -c1-600 | sed 's/\\/\\\\/g; s/"/\\"/g')"; steps+=("{\"name\":\"$1\",\"ok\":$2,\"detail\":\"${d}\"}"); if [ "$2" = true ]; then echo "ok   $1"; else echo "FAIL $1: ${d:0:300}"; fi; }
ctl() { "${CTL[@]}" "$@" 2>&1; }
NAMES="vm-a vm-b vm-p vm-n peer-a peer-n joiner"
NET="${DOCKER_MICROVM_NET:-vesper-microvm-net}"
cleanup() { docker rm -f ${NAMES} >/dev/null 2>&1; docker network rm "${NET}" >/dev/null 2>&1; }
trap cleanup EXIT
[ -S "${SOCKET}" ] || { echo "docker-microvm-suite: ${SOCKET} missing; run 'make docker-up'" >&2; exit 2; }
docker image inspect "${IMAGE}" >/dev/null 2>&1 || docker pull -q "${IMAGE}" >/dev/null 2>&1 || { echo "docker-microvm-suite: cannot pull ${IMAGE}" >&2; exit 2; }
cleanup
ctl microvm -- drives-sweep >/dev/null
# Wait until a container's command has started (a vesper container boots its microVM first).
up() { local i; for i in $(seq 1 60); do docker exec "$1" true >/dev/null 2>&1 && return 0; sleep 0.5; done; return 1; }

# --- 1. The runtime is registered, beside the default ---------------------------------------------------------
rt="$(docker info --format '{{range $k, $v := .Runtimes}}{{$k}} {{end}}/{{.DefaultRuntime}}' 2>&1)"
case "${rt}" in *vesper\ */vesper-runc) record runtime_registered true "${rt}" ;; *) record runtime_registered false "${rt}" ;; esac

# --- 2. It runs in a microVM -------------------------------------------------------------------------------------
docker run -d --name vm-a --runtime vesper --hostname vm-a-host "${IMAGE}" sleep 600 >/dev/null 2>&1
up vm-a; id="$(docker inspect -f '{{.Id}}' vm-a 2>/dev/null)"
st="$(ctl microvm status "${id}" | head -1)"
# docker stats reads the microVM's cgroup: its limit is the default microVM's 352 MiB ceiling (judge N1).
ds="$(docker stats --no-stream --format '{{.MemUsage}}' vm-a 2>&1)"
case "${st}|${ds}" in "RUNNING ${id} "*"|"*"/ 352MiB") record runs_in_a_microvm true "${st}; docker stats: ${ds}" ;;
  *) record runs_in_a_microvm false "${st:-no container}; docker stats: ${ds}" ;; esac

# --- 3. The image's owners are the container's own (dockerd's remap is shifted out of the drive) -------------------
own="$(docker exec vm-a sh -c 'stat -c %u:%g /bin/busybox /etc/passwd /root | tr "\n" " "; id -u' 2>&1)"
case "${own}" in "0:0 0:0 0:0 0") record owners_are_the_containers true "${own}" ;; *) record owners_are_the_containers false "${own}" ;; esac

# --- 4. dockerd's hostname, hosts and resolv.conf reach the guest --------------------------------------------------
f="$(docker exec vm-a sh -c 'hostname; grep -c vm-a-host /etc/hosts; grep -c ^nameserver /etc/resolv.conf' 2>&1 | tr '\n' ' ')"
case "${f}" in "vm-a-host 1 "[1-9]*) record docker_files_reach_the_guest true "${f}" ;; *) record docker_files_reach_the_guest false "${f}" ;; esac

# --- 5. A runc peer by address, the internet, a published port ------------------------------------------------------
docker run -d --name peer-a "${IMAGE}" sh -c 'while true; do echo peer-ok | nc -l -p 9000; done' >/dev/null 2>&1
peer="$(docker inspect -f '{{.NetworkSettings.IPAddress}}' peer-a 2>/dev/null)"
docker run -d --name vm-p --runtime vesper -p "127.0.0.1:${PORT}:8080" "${IMAGE}" sh -c 'while true; do echo vm-ok | nc -l -p 8080; done' >/dev/null 2>&1
up vm-p
p="$(docker exec vm-p sh -c "nc -w 3 ${peer} 9000" 2>&1)"; i="$(docker exec vm-p sh -c 'nc -z -w 3 1.1.1.1 443 && echo open || echo closed' 2>&1)"
pub=""; for _ in $(seq 1 10); do pub="$(nc -w 3 127.0.0.1 "${PORT}" < /dev/null 2>&1)"; [ "${pub}" = vm-ok ] && break; sleep 0.5; done
if [ "${p}" = peer-ok ] && [ "${i}" = open ] && [ "${pub}" = vm-ok ]; then record network_peer_internet_published_port true "peer ${peer}: ${p}; 1.1.1.1:443 ${i}; 127.0.0.1:${PORT}: ${pub}"
else record network_peer_internet_published_port false "peer ${peer}: ${p}; internet: ${i}; published: ${pub}"; fi

# --- 5b. A user-defined network: a peer by name through dockerd's resolver, external names, dockerd's resolv.conf --
docker network create "${NET}" >/dev/null 2>&1
docker run -d --name peer-n --network "${NET}" "${IMAGE}" sh -c 'while true; do echo named-ok | nc -l -p 9000; done' >/dev/null 2>&1
docker run -d --name vm-n --runtime vesper --network "${NET}" "${IMAGE}" sleep 600 >/dev/null 2>&1
up vm-n
n="$(docker exec vm-n sh -c 'grep ^nameserver /etc/resolv.conf; nc -w 3 peer-n 9000; nslookup example.com >/dev/null 2>&1 && echo external-ok' 2>&1 | tr '\n' ' ')"
closed="$(docker exec vm-n sh -c 'nc -z -w 2 169.254.78.1 22 && echo open || echo closed' 2>&1)"
# Two vesper containers on one network reach each other (each guest has a MAC of its own).
docker run -d --name vm-b --runtime vesper --network "${NET}" "${IMAGE}" sh -c 'while true; do echo vm-b-ok | nc -l -p 9100; done' >/dev/null 2>&1
up vm-b
vv="$(docker exec vm-n sh -c 'nc -w 3 vm-b 9100' 2>&1)"
n="${n}${vv} "
case "${n}|${closed}" in "nameserver 127.0.0.11 named-ok external-ok vm-b-ok |closed") record user_network_names_and_dns true "${n}; other ports on the DNS link: ${closed}" ;;
  *) record user_network_names_and_dns false "${n}; other ports on the DNS link: ${closed}" ;; esac
docker rm -f vm-n vm-b peer-n >/dev/null 2>&1

# --- 6. A spoofed source address and a spoofed MAC go nowhere; the real ones still work ----------------------------
sp="$(docker exec vm-p sh -c "ip addr add 172.17.250.250/16 dev eth0 && (nc -w 3 -s 172.17.250.250 ${peer} 9000 || echo no-reply); ip addr del 172.17.250.250/16 dev eth0" 2>&1 | tr '\n' ' ')"
mac="$(docker exec vm-p sh -c "m=\$(cat /sys/class/net/eth0/address); ifconfig eth0 hw ether 02:00:00:00:00:99 && (nc -w 3 ${peer} 9000 || echo no-reply); ifconfig eth0 hw ether \$m" 2>&1 | tr '\n' ' ')"
after="$(docker exec vm-p sh -c "nc -w 3 ${peer} 9000" 2>&1)"
case "${sp}|${mac}|${after}" in *no-reply*"|"*no-reply*"|peer-ok") record spoofed_frames_go_nowhere true "address: ${sp}; MAC: ${mac}; real: ${after}" ;;
  *) record spoofed_frames_go_nowhere false "address: ${sp}; MAC: ${mac}; real: ${after}" ;; esac

# --- 7. Writes survive stop and start ------------------------------------------------------------------------------
docker exec vm-a sh -c 'echo kept-across-starts > /kept.txt' >/dev/null 2>&1
docker stop -t 2 vm-a >/dev/null 2>&1; stopped="$(ctl microvm status "${id}" | head -1)"
# The reaper's sweep runs every 10 s; a stopped container's drive must survive it (council REL-1, COR-1).
ctl microvm -- drives-sweep >/dev/null
docker start vm-a >/dev/null 2>&1; up vm-a; k="$(docker exec vm-a cat /kept.txt 2>&1)"
case "${stopped}|${k}" in "ABSENT ${id}|kept-across-starts") record writes_survive_stop_and_start true "stopped: ${stopped}; read back: ${k}" ;;
  *) record writes_survive_stop_and_start false "stopped: ${stopped}; read back: ${k}" ;; esac

# --- 8. Every option ADR 0011 refuses is refused by name ----------------------------------------------------------
bad=""
for opt in "--privileged" "--network host" "--network container:peer-a" "-v /tmp:/x" "--read-only" "--device /dev/null" "--pid host" "--ipc host"; do
  # shellcheck disable=SC2086
  out="$(docker create --runtime vesper ${opt} "${IMAGE}" true 2>&1)"
  case "${out}" in *"(ADR 0011)"*) ;; *) bad="${bad} [${opt}: ${out:0:120}]"; docker rm -f "${out}" >/dev/null 2>&1 ;; esac
done
for mode in "--network container:vm-a" "--pid container:vm-a" "--ipc container:vm-a"; do
  # shellcheck disable=SC2086
  out="$(docker create --name joiner ${mode} "${IMAGE}" true 2>&1)"
  case "${out}" in *"(ADR 0011)"*) ;; *) bad="${bad} [runc ${mode}: ${out:0:120}]"; docker rm -f joiner >/dev/null 2>&1 ;; esac
done
tm="$(docker run --rm --runtime vesper --tmpfs /scratch:size=8m "${IMAGE}" sh -c 'grep " /scratch " /proc/mounts | cut -d" " -f3' 2>&1)"
[ "${tm}" = tmpfs ] || bad="${bad} [--tmpfs is honoured: ${tm}]"
[ -z "${bad}" ] && record refused_options_are_refused true "eight vesper options and three runc joins refused with (ADR 0011); --tmpfs honoured" || record refused_options_are_refused false "${bad}"

# --- 9. Nothing is left, and the sweep removes the drives ---------------------------------------------------------
pid_vm_p="$(docker inspect -f '{{.Id}}' vm-p 2>/dev/null)"
docker rm -f vm-a vm-p peer-a >/dev/null 2>&1; sleep 2
a="$(ctl microvm status "${id}" | head -1)"; b="$(ctl microvm status "${pid_vm_p}" | head -1)"
# The reaper sweeps drives on its own pass; this one makes sure, then the list must not name either container.
sw="$(ctl microvm -- drives-sweep | head -1)"; dl="$(ctl microvm -- drives-list | tr '\n' ' ')"; left="$(ctl ctr -- leftovers | head -1)"
case "${a}|${b}|${left}" in "ABSENT ${id}|ABSENT ${pid_vm_p}|CLEAN")
    case "${dl}" in *"${id}"*|*"${pid_vm_p}"*) record nothing_left false "drives remain: ${dl}" ;; *) record nothing_left true "${a}; ${b}; ${sw}; ${dl}; ${left}" ;; esac ;;
  *) record nothing_left false "${a}; ${b}; ${sw}; ${dl}; ${left}" ;; esac

passed=true; for s in "${steps[@]}"; do case "$s" in *'"ok":false'*) passed=false ;; esac; done
printf '{"steps":[%s],"passed":%s}\n' "$(IFS=,; echo "${steps[*]}")" "${passed}" > "${REPORT}"
echo "docker-microvm suite: passed=${passed} (report: ${REPORT})"
[ "${passed}" = true ]
