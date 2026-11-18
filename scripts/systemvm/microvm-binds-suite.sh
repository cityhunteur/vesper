#!/usr/bin/env bash
# GH #160 M7 / ADR 0009: the binds-suite failure tests against the microVM path (containerd tasks on the Vesper shim
# whose bind mounts come from a macOS share as NFS mounts from the microVM's own server).
#
# Steps: 32 MiB written then SIGKILL keeps every byte; a stalled server fails the RUN quickly with an error naming
# the share; the server killed under load comes back and the writes all land; 0600 files and 0700 directories read
# and write; at least 40 directories below an export read, a write through a descriptor opened before a rename
# across directories keeps today's recorded outcome, and three more rename cases are measured; a share source whose
# path holds spaces and shell characters mounts and reads; binds from two shares share one server and stay apart;
# writes land when one inode is reachable read-only and read-write (TD-072); single-file binds (GH #208); a bound
# file that macOS replaces is bound again within seconds (D-052);
# after the shim is killed the reaper leaves no server,
# namespace or record. Needs `make systemvm-up-shares`.
# Report: run/systemvm/microvm-binds-report.json.
set -uo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SHARE="${BINDS_SHARE:-${ROOT_DIR}/run/WOR-39/share}"
REPORT="${ROOT_DIR}/run/systemvm/microvm-binds-report.json"
CTL=(cargo run -q --bin systemvmctl -- control)
# A new directory each run: a directory deleted on macOS and made again under the same name seconds later can still
# be the deleted one in the System VM, and a microVM's writes then land nowhere (TD-071).
F="${SHARE}/m7-$(date +%s)"
cd "${ROOT_DIR}"; mkdir -p "$(dirname "${REPORT}")"
steps=()
record() { local d; d="$(printf '%s' "$3" | tr '\n' ' ' | cut -c1-900 | sed 's/\\/\\\\/g; s/"/\\"/g')"; steps+=("{\"name\":\"$1\",\"ok\":$2,\"detail\":\"${d}\"}"); if [ "$2" = true ]; then echo "ok   $1"; else echo "FAIL $1: ${d}"; fi; }
ctl() { "${CTL[@]}" "$@" 2>&1; }
runb() { ctl ctr -- run-binds "$@"; }
wait_stopped() { for _ in $(seq 1 "${2:-60}"); do case "$(ctl ctr -- state "$1")" in *STOPPED*|*ABSENT*) return 0 ;; esac; sleep 1; done; return 1; }
gone() { ctl ctr -- kill "$1" 30 >/dev/null; ctl ctr -- delete "$1" >/dev/null; }
ctl share list | grep -q " ${SHARE} mode=" || { echo "microvm-binds-suite: ${SHARE} is not a share; run 'make systemvm-up-shares'" >&2; exit 2; }
ctl ctr -- import >/dev/null
rm -rf "${SHARE}"/m7 "${SHARE}"/m7-*; mkdir -p "${F}/w" "${F}/load"
B=(--bind="${F}/w:/data:rw")

# --- 1. 32 MiB written without fsync, then SIGKILL: macOS has every byte ------------------------------------------
st="$(runb "${B[@]}" --detach binds-dd 'head -c 33554432 /dev/urandom | tee /data/big | sha256sum | cut -d" " -f1 > /dev/shm/sum 2>/dev/null || sha256sum /data/big | cut -d" " -f1 > /dev/shm/sum; cp /dev/shm/sum /data/big.sum; sleep 600')"
for _ in $(seq 1 60); do [ -s "${F}/w/big.sum" ] && break; sleep 1; done
k="$(ctl ctr -- kill binds-dd 30)"; ctl ctr -- delete binds-dd >/dev/null
# The guest flushed everything to the server before EXIT; the System VM's virtio-fs write-back reaches macOS a
# moment later, so wait for the size and say how long it took.
# (macOS stat can report an older size for a while after the content is all there, so count the bytes.)
lag=0; for _ in $(seq 1 30); do [ "$(wc -c < "${F}/w/big" 2>/dev/null | tr -d ' ')" = 33554432 ] && break; sleep 0.5; lag=$((lag + 1)); done
size="$(wc -c < "${F}/w/big" 2>/dev/null | tr -d ' ')"; want="$(cat "${F}/w/big.sum" 2>/dev/null)"; got="$(shasum -a 256 "${F}/w/big" 2>/dev/null | cut -d' ' -f1)"
[ "${size}" = 33554432 ] && [ -n "${want}" ] && [ "${want}" = "${got}" ] && record write_then_kill_keeps_every_byte true "${st}; ${k}; ${size} bytes on macOS $((lag / 2)) s after the task stopped, sha256 matches the guest's" \
  || record write_then_kill_keeps_every_byte false "${st}; ${k}; size ${size:-none}; guest sum ${want:-none}; macOS sum ${got:-none}"

