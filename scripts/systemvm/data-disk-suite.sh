#!/usr/bin/env bash
# GH #164: the System VM's persistent data disk. Proves that images, containers and volumes survive a reboot, that
# a disk is formatted only on first use, and that a disk with another label is never formatted. Restarts the System
# VM itself (twice with the default disk, twice with a scratch disk) and leaves it running on the default disk.
# Needs `make systemvm-prepare`, `make docker-up` (the proxy survives the restarts) and network access for one pull.
set -uo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"
SOCKET="${DOCKER_SOCKET:-${ROOT_DIR}/run/vesper/docker.sock}"
REPORT="${ROOT_DIR}/run/systemvm/data-disk-report.json"
IMAGE="${DATA_IMAGE:-alpine:3.22}"
SCRATCH="${ROOT_DIR}/run/systemvm/data-disk-suite-scratch.img"
DOCKER="${DOCKER_BIN:-docker}"
export DOCKER_HOST="unix://${SOCKET}"
[ -S "${SOCKET}" ] || { echo "data-disk-suite: ${SOCKET} missing; run 'make docker-up'" >&2; exit 2; }
steps=()
record() { local d; d="$(printf '%s' "$3" | tr '\n' ' ' | cut -c1-300 | sed 's/\\/\\\\/g; s/"/\\"/g')"; steps+=("{\"name\":\"$1\",\"ok\":$2,\"detail\":\"${d}\"}"); if [ "$2" = true ]; then echo "ok   $1: $3"; else echo "FAIL $1: $3"; fi; }
data() { cargo run -q --bin systemvmctl -- control data "$@" 2>&1; }
# Waits until the System VM has decided about its data disk (the decision is made before containerd starts).
data_state() { local out=""; for _ in $(seq 1 60); do out="$(data status)"; case "${out}" in *"DATA formatted"*|*"DATA mounted"*|*"DATA refused"*|*"DATA absent"*) break ;; esac; sleep 1; done; echo "${out}"; }
docker_ready() { for _ in $(seq 1 60); do "${DOCKER}" version >/dev/null 2>&1 && return 0; sleep 1; done; return 1; }
# reboot DISK: regenerate the launch config for DISK and restart the System VM; prints the launcher's ready time.
reboot() {
  bash scripts/systemvm/down.sh >/dev/null 2>&1
  VESPER_DATA_DISK="$1" bash scripts/systemvm/generate-launch-config.sh >/dev/null 2>&1 || return 1
  bash scripts/systemvm/up.sh >/dev/null 2>&1 || return 1
  sed -n 's/.*"status":"ready","elapsed_ms":\([0-9]*\).*/\1/p' run/systemvm/vmlauncher.log | tail -1
}
in_vm() { "${DOCKER}" run --rm --privileged --pid=host "${IMAGE}" nsenter -t 1 -m sh -c "$1" 2>&1; }

# 1. The default disk is formatted or mounted, and the state directories live on it.
ms="$(reboot "${ROOT_DIR}/run/systemvm/data.img")"; st="$(data_state)"; docker_ready
case "${st}" in *"DATA formatted /dev/"*|*"DATA mounted /dev/"*) record data_disk_in_use true "${st}; ready in ${ms} ms" ;; *) record data_disk_in_use false "${st}" ;; esac
"${DOCKER}" pull -q "${IMAGE}" >/dev/null 2>&1
binds="$(in_vm 'for d in /var/lib/docker /var/lib/containerd /var/lib/vesper/rootfs-cache; do grep -q " $d ext4 " /proc/mounts && echo ok; done' | grep -c ok)"
[ "${binds}" = 3 ] && record state_dirs_on_disk true "/var/lib/docker, /var/lib/containerd and /var/lib/vesper/rootfs-cache are on the data disk" || record state_dirs_on_disk false "${binds} of 3 state directories are on the disk"

