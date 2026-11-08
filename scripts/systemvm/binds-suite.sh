#!/usr/bin/env bash
# GH #29 checklist: virtio-fs shares configured in the launch config are mounted in the guest at the same
# absolute path, so Docker bind mounts of those host paths work from macOS; ownership mapping, chmod,
# read-only enforcement (share-level and `:ro`), promptness of host edits, inotify behaviour and the path
# policy are recorded. Needs `make systemvm-up` with VESPER_SHARES naming run/WOR-39/share and
# run/WOR-39/share-ro:ro (see Makefile `binds-suite`), `make docker-up`, and the alpine:3.22 image.
set -uo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SOCKET="${DOCKER_SOCKET:-${ROOT_DIR}/run/vesper/docker.sock}"
REPORT="${ROOT_DIR}/run/systemvm/binds-report.json"
SHARE="${BINDS_SHARE:-${ROOT_DIR}/run/WOR-39/share}"
SHARE_RO="${BINDS_SHARE_RO:-${ROOT_DIR}/run/WOR-39/share-ro}"
IMAGE="${BINDS_IMAGE:-alpine:3.22}"
DOCKER="${DOCKER_BIN:-docker}"
CTL=(cargo run -q --bin systemvmctl -- control)
export DOCKER_HOST="unix://${SOCKET}"
cd "${ROOT_DIR}"; mkdir -p "$(dirname "${REPORT}")" "${SHARE}" "${SHARE_RO}"
steps=()
record() { local d; d="$(printf '%s' "$3" | tr '\n' ' ' | cut -c1-300 | sed 's/\\/\\\\/g; s/"/\\"/g')"; steps+=("{\"name\":\"$1\",\"ok\":$2,\"detail\":\"${d}\"}"); if [ "$2" = true ]; then echo "ok   $1: ${d:0:150}"; else echo "FAIL $1: ${d:0:200}"; fi; }
run() { "${DOCKER}" run --rm "$@"; }
[ -S "${SOCKET}" ] || { echo "binds-suite: ${SOCKET} missing; run 'make docker-up'" >&2; exit 2; }
"${DOCKER}" image inspect "${IMAGE}" >/dev/null 2>&1 || "${DOCKER}" pull -q "${IMAGE}" >/dev/null 2>&1 || { echo "binds-suite: cannot pull ${IMAGE}" >&2; exit 2; }
rm -f "${SHARE}"/*; echo "from-host" > "${SHARE}/hello.txt"; echo "read-only content" > "${SHARE_RO}/ro.txt"; "${DOCKER}" rm -f binds-inot >/dev/null 2>&1

out="$("${CTL[@]}" share list 2>&1)"
if echo "${out}" | grep -q "SHARE share0 ${SHARE} mode=rw" && echo "${out}" | grep -q "SHARE share1 ${SHARE_RO} mode=ro"; then record shares_mounted_at_host_paths true "${out}"; else record shares_mounted_at_host_paths false "${out}"; fi

out="$(run -v "${SHARE}:/data" "${IMAGE}" cat /data/hello.txt 2>&1)"
[ "${out}" = "from-host" ] && record host_file_readable_in_container true "cat /data/hello.txt -> ${out}" || record host_file_readable_in_container false "${out}"

host_owner="$(stat -f '%u:%g' "${SHARE}/hello.txt")"; guest_owner="$(run -v "${SHARE}:/data" "${IMAGE}" stat -c '%u:%g' /data/hello.txt 2>&1)"
[ "${guest_owner}" = "0:0" ] && record ownership_mapped_to_root_in_guest true "host ${host_owner} appears as ${guest_owner} in the container (the sharing user owns the share; containers see root)" || record ownership_mapped_to_root_in_guest false "host ${host_owner}, guest ${guest_owner}"

run -v "${SHARE}:/data" "${IMAGE}" sh -c 'echo from-container > /data/c.txt && chmod 640 /data/c.txt && chown 1000:1000 /data/c.txt' >/dev/null 2>&1
h="$(stat -f '%u:%g %Lp' "${SHARE}/c.txt" 2>&1)"; c="$(cat "${SHARE}/c.txt" 2>&1)"; g="$(run -v "${SHARE}:/data" "${IMAGE}" stat -c '%u:%g %a' /data/c.txt 2>&1)"
if [ "${c}" = "from-container" ] && [ "${h}" = "${host_owner} 640" ] && [ "${g}" = "0:0 640" ]; then record container_write_owned_by_host_user true "container wrote c.txt, chmod 640 kept, chown 1000:1000 ignored; host sees ${h}, guest sees ${g}"; else record container_write_owned_by_host_user false "host ${h} content ${c}; guest ${g}"; fi

out="$(run -v "${SHARE_RO}:/ro" "${IMAGE}" sh -c 'cat /ro/ro.txt && echo x > /ro/y.txt' 2>&1 | tail -1)"
case "${out}" in *"Read-only file system"*) record readonly_share_enforced true "${out}" ;; *) record readonly_share_enforced false "${out}" ;; esac
out="$(run -v "${SHARE}:/data:ro" "${IMAGE}" sh -c 'echo x > /data/z.txt' 2>&1 | tail -1)"
case "${out}" in *"Read-only file system"*) record docker_ro_flag_enforced true "${out}" ;; *) record docker_ro_flag_enforced false "${out}" ;; esac

stamp="prompt-$(date +%s%N)"; echo "${stamp}" > "${SHARE}/p.txt"; out="$(run -v "${SHARE}:/data" "${IMAGE}" cat /data/p.txt 2>&1)"
[ "${out}" = "${stamp}" ] && record host_edit_visible_promptly true "host wrote p.txt and the next container read saw it" || record host_edit_visible_promptly false "expected ${stamp}, got ${out}"

"${DOCKER}" run -d --name binds-inot -v "${SHARE}:/data" "${IMAGE}" sh -c 'inotifyd - /data:cwnd > /tmp/ev.log 2>&1 & sleep 120' >/dev/null 2>&1; sleep 1
"${DOCKER}" exec binds-inot sh -c 'echo b > /data/from-guest.txt' >/dev/null 2>&1; echo "host-edit" >> "${SHARE}/hello.txt"; echo "new" > "${SHARE}/from-host.txt"; sleep 2
ev="$("${DOCKER}" exec binds-inot cat /tmp/ev.log 2>&1 | tr '\t' ' ' | tr '\n' ';')"
case "${ev}" in *from-guest.txt*) record inotify_guest_writes_seen true "events: ${ev}" ;; *) record inotify_guest_writes_seen false "events: ${ev}" ;; esac
case "${ev}" in *from-host.txt*|*hello.txt*) record inotify_host_edits_not_propagated false "unexpected: host edits produced inotify events: ${ev}" ;; *) record inotify_host_edits_not_propagated true "host edits produce no inotify events in the guest (virtio-fs limitation, TD-038); contents and mtime are visible, so watchers must poll" ;; esac
"${DOCKER}" rm -f binds-inot >/dev/null 2>&1

out="$(run -v /tmp/vesper-not-shared:/x "${IMAGE}" sh -c 'ls -A /x | wc -l' 2>&1)"
[ "${out}" = "0" ] && [ ! -e /tmp/vesper-not-shared ] && record unshared_path_is_guest_local true "-v /tmp/vesper-not-shared is a guest-local empty directory; only configured shares map host paths (path policy)" || record unshared_path_is_guest_local false "${out}; host dir exists: $([ -e /tmp/vesper-not-shared ] && echo yes || echo no)"

rm -f "${SHARE}"/c.txt "${SHARE}"/p.txt "${SHARE}"/from-guest.txt "${SHARE}"/from-host.txt
passed=true; for s in "${steps[@]}"; do case "$s" in *'"ok":false'*) passed=false;; esac; done
printf '{"share":"%s","share_ro":"%s","steps":[%s],"passed":%s}\n' "${SHARE}" "${SHARE_RO}" "$(IFS=,; echo "${steps[*]}")" "${passed}" > "${REPORT}"
echo "binds suite: passed=${passed} (report: ${REPORT})"
[ "${passed}" = true ]