# --- 2. A stalled server at session start fails the RUN quickly, naming the share ---------------------------------
s="$(ctl microvm test-unfs-stall-next)"
t0=$(date +%s); out="$(runb "${B[@]}" binds-stall 'echo should-not-run')"; t1=$(date +%s)
ctl ctr -- delete binds-stall >/dev/null 2>&1
case "${out}" in *"did not answer the mount of /s/0"*) ok=true ;; *) ok=false ;; esac
[ "${ok}" = true ] && [ $((t1 - t0)) -le 30 ] && record stalled_server_fails_the_run true "${s}; failed in $((t1 - t0)) s (microVM boot included; mount bound 5 s): $(printf '%s' "${out}" | tail -2)" \
  || record stalled_server_fails_the_run false "${s}; $((t1 - t0)) s: ${out}"

# --- 3. The server killed under load comes back, and every write lands ---------------------------------------------
st="$(runb --bind="${F}/load:/data:rw" --detach binds-load 'i=1; while [ $i -le 200 ]; do dd if=/dev/zero of=/data/f$i bs=64k count=1 2>/dev/null || echo fail $i >> /tmp/fails; i=$((i+1)); done; echo finished > /data/done; sleep 600')"
sleep 6; k1="$(ctl microvm test-unfs-kill binds-load)"
for _ in $(seq 1 90); do [ -s "${F}/load/done" ] && break; sleep 1; done
n="$(find "${F}/load" -name 'f*' -size 65536c | wc -l | tr -d ' ')"
gone binds-load
[ -s "${F}/load/done" ] && [ "${n}" = 200 ] && case "${k1}" in KILLED*) true ;; *) false ;; esac \
  && record server_respawn_under_load true "${k1} during the writes; 200 of 200 files of 64 KiB on macOS" \
  || record server_respawn_under_load false "${st}; ${k1}; done=$(cat "${F}/load/done" 2>/dev/null); ${n} of 200 files"

# --- 4. 0600 files and 0700 directories read and write -------------------------------------------------------------
echo secret > "${F}/w/s600"; chmod 600 "${F}/w/s600"; mkdir -p "${F}/w/d700"; echo inner > "${F}/w/d700/in"; chmod 700 "${F}/w/d700"
out="$(runb "${B[@]}" binds-modes 'cat /data/s600 /data/d700/in; echo from-guest > /data/d700/new && echo wrote; echo more >> /data/s600 && echo appended')"
case "${out}" in "RC 0"*"OUT secret"*"OUT inner"*"OUT wrote"*"OUT appended"*) [ "$(cat "${F}/w/d700/new")" = from-guest ] && record modes_0600_0700_read_and_write true "${out}" || record modes_0600_0700_read_and_write false "${out}; macOS new: $(cat "${F}/w/d700/new" 2>&1)" ;;
  *) record modes_0600_0700_read_and_write false "${out}" ;; esac