# 2. An image, a stopped container and a volume survive a reboot, and the disk is not formatted again.
"${DOCKER}" rm -f vesper-data-probe >/dev/null 2>&1; "${DOCKER}" volume rm -f vesper-data-probe >/dev/null 2>&1
stamp="persist-$(date +%s)"
"${DOCKER}" run --rm -v vesper-data-probe:/v "${IMAGE}" sh -c "echo ${stamp} > /v/stamp" >/dev/null 2>&1
"${DOCKER}" create --name vesper-data-probe "${IMAGE}" echo hello >/dev/null 2>&1
ms="$(reboot "${ROOT_DIR}/run/systemvm/data.img")"; st="$(data_state)"; docker_ready
case "${st}" in *"DATA mounted /dev/"*) record data_disk_not_reformatted true "${st}" ;; *) record data_disk_not_reformatted false "second boot: ${st}" ;; esac
"${DOCKER}" image inspect "${IMAGE}" >/dev/null 2>&1 && record image_survives_reboot true "${IMAGE} present after reboot without a pull" || record image_survives_reboot false "${IMAGE} missing after reboot"
out="$("${DOCKER}" start -a vesper-data-probe 2>&1)"
[ "${out}" = hello ] && record container_survives_reboot true "the stopped container started after reboot and printed: ${out}" || record container_survives_reboot false "${out}"
out="$("${DOCKER}" run --rm -v vesper-data-probe:/v "${IMAGE}" cat /v/stamp 2>&1)"
[ "${out}" = "${stamp}" ] && record volume_survives_reboot true "named volume kept ${stamp}" || record volume_survives_reboot false "expected ${stamp}, got ${out}"
"${DOCKER}" rm -f vesper-data-probe >/dev/null 2>&1; "${DOCKER}" volume rm -f vesper-data-probe >/dev/null 2>&1
[ -n "${ms}" ] && [ "${ms}" -le 5000 ] && record boot_within_budget true "ready in ${ms} ms with the disk mounted" || record boot_within_budget false "ready in ${ms:-?} ms (budget 5000)"

# 3. A disk with another label is never formatted; the System VM falls back to tmpfs and Docker still works.
/bin/rm -f "${SCRATCH}"
reboot "${SCRATCH}" >/dev/null; data_state >/dev/null; docker_ready
dev="$(data status | sed -n 's/.*DATA [a-z]* \(\/dev\/[a-z]*\).*/\1/p')"
"${DOCKER}" pull -q "${IMAGE}" >/dev/null 2>&1
in_vm "e2label ${dev:-/dev/vdb} not-vesper" >/dev/null
reboot "${SCRATCH}" >/dev/null; st="$(data_state)"; docker_ready
label="$("${DOCKER}" pull -q "${IMAGE}" >/dev/null 2>&1; in_vm "blkid ${dev:-/dev/vdb}" | sed -n 's/.*LABEL="\([^"]*\)".*/\1/p')"
case "${st}" in *"DATA refused /dev/"*"foreign:ext4:not-vesper"*)
  [ "${label}" = not-vesper ] && record foreign_disk_not_formatted true "${st}; label still ${label}" || record foreign_disk_not_formatted false "${st}; label now ${label}" ;;
  *) record foreign_disk_not_formatted false "${st}" ;; esac
"${DOCKER}" run --rm "${IMAGE}" true >/dev/null 2>&1 && record docker_works_without_disk true "containers run on tmpfs when the disk is refused" || record docker_works_without_disk false "docker run failed with the disk refused"

# Back to the default disk.
reboot "${ROOT_DIR}/run/systemvm/data.img" >/dev/null; data_state >/dev/null; docker_ready
/bin/rm -f "${SCRATCH}"

passed=true; for s in "${steps[@]}"; do case "$s" in *'"ok":false'*) passed=false;; esac; done
printf '{"steps":[%s],"passed":%s}\n' "$(IFS=,; echo "${steps[*]}")" "${passed}" > "${REPORT}"
echo "data disk suite: passed=${passed} (report: ${REPORT})"
[ "${passed}" = true ]
