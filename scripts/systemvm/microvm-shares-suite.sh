#!/usr/bin/env bash
# GH #160 M4 and M5 / ADR 0009: a containerd task on the Vesper shim gets its bind mounts from a macOS share as NFS
# mounts from its own microVM's server.
#
# Through `ctr run --runtime io.containerd.vesper.v2 --mount type=bind,...` (vesper-ctr run-binds): a read-write bind
# and a read-only bind inside it are NFS mounts in the guest; the guest reads, writes reach macOS, the read-only one
# refuses writes; a macOS write appears in the guest; the GH #159 lost write (`>` then `>>`) does not recur in 1,000
# tries, judged in the guest and from macOS after the task; a file source is refused with a clear error; a bind from
# outside every share keeps today's behaviour; a non-root guest user writes; nothing is left afterwards.
# Needs `make systemvm-up-shares`. Report: run/systemvm/microvm-shares-report.json.
set -uo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SHARE="${SHARES_SHARE:-${ROOT_DIR}/run/WOR-39/share}"
REPORT="${ROOT_DIR}/run/systemvm/microvm-shares-report.json"
CTL=(cargo run -q --bin systemvmctl -- control)
F="${SHARE}/m4"
TRIES="${SHARES_TRIES:-1000}"
cd "${ROOT_DIR}"; mkdir -p "$(dirname "${REPORT}")"
steps=()
record() { local d; d="$(printf '%s' "$3" | tr '\n' ' ' | cut -c1-900 | sed 's/\\/\\\\/g; s/"/\\"/g')"; steps+=("{\"name\":\"$1\",\"ok\":$2,\"detail\":\"${d}\"}"); if [ "$2" = true ]; then echo "ok   $1"; else echo "FAIL $1: ${d}"; fi; }
ctl() { "${CTL[@]}" "$@" 2>&1; }
runb() { ctl ctr -- run-binds "$@"; }
ctl share list | grep -q " ${SHARE} mode=" || { echo "microvm-shares-suite: ${SHARE} is not a share; run 'make systemvm-up-shares'" >&2; exit 2; }
ctl ctr -- import >/dev/null

rm -rf "${F}"; mkdir -p "${F}/proj/conf" "${F}/lw"
echo hello > "${F}/proj/file.txt"; echo conf > "${F}/proj/conf/app.conf"; echo single > "${F}/proj/single.txt"
B=(--bind="${F}/proj:/data:rw" --bind="${F}/proj/conf:/data/conf:ro")

# --- 1. Binds from a share are NFS mounts in the guest ------------------------------------------------------------
out="$(runb "${B[@]}" shares-a 'cat /data/file.txt; echo from-guest > /data/new.txt; echo x > /data/conf/x 2>/dev/null && echo RO-WRITTEN || echo ro-refused; grep -c " nfs " /proc/mounts')"
case "${out}" in "RC 0"*"OUT hello"*"OUT ro-refused"*"OUT 2"*) ;; *) out="BAD ${out}" ;; esac
if [ "$(cat "${F}/proj/new.txt" 2>/dev/null)" = from-guest ] && [ ! -e "${F}/proj/conf/x" ] && case "${out}" in BAD*) false ;; *) true ;; esac; then
  record binds_become_nfs_mounts true "${out}; macOS sees new.txt=from-guest, conf/x absent"
else
  record binds_become_nfs_mounts false "${out}; new.txt=$(cat "${F}/proj/new.txt" 2>&1)"
fi

# --- 2. A macOS write appears in the guest (actimeo=1) ------------------------------------------------------------
rm -f "${F}/proj/from-mac.txt"
( runb "${B[@]}" shares-b 'for i in $(seq 1 100); do [ "$(cat /data/from-mac.txt 2>/dev/null)" = mac-wrote ] && { echo seen-after-tenths $i; exit 0; }; sleep 0.1; done; echo not-seen; exit 1' > "${F}.b.out" 2>&1 ) &
bp=$!
sleep 4; echo mac-wrote > "${F}/proj/from-mac.txt"
wait "${bp}"; out="$(tr '\n' ' ' < "${F}.b.out")"; rm -f "${F}.b.out"
case "${out}" in "RC 0"*"seen-after-tenths"*) record macos_write_seen_in_guest true "${out}" ;; *) record macos_write_seen_in_guest false "${out}" ;; esac