# --- 5. Deep paths, and descriptors that outlive a rename ----------------------------------------------------------
# unfs3 keeps a file's whole directory path in its 64-byte handle, so it serves a limited depth below an export
# (TD-067, a recorded deviation; the GH #160 criterion asked for 60). The step measures the limit, counted from the
# export (/data), and fails if it shrinks below 40. Writes must keep a file's data (TD-068, fixed by the patched
# unfs3, D-044: an O_CREAT open of an existing file the guest had not looked up used to truncate it): an append with
# no rename to a file not looked up first, the same after a read, through a descriptor opened before a rename across
# directories, in the same directory (logrotate), and on macOS, and a write at an offset; any loss fails the step.
deep="${F}/w/deep"; p="${deep}"; for i in $(seq 1 60); do p="${p}/d${i}"; done
mkdir -p "${p}" "${F}/w/ra" "${F}/w/rb" "${F}/w/rc" "${F}/w/rd"
echo start > "${F}/w/ra/f"; echo start > "${F}/w/ra/log"; echo start > "${F}/w/ra/plain"; echo start > "${F}/w/ra/seen"; printf 'xxxxxxxx' > "${F}/w/ra/w"; echo start > "${F}/w/rc/h"
out="$(runb "${B[@]}" binds-deep 'p=/data/deep; d=1; while [ $d -le 61 ]; do q=$p; [ $d -gt 1 ] && q=$p/d$((d-1)); ls $q >/dev/null 2>&1 || break; p=$q; d=$((d+1)); done; echo depth $((d-1)); exec 6>> /data/ra/plain; echo late >&6; exec 6>&-; cat /data/ra/seen >/dev/null; exec 6>> /data/ra/seen; echo late >&6; exec 6>&-; exec 3>> /data/ra/f; mv /data/ra/f /data/rb/f; echo late >&3 && echo late-ok; exec 3>&-; exec 4>> /data/ra/log; mv /data/ra/log /data/ra/log.1; echo late >&4 && echo log-ok; exec 4>&-; exec 5<> /data/ra/w; printf AB >&5; mv /data/ra/w /data/rb/w; printf CD >&5 && echo w-ok; exec 5>&-; sync')"
st="$(runb "${B[@]}" --detach binds-hostmv 'exec 3>> /data/rc/h; echo ready > /data/rc/ready; i=0; while [ ! -e /data/rc/go ] && [ $i -lt 300 ]; do sleep 0.1; i=$((i+1)); done; echo late >&3 && echo ok > /data/rc/wrote; exec 3>&-; sync; echo done > /data/rc/done; sleep 600')"
for _ in $(seq 1 60); do [ -s "${F}/w/rc/ready" ] && break; sleep 0.5; done
mv "${F}/w/rc/h" "${F}/w/rd/h"; touch "${F}/w/rc/go"
for _ in $(seq 1 60); do [ -s "${F}/w/rc/done" ] && break; sleep 0.5; done
gone binds-hostmv
depth="$(printf '%s\n' "${out}" | sed -n 's/^OUT depth //p')"
show() { if [ -e "$1" ]; then od -An -c "$1" | tr -s ' ' | tr '\n' ' '; else printf missing; fi; }
keeps() { [ "$(cat "$1" 2>/dev/null)" = "start
late" ]; }
rename=""
keeps "${F}/w/rb/f" && keeps "${F}/w/ra/plain" && keeps "${F}/w/ra/seen" && keeps "${F}/w/ra/log.1" && keeps "${F}/w/rd/h" \
  && [ "$(cat "${F}/w/rb/w" 2>/dev/null)" = ABCDxxxx ] && rename="every write kept the file's data (TD-068 fixed, D-044)"
facts="append, no rename, file not looked up first: $(show "${F}/w/ra/plain"); looked up first: $(show "${F}/w/ra/seen"); same-dir log.1: $(show "${F}/w/ra/log.1"), log: $(show "${F}/w/ra/log"); offset write rb/w: $(show "${F}/w/rb/w"); macOS rename rd/h: $(show "${F}/w/rd/h"), rc/h: $(show "${F}/w/rc/h"), guest wrote: $(cat "${F}/w/rc/wrote" 2>/dev/null || echo no)"
case "${out}" in *"OUT late-ok"*) late=true ;; *) late=false ;; esac
if [ -n "${depth}" ] && [ "${depth}" -ge 40 ] && [ -n "${rename}" ] && [ "${late}" = true ] && [ ! -e "${F}/w/ra/f" ]; then
  record deep_path_and_rename true "${depth} directories below the export read (TD-067); cross-directory rename: ${rename}; measured: ${facts}"
else
  record deep_path_and_rename false "depth ${depth:-?}; cross-directory rb/f: $(show "${F}/w/rb/f"); ra/f: $(show "${F}/w/ra/f"); ${facts}; ${out}; ${st}"
fi

# --- 6. A source path with spaces and shell characters ------------------------------------------------------------
odd="${F}/w/sp ace;\$(id)&x"; mkdir -p "${odd}"; echo odd > "${odd}/f"
b64="$(printf '%s' "${odd}" | base64 | tr -d '\n')"
out="$(runb --bind64="${b64}:/odd:rw" binds-odd 'cat /odd/f; echo back > /odd/g && echo wrote')"
case "${out}" in "RC 0"*"OUT odd"*"OUT wrote"*) [ "$(cat "${odd}/g" 2>/dev/null)" = back ] && record odd_source_path true "${out}" || record odd_source_path false "${out}; g: $(cat "${odd}/g" 2>&1)" ;;
  *) record odd_source_path false "${out}" ;; esac

# --- 6b. Binds from two shares (GH #207): one server; the same relative path in each share stays two directories,
# each write lands in its own share, and a read-only bind inside the second share stays read-only ------------------
SHARE2="${BINDS_SHARE2:-$(dirname "${SHARE}")/share-nowatch}"
R="$(basename "${F}")/two"; A="${SHARE}/${R}"; Z="${SHARE2}/${R}"
rm -rf "${SHARE2}"/m7-*; mkdir -p "${A}" "${Z}/sub"; echo first > "${A}/id"; echo second > "${Z}/id"
out="$(runb --bind="${A}:/a:rw" --bind="${Z}:/b:rw" --bind="${Z}/sub:/b/sub:ro" binds-two 'cat /a/id /b/id; echo from-a > /a/w; echo from-b > /b/w; { echo x > /b/sub/no; } 2>/dev/null && echo RO-WRITTEN || echo ro-refused; grep -c " nfs " /proc/mounts')"
case "${out}" in "RC 0"*"OUT first"*"OUT second"*"OUT ro-refused"*"OUT 3"*)
  if [ "$(cat "${A}/w" 2>/dev/null)" = from-a ] && [ "$(cat "${Z}/w" 2>/dev/null)" = from-b ] && [ ! -e "${Z}/sub/no" ] && [ ! -e "${A}/sub" ]; then
    record binds_from_two_shares true "${out}; macOS: ${A}/w=from-a, ${Z}/w=from-b, sub/no absent"
  else record binds_from_two_shares false "${out}; a/w=$(cat "${A}/w" 2>&1) b/w=$(cat "${Z}/w" 2>&1)"; fi ;;
  *) record binds_from_two_shares false "${out}" ;; esac

# --- 6c. One inode through a read-only and a read-write export: writes through the read-write one land (TD-072,
# unfs3 patch 0004). Before the patch a read through the read-only export made later writes through the other fail
# with ROFS, silently for a shell redirection ------------------------------------------------------------------------
T="${F}/t72"; mkdir -p "${T}/proj/out"; echo w > "${T}/proj/out/w1"; echo p > "${T}/proj/p.txt"
out="$(runb --bind="${T}/proj:/app:ro" --bind="${T}/proj/out:/else:rw" --bind="${T}/proj/p.txt:/x.txt:rw" binds-t72 'cat /app/out/w1 /app/p.txt >/dev/null; sleep 2; ls -l /app /app/out >/dev/null; cat /app/out/w1 /app/p.txt >/dev/null; echo b >> /else/w1; echo b >> /x.txt; sync; { echo z > /app/p.txt; } 2>/dev/null && echo RO-WRITTEN || echo ro-refused')"
w1="$(tr '\n' ' ' < "${T}/proj/out/w1")"; pt="$(tr '\n' ' ' < "${T}/proj/p.txt")"
case "${out}|${w1}|${pt}" in "RC 0"*"OUT ro-refused"*"|w b |p b ") record writes_land_through_either_export true "${out}; macOS: out/w1='${w1}' p.txt='${pt}'" ;;
  *) record writes_land_through_either_export false "${out}; macOS: out/w1='${w1}' p.txt='${pt}' (want 'w b' and 'p b')" ;; esac