# --- 3. The GH #159 lost write does not recur ------------------------------------------------------------------------
out="$(runb "${B[@]}" shares-c "bad=0; i=1; while [ \$i -le ${TRIES} ]; do f=/data/lw-\$i; echo a\$i > \$f; echo b\$i >> \$f; [ \"\$(sed -n 1p \$f)\" = a\$i ] && [ \"\$(sed -n 2p \$f)\" = b\$i ] && [ \$(wc -l < \$f) = 2 ] || bad=\$((bad+1)); i=\$((i+1)); done; echo guest-bad \$bad")"
macbad=0; for i in $(seq 1 "${TRIES}"); do [ "$(cat "${F}/proj/lw-${i}" 2>/dev/null)" = "$(printf 'a%s\nb%s' "${i}" "${i}")" ] || macbad=$((macbad + 1)); done
case "${out}" in "RC 0"*"guest-bad 0"*) [ "${macbad}" = 0 ] && record lost_write_does_not_recur true "${TRIES} tries: ${out}; macOS bad ${macbad}" || record lost_write_does_not_recur false "${out}; macOS bad ${macbad}" ;;
  *) record lost_write_does_not_recur false "${out}; macOS bad ${macbad}" ;; esac
rm -f "${F}"/proj/lw-*

# --- 4. A file source is refused; a bind outside every share keeps today's behaviour --------------------------------
out1="$(runb --bind="${F}/proj/single.txt:/single.txt:ro" shares-d 'echo ran')"
out2="$(runb --bind=/etc:/hostetc:ro shares-e 'echo ran; grep -c " nfs " /proc/mounts || true')"
case "${out1}|${out2}" in *"is a file"*"|RC 0"*"OUT ran"*"OUT 0"*) record file_refused_other_binds_unchanged true "file: ${out1}; outside a share: ${out2}" ;;
  *) record file_refused_other_binds_unchanged false "file: ${out1}; outside a share: ${out2}" ;; esac

# --- 5. A non-root guest user writes through the mount ------------------------------------------------------------
chmod 0777 "${F}/proj"
out="$(runb "${B[@]}" --user=1000:1000 shares-f 'id -u; echo by-1000 > /data/by-1000.txt && ls -ln /data/by-1000.txt; cat /data/file.txt')"
case "${out}" in "RC 0"*"OUT 1000"*" 1000 "*"OUT hello"*) [ "$(cat "${F}/proj/by-1000.txt" 2>/dev/null)" = by-1000 ] && record non_root_guest_user_writes true "${out}" || record non_root_guest_user_writes false "${out}; macOS: $(cat "${F}/proj/by-1000.txt" 2>&1)" ;;
  *) record non_root_guest_user_writes false "${out}" ;; esac

# --- 5b. The server stops when the task stops, before Delete (GH #160 M5) -----------------------------------------
st="$(runb "${B[@]}" --detach shares-g 'echo quick > /data/quick.txt')"
for _ in $(seq 1 60); do case "$(ctl ctr -- state shares-g)" in *STOPPED*) break ;; esac; sleep 0.5; done
sleep 2; srvg="$(ctl microvm unfs shares-g | head -1)"; ctl ctr -- delete shares-g >/dev/null
case "${st}:${srvg}" in "STARTED shares-g:UNFS shares-g absent") record server_stops_with_the_task true "${st}; after the task stopped, before delete: ${srvg}" ;;
  *) record server_stops_with_the_task false "${st}; ${srvg}" ;; esac

# --- 5c. SIGKILL ends a task stuck on a dead server: the shim stops the microVM after its grace -----------------
st="$(runb "${B[@]}" --detach shares-k 'while true; do echo x >> /data/k.txt; sleep 0.2; done')"
sleep 4
down="$(ctl microvm unfs-stop shares-k)"   # the guest's next write, and its unmount, now wait on a dead server
sleep 2
t0=$(date +%s); killed="$(ctl ctr -- kill shares-k 60)"; t1=$(date +%s)
ctl ctr -- delete shares-k >/dev/null
case "${st}|${down}|${killed}" in "STARTED shares-k|UNFS shares-k stopped|KILLED shares-k") record sigkill_bounded_on_dead_server true "${down}; ${killed} after $((t1 - t0)) s (shim grace 10 s)" ;;
  *) record sigkill_bounded_on_dead_server false "${st}; ${down}; ${killed} after $((t1 - t0)) s" ;; esac

# --- 6. Nothing is left -------------------------------------------------------------------------------------------
left="$(ctl ctr -- leftovers)"; srv=""; gone=true
for n in shares-a shares-b shares-c shares-d shares-e shares-f shares-g shares-k; do
  u="$(ctl microvm unfs "${n}" | head -1)"; srv="${srv} ${u};"
  [ "${u}" = "UNFS ${n} absent" ] || gone=false   # only the documented answer counts, not an error or nothing
done
case "${left}" in *CLEAN*) record nothing_left "${gone}" "${left};${srv}" ;; *) record nothing_left false "${left};${srv}" ;; esac

passed=true; for s in "${steps[@]}"; do case "$s" in *'"ok":false'*) passed=false ;; esac; done
printf '{"steps":[%s],"passed":%s}\n' "$(IFS=,; echo "${steps[*]}")" "${passed}" > "${REPORT}"
echo "microvm-shares suite: passed=${passed} (report: ${REPORT})"
[ "${passed}" = true ]