# --- 6d. Single-file binds (GH #208, D-048): read and write; a read-only file bind and a read-only file nested in a
# read-write directory refuse writes; one file bound both ways keeps its writes; the export's directory refuses new
# entries, renames and removal from a hostile task; a file onto a directory is refused; a macOS edit reaches a
# watcher (the next step replaces files on macOS) --------------------------------------------------------------------
G="${F}/g208"; mkdir -p "${G}/dir"; echo one > "${G}/a.txt"; echo conf > "${G}/dir/conf.txt"; echo w > "${G}/w.txt"
nfsopt='addr=169.254.77.1,vers=3,proto=tcp,port=2049,mountport=2050,mountproto=tcp,nolock'
out="$(runb --bind="${G}/a.txt:/etc/a.txt:rw" --bind="${G}/dir:/app:rw" --bind="${G}/dir/conf.txt:/app/conf.txt:ro" --bind="${G}/a.txt:/ro/a.txt:ro" binds-file "cat /etc/a.txt; echo two >> /etc/a.txt; { echo x > /app/conf.txt; } 2>/dev/null && echo NEST-WRITTEN || echo nest-ro-refused; { echo y > /ro/a.txt; } 2>/dev/null && echo RO-WRITTEN || echo ro-refused; mv /etc/a.txt /etc/b.txt 2>/dev/null && echo MOVED || echo rename-busy; mkdir -p /mnt/x; mount -t nfs -o ${nfsopt} 169.254.77.1:/s/0 /mnt/x && echo export-entries \$(ls /mnt/x); touch /mnt/x/sib 2>/dev/null && echo SIBLING || echo sibling-refused; rm /mnt/x/f 2>/dev/null && echo REMOVED || echo remove-refused")"
dirdest="$(runb --bind="${G}/w.txt:/etc:rw" binds-file-dir 'echo ran')"
case "${out}|${dirdest}" in "RC 0"*"OUT one"*"OUT nest-ro-refused"*"OUT ro-refused"*"OUT rename-busy"*"OUT export-entries f"*"OUT sibling-refused"*"OUT remove-refused"*"|"*"is a directory; a file can only be bound onto a file"*)
  if [ "$(tr '\n' ' ' < "${G}/a.txt")" = "one two " ] && [ "$(cat "${G}/dir/conf.txt")" = conf ]; then
    record file_binds true "${out}; macOS a.txt='one two', conf.txt unchanged; onto a directory: ${dirdest}"
  else record file_binds false "${out}; macOS a.txt='$(tr '\n' ' ' < "${G}/a.txt")'"; fi ;;
  *) record file_binds false "${out}; onto a directory: ${dirdest}" ;; esac
# --- 6e. A file that macOS replaces is bound again (D-052). An editor's save, `git checkout` and `sed -i` rename a
# new file over the old one. The share server attaches the new file and the guest binds it again: a file bound both
# read-write and read-only, a read-only file inside a read-write directory, and a read-only file on a share that
# sends no change events. The task reads them twice a second, with its uptime. Before the replacement an edit in
# place reaches a watcher. Afterwards every bind shows the new content within 8 s by the guest's clock and to the
# end; each of the four binds was made again once or twice, not at every notice; the read-write bind still writes,
# and that write is in the new file on macOS; the nested file refuses a write at the guest's bind and, with that
# bind removed, through the outer export too, where only the server's own read-only nest stands ------------------
mkdir -p "${G}/log" "${G}/rdir" "${SHARE2}/$(basename "${F}")"; N="${SHARE2}/$(basename "${F}")/n.txt"
echo v1 > "${G}/e.txt"; echo r1 > "${G}/rdir/conf.txt"; echo n1 > "${N}"
st="$(runb --bind="${G}/e.txt:/e.txt:rw" --bind="${G}/e.txt:/ro/e.txt:ro" --bind="${G}/rdir:/app:rw" --bind="${G}/rdir/conf.txt:/app/conf.txt:ro" --bind="${N}:/n.txt:ro" --bind="${G}/log:/log:rw" --detach binds-file-ev 'inotifyd - /e.txt:c > /log/ev 2>&1 & sleep 9; cat /e.txt > /log/before 2>&1; i=0; while [ $i -lt 44 ]; do i=$((i+1)); echo "$(cut -d" " -f1 /proc/uptime) e=$(cat /e.txt 2>&1 | tail -1) ro=$(cat /ro/e.txt 2>&1 | tail -1) conf=$(cat /app/conf.txt 2>&1 | tail -1) n=$(cat /n.txt 2>&1 | tail -1)" > /log/s.$i; sleep 0.5; done; { echo x >> /app/conf.txt; } 2>/dev/null && echo NEST-WRITTEN > /log/nest || echo nest-refused > /log/nest; { echo y >> /ro/e.txt; } 2>/dev/null && echo RO-WRITTEN >> /log/nest || echo ro-refused >> /log/nest; echo guest >> /e.txt; sync; if umount /app/conf.txt; then { echo outer >> /app/conf.txt; sync; } 2>/dev/null; echo outer-tried >> /log/nest; else echo guest-bind-not-removed >> /log/nest; fi; kill %1')"
sleep 5; echo v2 >> "${G}/e.txt"; sleep 8
at="$(ls "${G}/log" | grep -c '^s\.')"   # the samples taken before the replacement
for f in "${G}/e.txt:v3" "${G}/rdir/conf.txt:r2" "${N}:n2"; do p="${f%%:*}"; printf '%s\n' "${f##*:}" > "${p}.tmp" && mv "${p}.tmp" "${p}"; done
wait_stopped binds-file-ev 70 || ctl ctr -- kill binds-file-ev 30 >/dev/null
# Each of the four destinations made again: counted by name, so one bind made twice does not stand in for another.
rebound="$(ctl microvm log binds-file-ev 300 | tr -d '\r' | grep -a 'VESPER-SHARE-REBOUND' | sort | uniq -c | awk '{n++; t += $1; d = d " " $3 "x" $1} END {printf "%d of 4 destinations, %d times:%s", n, t, d}')"; ctl ctr -- delete binds-file-ev >/dev/null
for _ in $(seq 1 20); do grep -q guest "${G}/e.txt" 2>/dev/null && break; sleep 0.5; done   # the write-back reaches macOS a moment later
ev="$(grep -c e.txt "${G}/log/ev" 2>/dev/null)"; before="$(tr '\n' ' ' < "${G}/log/before" 2>/dev/null)"; nest="$(tr '\n' ' ' < "${G}/log/nest" 2>/dev/null)"
want="e=v3 ro=v3 conf=r2 n=n2"; first=""
for i in $(seq 1 44); do [ "$(cut -d' ' -f2- "${G}/log/s.${i}" 2>/dev/null)" = "${want}" ] && { first="${i}"; break; }; done
last="$(cut -d' ' -f2- "${G}/log/s.44" 2>/dev/null)"; mac="$(tr '\n' ' ' < "${G}/e.txt")|$(tr '\n' ' ' < "${G}/rdir/conf.txt")|$(tr '\n' ' ' < "${N}")"
# Seconds on the guest's clock from the last sample before the replacement to the first with all four new.
secs="$(awk -v a="$(cut -d' ' -f1 "${G}/log/s.${at}" 2>/dev/null)" -v b="$(cut -d' ' -f1 "${G}/log/s.${first:-0}" 2>/dev/null)" 'BEGIN { if (a > 0 && b >= a) printf "%d", b - a + 0.999; else print 99 }')"
if [ "${st}" = "STARTED binds-file-ev" ] && [ "${before}" = "v1 v2 " ] && [ "${ev:-0}" -ge 1 ] && [ -n "${first}" ] && [ "${secs}" -le 8 ] \
  && [ "${last}" = "${want}" ] && [ "${nest}" = "nest-refused ro-refused outer-tried " ] && [ "${mac}" = "v3 guest |r2 |n2 " ] \
  && case "${rebound}" in "4 of 4 destinations, "[4-8]" times:"*) true ;; *) false ;; esac; then
  record file_bind_follows_a_replaced_file true "edit in place seen: '${before}', ${ev} IN_MODIFY; all four binds new within ${secs} s of the replacement and to the end; ${nest}; binds made again: ${rebound}; macOS: ${mac}"
else
  record file_bind_follows_a_replaced_file false "${st}; before '${before}'; events ${ev:-none}; replaced after sample ${at}, first all-new sample '${first:-none}' (${secs} s), last '${last}'; ${nest:-no nest result}; binds made again: ${rebound:-none}; macOS: ${mac}; s.$((at + 4)): $(cat "${G}/log/s.$((at + 4))" 2>/dev/null)"
fi

# --- 6f. A task cannot use the re-attach to write a file bound read-only (D-052). With a read-write directory bind
# the task renames directories so that the path of its read-write file bind names another file, which a second bind
# made read-only. The server sees another file at the path and attaches it, taken through the directory's export,
# where the read-only nest covers it: the clone is read-only. The file on macOS keeps its content ----------------
H="${F}/h52"; mkdir -p "${H}/x" "${H}/y"; echo mine > "${H}/x/secret"; echo protected > "${H}/y/secret"
st="$(runb --bind="${H}:/d:rw" --bind="${H}/x/secret:/a:rw" --bind="${H}/y/secret:/b:ro" --detach binds-swap 'mv /d/x /d/x2 && mv /d/y /d/x && echo renamed > /d/state; sleep 8; { echo evil >> /a; sync; } 2>/dev/null; { echo evil >> /d/x/secret; sync; } 2>/dev/null; cat /a > /d/a-now 2>&1; sleep 2')"
wait_stopped binds-swap 60 || ctl ctr -- kill binds-swap 30 >/dev/null
slog="$(ctl microvm unfs-log binds-swap 60 | grep -a 'another file is at its path' | tr '\n' ' ')"; ctl ctr -- delete binds-swap >/dev/null
sleep 2; prot="$(tr '\n' ' ' < "${H}/x/secret" 2>/dev/null)"; mine="$(tr '\n' ' ' < "${H}/x2/secret" 2>/dev/null)"; anow="$(tr '\n' ' ' < "${H}/a-now" 2>/dev/null)"
case "${st}|$(cat "${H}/state" 2>/dev/null)|${prot}|${slog}" in
  "STARTED binds-swap|renamed|protected |"*"x/secret"*"taken through the export"*"stays read-only"*)
    record renamed_into_place_keeps_read_only true "after the renames the server attached the file at the path read-only (${slog}); macOS: x/secret='${prot}' x2/secret='${mine}'; the task's /a then read '${anow}'" ;;
  *) record renamed_into_place_keeps_read_only false "${st}; state '$(cat "${H}/state" 2>/dev/null)'; macOS x/secret='${prot}' (want 'protected') x2/secret='${mine}'; /a read '${anow}'; server log: ${slog:-no attach logged}" ;;
esac

# --- 6g. A file replaced after the server came back is bound again too (D-052): the export of a file is a small
# file system that each new sandbox makes anew, and the guest must still be able to look the file up in it -----------
K="${F}/k52"; mkdir -p "${K}/log"; echo k1 > "${K}/k.txt"
st="$(runb --bind="${K}/k.txt:/k.txt:rw" --bind="${K}/log:/log:rw" --detach binds-krep 'i=0; while [ $i -lt 40 ]; do i=$((i+1)); echo "k=$(cat /k.txt 2>&1 | tail -1)" > /log/s.$i; sleep 0.5; done')"
sleep 4; killed="$(ctl microvm test-unfs-kill binds-krep | head -1)"; sleep 5
printf 'k2\n' > "${K}/k.tmp" && mv "${K}/k.tmp" "${K}/k.txt"
wait_stopped binds-krep 60 || ctl ctr -- kill binds-krep 30 >/dev/null
ctl ctr -- delete binds-krep >/dev/null
last="$(cat "${K}/log/s.40" 2>/dev/null)"; newc="$(grep -l 'k=k2' "${K}"/log/s.* 2>/dev/null | wc -l | tr -d ' ')"
case "${st}|${last}" in "STARTED binds-krep|k=k2") record replaced_after_a_server_restart true "${killed}; then replaced: ${newc} samples read the new file, the last '${last}'" ;;
  *) record replaced_after_a_server_restart false "${st}; ${killed}; last sample '${last}'; ${newc} samples read the new file" ;; esac

# --- 7. After the shim is killed, the reaper leaves no server, namespace or record ----------------------------------
st="$(runb "${B[@]}" --detach binds-shim 'sleep 600')"
sleep 4; ks="$(ctl ctr -- kill-shim binds-shim)"
clean=""; for _ in $(seq 1 30); do l="$(ctl ctr -- leftovers)"; case "${l}" in *shareservers=*|*sharecgroups=*|*sharenets=*|*sharerecords=*|*firecracker=*) sleep 5 ;; *) clean="${l}"; break ;; esac; done
ctl ctr -- delete binds-shim >/dev/null 2>&1; ctl ctr -- try container delete binds-shim >/dev/null 2>&1
[ -n "${clean}" ] && record reaper_clears_after_shim_killed true "${st}; ${ks}; then: ${clean} (task and container entries are containerd's, removed by delete)" \
  || record reaper_clears_after_shim_killed false "${st}; ${ks}; still: ${l}"

passed=true; for s in "${steps[@]}"; do case "$s" in *'"ok":false'*) passed=false ;; esac; done
printf '{"steps":[%s],"passed":%s}\n' "$(IFS=,; echo "${steps[*]}")" "${passed}" > "${REPORT}"
echo "microvm-binds suite: passed=${passed} (report: ${REPORT})"
[ "${passed}" = true ]
