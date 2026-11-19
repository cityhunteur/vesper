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
# file that macOS replaces is bound again within seconds (D-052); what is bound read-only stays read-only under
# another letter case, after a move into a second read-write bind and after a server restart (TD-073);
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
# no_anchors ID: the next server of that task starts without anchors (TD-080). Some steps are about the layers
# beneath that rule, and their task renames a directory on the way to a read-only bind. The order is for one task
# by name, the server's log says that it started so, and it is taken back when the suite ends.
no_anchors() { ctl microvm test-unfs-no-anchors-next "$1" >/dev/null; }
trap 'ctl microvm test-unfs-no-anchors-next none >/dev/null 2>&1' EXIT
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
no_anchors binds-swap
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

# --- 6h. What is bound read-only stays read-only under every name (TD-073, D-053, unfs3 patch 0006). The server
# knows a read-only source by its device and inode number and refuses a request that changes it, or anything under
# it, with every symbolic link in the path resolved.
# (1) Another letter case: a macOS volume ignores case and the System VM does not, so SECRET is a second name for
#     secret with no read-only mount on it. The task appends, truncates, removes and renames through it, and creates
#     and removes in a read-only directory through CONF. (On a volume that keeps case the suite makes SECRET a hard
#     link and CONF a symbolic link, which reach the same objects.)
# (2) Links: a hard link macOS made to the file; a hard link the task tries to make out of the read-only directory,
#     also from a symbolic link to a file in it; symbolic links the task makes to the directory and to the file,
#     with a change of the file's times through the link's own handle (`touch -h`); a rename onto the file.
# (3) A move: the task moves the file's directory, and the directory above a read-only directory, into a second
#     read-write bind inside the first, and tries there.
# (4) A restart: the task renames directories so that its read-write file bind's path names the read-only file, the
#     server is killed and starts again from the paths, and the task then writes through that bind.
# A read-write bind inside a read-only one, and the outer bind itself, still write ------------------------------
I="${F}/i73"; mkdir -p "${I}/y" "${I}/conf" "${I}/work" "${I}/top/conf2" "${I}/rod/out" "${I}/hx" "${I}/hy"
echo protected > "${I}/y/secret"; echo keep > "${I}/conf/keep"; echo keep2 > "${I}/top/conf2/keep"
echo mine > "${I}/hx/secret"; echo guarded > "${I}/hy/secret"; ln "${I}/y/secret" "${I}/y/hard"; touch -t 202001010000 "${I}/y/secret"; mt0="$(stat -f %m "${I}/y/secret")"
insensitive=yes; [ -e "${I}/y/SECRET" ] || { insensitive=no; ln "${I}/y/secret" "${I}/y/SECRET"; ln -s conf "${I}/CONF"; }
# One line: the control channel carries a command as one line. The append through the task's own link uses dd with
# fsync: the client already knows that file, so a plain ">>" opens it with no request to the server, and the
# server's refusal of the write would come back only at close, where the shell does not look.
PROT='try() { if e=$("$@" 2>&1); then echo "DONE: $*"; else echo "refused: $* [$(echo "$e" | tail -1 | sed "s/.*: //")]"; fi; }; try sh -c "echo evil >> /d/y/SECRET"; try sh -c ": > /d/y/SECRET"; try rm /d/y/SECRET; try mv /d/y/SECRET /d/y/gone; echo f > /d/free0; try sh -c "echo evil > /d/CONF/new"; try rm /d/CONF/keep; try mv /d/free0 /d/CONF/in; try sh -c "echo evil >> /d/y/hard"; try ln /d/CONF/keep /d/k1; ln -s CONF /d/L 2>/dev/null; try sh -c "echo evil > /d/L/new"; try rm /d/L/keep; ln -s y/SECRET /d/LF 2>/dev/null; try sh -c "echo evil | dd of=/d/LF oflag=append conv=notrunc,fsync"; try touch -h /d/LF; echo f > /d/free1; try mv /d/free1 /d/y/SECRET; ln -s conf/keep /d/L3 2>/dev/null; try ln /d/L3 /d/k3; sync; mv /d/y /d/work/y && mv /d/top /d/work/top && echo moved; try sh -c "echo evil >> /w/y/secret"; try rm /w/y/secret; try sh -c "echo evil > /w/top/conf2/new"; try sh -c "echo evil >> /w/top/conf2/keep"; try rm /w/top/conf2/keep; try mv /w/top/conf2/keep /w/top/conf2/gone; try ln /w/top/conf2/keep /w/k2; try mkdir /w/top/conf2/nd; try ln -s x /w/top/conf2/nl; ln -s /nowhere/at/all /d/dl; case "$(chown -h 0:0 /d/dl 2>&1)" in *Read-only*) echo link-attrs-refused ;; *) echo link-attrs-free ;; esac; rm -f /d/dl; sync; echo ok > /out/written && echo inner-rw-writes; echo free > /d/free && echo outer-rw-writes; rm -f /d/L /d/LF /d/L3 /d/free1 && echo own-links-removed; sync'
no_anchors binds-prot
o1="$(runb --bind="${I}:/d:rw" --bind="${I}/y/secret:/b:ro" --bind="${I}/conf:/c:ro" --bind="${I}/top/conf2:/c2:ro" --bind="${I}/work:/w:rw" --bind="${I}/rod:/r:ro" --bind="${I}/rod/out:/out:rw" binds-prot "${PROT}")"
p0="none"
no_anchors binds-prot2
st="$(runb --bind="${I}:/d:rw" --bind="${I}/hx/secret:/a:rw" --bind="${I}/hy/secret:/g:ro" --detach binds-prot2 'mv /d/hx /d/hx2 && mv /d/hy /d/hx && mkdir /d/hy && echo decoy > /d/hy/secret && echo renamed > /d/state2; i=0; while [ $i -lt 80 ] && [ ! -e /d/go ]; do i=$((i+1)); sleep 0.5; done; [ -e /d/go ] && echo went > /d/state3; { echo evil >> /a; sync; } 2>/dev/null; { echo evil >> /d/hx/secret; sync; } 2>/dev/null; { echo evil >> /d/hx/SECRET; sync; } 2>/dev/null; sleep 1')"
for _ in $(seq 1 20); do [ -e "${I}/state2" ] && break; sleep 0.5; done
# The server process itself: the one killed, and the one that listens afterwards.
upid2() { ctl microvm unfs-isolation binds-prot2 | sed -n 's/^UNFSISO [^ ]* pid=\([0-9]*\).*/\1/p' | head -1; }
p0="$(upid2)"; killed="$(ctl microvm test-unfs-kill binds-prot2 | head -1)"
p1=""; for _ in $(seq 1 40); do case "$(ctl microvm unfs binds-prot2 | head -1)" in *"listen=yes"*) p1="$(upid2)"; [ -n "${p1}" ] && [ "${p1}" != "${p0}" ] && break ;; esac; sleep 0.5; done
slog="$(ctl microvm unfs-log binds-prot2 40 | grep -a 'served read-only' | tail -1)"; touch "${I}/go"
wait_stopped binds-prot2 80 || ctl ctr -- kill binds-prot2 30 >/dev/null
ctl ctr -- delete binds-prot2 >/dev/null; sleep 2
m1="$(tr '\n' ' ' < "${I}/work/y/secret" 2>/dev/null)"; m2="$(tr '\n' ' ' < "${I}/conf/keep" 2>/dev/null):$(ls "${I}/conf" | tr '\n' ' ')"
m5="$(tr '\n' ' ' < "${I}/work/top/conf2/keep" 2>/dev/null):$(ls "${I}/work/top/conf2" | tr '\n' ' ')"; m3="$(tr '\n' ' ' < "${I}/hx/secret" 2>/dev/null)"
m4="$(cat "${I}/rod/out/written" 2>/dev/null)/$(cat "${I}/free" 2>/dev/null)"; stray="$(ls "${I}" "${I}/work" "${I}/work/top/conf2" | grep -c -e '^k[123]$' -e '^n[dl]$')"; mt1="$(stat -f %m "${I}/work/y/secret" 2>/dev/null)"
done_n="$(printf '%s\n' "${o1}" | grep -c 'DONE:')"; ref_n="$(printf '%s\n' "${o1}" | grep -c 'refused:')"; rofs_n="$(printf '%s\n' "${o1}" | grep -c 'refused:.*\[Read-only file system\]')"
# Every try must be refused by the server as read-only (another error would not show the check), the link the
# task made to nowhere must keep its own attributes free, and the restart must have used the record.
if [ "${done_n}" = 0 ] && [ "${ref_n}" = 24 ] && [ "${rofs_n}" = 24 ] && [ "${mt1}" = "${mt0}" ] && [ -n "${slog}" ] && case "${o1}" in "RC 0"*"OUT moved"*"OUT link-attrs-free"*"OUT inner-rw-writes"*"OUT outer-rw-writes"*"OUT own-links-removed"*) true ;; *) false ;; esac \
  && [ "${m1}" = "protected " ] && [ "${m2}" = "keep :keep " ] && [ "${m5}" = "keep2 :keep " ] && [ "${m4}" = "ok/free" ] && [ "${stray}" = 0 ] \
  && [ "${st}" = "STARTED binds-prot2" ] && [ "$(cat "${I}/state3" 2>/dev/null)" = went ] && [ -n "${p1}" ] && [ "${p1}" != "${p0}" ] && [ "${m3}" = "guarded " ]; then
  record read_only_holds_under_every_name true "volume ignores case: ${insensitive}; ${ref_n} changes through other names, links and after the moves all refused ($(printf '%s\n' "${o1}" | grep 'refused:' | sed 's/.*\[//; s/\]//' | sort | uniq -c | tr '\n' ';')); macOS: secret='${m1}' conf='${m2}' conf2='${m5}'; the server went from pid ${p0} to ${p1} after the renames (${slog:-no line}), the task wrote after that, and macOS still reads '${m3}'; a read-write bind inside a read-only one, and the outer bind, still write (${m4})"
else
  record read_only_holds_under_every_name false "volume ignores case: ${insensitive}; ${done_n} changes went through: $(printf '%s\n' "${o1}" | grep 'DONE:' | tr '\n' ';'); ${ref_n} refused (want 24), ${rofs_n} as read-only; mtime ${mt0} -> ${mt1:-none}; out: $(printf '%s' "${o1}" | grep -v 'refused:.*Read-only' | tr '\n' ' ' | cut -c1-360); macOS secret='${m1}' conf='${m2}' conf2='${m5}' written/free='${m4}' stray links ${stray}; restart part: ${st}; ${killed:-not killed}; pid ${p0} -> ${p1:-none}; state3 '$(cat "${I}/state3" 2>/dev/null)'; hx/secret='${m3}' (want 'guarded')"
fi

# --- 6i. A file that replaced a read-only file stays read-only after the task moves it aside (TD-073). macOS
# replaces a read-only file that lies in a read-write bind, and the server attaches the new file. The task then
# moves the file's directory aside and puts a decoy at the path, so the server attaches the decoy. The new file is no
# longer at the protected place, but the server remembers every object it found there, and refuses a change to it
# under a name that has no read-only mount: a hard link macOS made, and the other letter case -----------------------
K="${F}/k73"; mkdir -p "${K}/hz"; echo first > "${K}/hz/secret"
MOVED='try() { if e=$("$@" 2>&1); then echo "DONE: $*"; else echo "refused: $* [$(echo "$e" | tail -1 | sed "s/.*: //")]"; fi; }; i=0; while [ $i -lt 80 ] && [ ! -e /d/go1 ]; do i=$((i+1)); sleep 0.5; done; mv /d/hz /d/hz2 && mkdir /d/hz && echo decoy > /d/hz/secret && echo moved > /d/state; i=0; while [ $i -lt 80 ] && [ ! -e /d/go2 ]; do i=$((i+1)); sleep 0.5; done; { try dd if=/d/state of=/d/hz2/hard conv=notrunc,fsync; try sh -c ": > /d/hz2/hard"; try rm /d/hz2/hard; try mv /d/hz2/hard /d/gone; [ -e /d/HZ2/secret ] && try dd if=/d/state of=/d/HZ2/secret conv=notrunc,fsync; echo tried; } > /d/out 2>&1; sync'
no_anchors binds-moved
st="$(runb --bind="${K}:/d:rw" --bind="${K}/hz/secret:/b:ro" --detach binds-moved "${MOVED}")"
attaches() { ctl microvm unfs-log binds-moved 80 | grep -a -c 'another file is at its path'; }
sleep 2; a0="$(attaches)"; echo second > "${K}/hz/secret.new"; mv "${K}/hz/secret.new" "${K}/hz/secret"; ln "${K}/hz/secret" "${K}/hz/hard"
a1=0; for _ in $(seq 1 30); do a1="$(attaches)"; [ "${a1}" -gt "${a0}" ] && break; sleep 0.5; done
touch "${K}/go1"; for _ in $(seq 1 30); do [ -e "${K}/state" ] && break; sleep 0.5; done
a2=0; for _ in $(seq 1 30); do a2="$(attaches)"; [ "${a2}" -gt "${a1}" ] && break; sleep 0.5; done
# The System VM gives a file another inode number once nothing holds it (run/TD-073/notes.txt 8). The supervisor
# holds the file it replaced, so the number the server remembers stays that file's after the caches are dropped.
sleep 2; dropped="$(ctl microvm test-drop-caches | head -1)"; sleep 1; touch "${K}/go2"
wait_stopped binds-moved 80 || ctl ctr -- kill binds-moved 30 >/dev/null
ctl ctr -- delete binds-moved >/dev/null; sleep 2
ko="$(cat "${K}/out" 2>/dev/null)"; kd="$(printf '%s\n' "${ko}" | grep -c 'DONE:')"; kr="$(printf '%s\n' "${ko}" | grep -c 'refused:')"; kro="$(printf '%s\n' "${ko}" | grep -c 'refused:.*\[Read-only file system\]')"
ks="$(tr '\n' ' ' < "${K}/hz2/secret" 2>/dev/null)"; kh="$(tr '\n' ' ' < "${K}/hz2/hard" 2>/dev/null)"
if [ "${st}" = "STARTED binds-moved" ] && [ "${a1}" -gt "${a0}" ] && [ "${a2}" -gt "${a1}" ] && [ "${dropped}" = DROPPED ] && [ "${kd}" = 0 ] && [ "${kr}" -ge 4 ] && [ "${kro}" = "${kr}" ] \
  && case "${ko}" in *tried*) true ;; *) false ;; esac && [ "${ks}" = "second " ] && [ "${kh}" = "second " ]; then
  record replaced_file_stays_read_only_when_moved true "the server attached the file macOS put in place, then the task's decoy (${a2} attaches), and the System VM dropped its caches; ${kr} changes to the moved file through a hard link and the other letter case all refused ($(printf '%s\n' "${ko}" | grep 'refused:' | sed 's/.*\[//; s/\]//' | sort | uniq -c | tr '\n' ';')); macOS still reads '${ks}'"
else
  record replaced_file_stays_read_only_when_moved false "${st}; attaches ${a0}, ${a1} then ${a2}; caches: ${dropped:-not dropped}; ${kd} changes went through: $(printf '%s\n' "${ko}" | grep 'DONE:' | tr '\n' ';'); ${kr} refused; out: $(printf '%s' "${ko}" | tr '\n' ' ' | cut -c1-300); macOS hz2/secret='${ks}' hz2/hard='${kh}' (want 'second ')"
fi

# --- 6j. After a restart, a read-write file bind that the task points at a file in a read-only directory is
# attached read-only (TD-073, council S-3). The task first puts a decoy at the read-only directory's path, so the
# server that starts after a kill has no read-only mount on the true directory; only the supervisor's record names
# it. The task then renames directories so that its read-write file bind's path names a file in the true directory.
# The supervisor attaches that file, compares the path with the record, and attaches it read-only -----------------
J="${F}/j73"; mkdir -p "${J}/p/conf" "${J}/x/conf"; echo keep > "${J}/p/conf/keep"; echo mine > "${J}/x/conf/keep"
REATT='try() { if e=$("$@" 2>&1); then echo "DONE: $*"; else echo "refused: $* [$(echo "$e" | tail -1 | sed "s/.*: //")]"; fi; }; w() { i=0; while [ $i -lt 80 ] && [ ! -e "$1" ]; do i=$((i+1)); sleep 0.5; done; }; mv /d/p /d/q && mkdir -p /d/p/conf && echo one > /d/state1; w /d/go1; mv /d/x /d/x9 && mv /d/q /d/x && echo two > /d/state2; w /d/go2; { n=0; while [ $n -lt 10 ]; do r="$(try dd if=/d/state2 of=/a conv=notrunc,fsync)"; case "$r" in *"Stale file handle"*) n=$((n+1)); sleep 1 ;; *) break ;; esac; done; echo "$r"; try sh -c ": > /a"; echo tried; } > /d/out 2>&1; sync'
no_anchors binds-reatt
st="$(runb --bind="${J}:/d:rw" --bind="${J}/p/conf:/c:ro" --bind="${J}/x/conf/keep:/a:rw" --detach binds-reatt "${REATT}")"
for _ in $(seq 1 30); do [ -e "${J}/state1" ] && break; sleep 0.5; done
jpid() { ctl microvm unfs-isolation binds-reatt | sed -n 's/^UNFSISO [^ ]* pid=\([0-9]*\).*/\1/p' | head -1; }
j0="$(jpid)"; jk="$(ctl microvm test-unfs-kill binds-reatt | head -1)"
j1=""; for _ in $(seq 1 40); do case "$(ctl microvm unfs binds-reatt | head -1)" in *"listen=yes"*) j1="$(jpid)"; [ -n "${j1}" ] && [ "${j1}" != "${j0}" ] && break ;; esac; sleep 0.5; done
touch "${J}/go1"; for _ in $(seq 1 30); do [ -e "${J}/state2" ] && break; sleep 0.5; done
jlog=""; for _ in $(seq 1 30); do jlog="$(ctl microvm unfs-log binds-reatt 80 | grep -a 'is attached read-only' | tail -1)"; [ -n "${jlog}" ] && break; sleep 0.5; done
sleep 2; touch "${J}/go2"   # the guest binds the new file within seconds; the task tries again while its bind is stale
wait_stopped binds-reatt 80 || ctl ctr -- kill binds-reatt 30 >/dev/null
ctl ctr -- delete binds-reatt >/dev/null; sleep 2
jo="$(cat "${J}/out" 2>/dev/null)"; jd="$(printf '%s\n' "${jo}" | grep -c 'DONE:')"; jr="$(printf '%s\n' "${jo}" | grep -c 'refused:')"; jro="$(printf '%s\n' "${jo}" | grep -c 'refused: dd .*\[Read-only file system\]')"
jm="$(tr '\n' ' ' < "${J}/x/conf/keep" 2>/dev/null)"
if [ "${st}" = "STARTED binds-reatt" ] && [ -n "${j1}" ] && [ "${j1}" != "${j0}" ] && [ -n "${jlog}" ] && [ "${jd}" = 0 ] && [ "${jr}" = 2 ] && [ "${jro}" = 1 ] && [ "${jm}" = "keep " ]; then
  record reattached_under_a_recorded_object_is_read_only true "the server went from pid ${j0} to ${j1}; after the task's second renames the supervisor logged: ${jlog}; the write through the file bind was refused as read-only and the truncate failed ($(printf '%s\n' "${jo}" | grep 'refused:' | sed 's/.*\[//; s/\]//' | tr '\n' ';')); macOS still reads '${jm}'"
else
  record reattached_under_a_recorded_object_is_read_only false "${st}; ${jk:-not killed}; pid ${j0} -> ${j1:-none}; log: ${jlog:-no line}; ${jd} went through, ${jr} refused, the write as read-only: ${jro}; out: $(printf '%s' "${jo}" | tr '\n' ' ' | cut -c1-300); macOS x/conf/keep='${jm}' (want 'keep ')"
fi

# --- 6k. What was bound read-only is still known after a restart and a cache drop (TD-076). The task moves the
# read-only file's directory aside and puts a decoy at the path; the server is killed and starts again from the
# paths, so the true file is at no source's path and only the supervisor's record names it, by inode number. The
# System VM gives a file another number once it has forgotten the file, so the supervisor holds each recorded
# object with a mount that outlives it. The caches are dropped after the restart, and the task then tries to change
# the true file under its new name -------------------------------------------------------------------------------
L="${F}/l76"; mkdir -p "${L}/hy"; echo guarded > "${L}/hy/secret"
HELD='try() { if e=$("$@" 2>&1); then echo "DONE: $*"; else echo "refused: $* [$(echo "$e" | tail -1 | sed "s/.*: //")]"; fi; }; w() { i=0; while [ $i -lt 80 ] && [ ! -e "$1" ]; do i=$((i+1)); sleep 0.5; done; }; mv /d/hy /d/hx && mkdir /d/hy && echo decoy > /d/hy/secret && echo one > /d/state1; w /d/go1; { try dd if=/d/state1 of=/d/hx/secret conv=notrunc,fsync; try sh -c ": > /d/hx/secret"; try rm /d/hx/secret; echo tried; } > /d/out 2>&1; sync'
no_anchors binds-held
st="$(runb --bind="${L}:/d:rw" --bind="${L}/hy/secret:/g:ro" --detach binds-held "${HELD}")"
for _ in $(seq 1 30); do [ -e "${L}/state1" ] && break; sleep 0.5; done
lpid() { ctl microvm unfs-isolation binds-held | sed -n 's/^UNFSISO [^ ]* pid=\([0-9]*\).*/\1/p' | head -1; }
sleep 3; l0="$(lpid)"; lheld="$(ctl microvm unfs-holds binds-held | head -1)"; lk="$(ctl microvm test-unfs-kill binds-held | head -1)"
l1=""; for _ in $(seq 1 40); do case "$(ctl microvm unfs binds-held | head -1)" in *"listen=yes"*) l1="$(lpid)"; [ -n "${l1}" ] && [ "${l1}" != "${l0}" ] && break ;; esac; sleep 0.5; done
sleep 1; ldrop="$(ctl microvm test-drop-caches | head -1)"; sleep 1; touch "${L}/go1"
wait_stopped binds-held 80 || ctl ctr -- kill binds-held 30 >/dev/null
ctl ctr -- delete binds-held >/dev/null; sleep 2
lo="$(cat "${L}/out" 2>/dev/null)"; ld="$(printf '%s\n' "${lo}" | grep -c 'DONE:')"; lr="$(printf '%s\n' "${lo}" | grep -c 'refused:.*\[Read-only file system\]')"
lm="$(tr '\n' ' ' < "${L}/hx/secret" 2>/dev/null)"; lleft="$(ctl microvm unfs-holds binds-held | head -1)"
if [ "${st}" = "STARTED binds-held" ] && [ -n "${l1}" ] && [ "${l1}" != "${l0}" ] && [ "${ldrop}" = DROPPED ] && [ "${ld}" = 0 ] && [ "${lr}" = 3 ] && [ "${lm}" = "guarded " ] && [ "${lleft}" = "HOLDS binds-held 0" ] \
  && case "${lheld}" in "HOLDS binds-held "[1-9]*) true ;; *) false ;; esac; then
  record recorded_object_known_after_restart_and_cache_drop true "the server went from pid ${l0} to ${l1} after the task moved the read-only file aside, and the System VM dropped its caches; ${lr} changes to the file under its new name refused as read-only; macOS still reads '${lm}'; holds before the kill: ${lheld#HOLDS binds-held }, after the task: ${lleft#HOLDS binds-held }"
else
  record recorded_object_known_after_restart_and_cache_drop false "${st}; ${lk:-not killed}; pid ${l0} -> ${l1:-none}; caches: ${ldrop:-not dropped}; ${ld} went through: $(printf '%s\n' "${lo}" | grep 'DONE:' | tr '\n' ';'); ${lr} refused as read-only (want 3); out: $(printf '%s' "${lo}" | tr '\n' ' ' | cut -c1-300); macOS hx/secret='${lm}' (want 'guarded '); holds before the kill: ${lheld:-unknown}; left: ${lleft:-unknown}"
fi

# --- 6l. An earlier version of a read-only file stays read-only though the task sent nothing while it was bound
# (TD-076). An editor that keeps a backup renames the file to "secret~" and writes a new one, twice, as soon as the
# first new file is attached. The second version was the bound file between the two saves and is the backup after
# the second. The task does not touch its binds in that time (it sleeps), so the server did not learn the file from
# a request of the task that changes something; the guest's own requests after each attach, which change nothing,
# are looks too, and the server's look on each turn of its loop has no test apart from them. Then the server is
# killed and the caches are dropped: the next server knows that version only by its
# recorded number, which is its number only because the supervisor held it with a mount when it was attached.
# The fixture is on the share that sends no change events: with events, the guest's replay of each save is a
# request that changes something, and the task would not be idle.
E="${SHARE2}/$(basename "${F}")/e76"; mkdir -p "${E}/hz"; echo v0 > "${E}/hz/secret"
IDLE='try() { if e=$("$@" 2>&1); then echo "DONE: $*"; else echo "refused: $* [$(echo "$e" | tail -1 | sed "s/.*: //")]"; fi; }; w() { i=0; while [ $i -lt 160 ] && [ ! -e "$1" ]; do i=$((i+1)); sleep 0.5; done; }; tries() { try dd if=/d/go1 of="/d/hz/secret~" conv=notrunc,fsync; try sh -c ": > /d/hz/secret~"; try rm "/d/hz/secret~"; echo tried; }; sleep 22; tries > /d/out1 2>&1; sync; w /d/go2; tries > /d/out2 2>&1; sync'
st="$(runb --bind="${E}:/d:rw" --bind="${E}/hz/secret:/b:ro" --detach binds-idle "${IDLE}")"
eatt() { ctl microvm unfs-log binds-idle 80 | grep -a -c 'another file is at its path'; }
epid() { ctl microvm unfs-isolation binds-idle | sed -n 's/^UNFSISO [^ ]* pid=\([0-9]*\).*/\1/p' | head -1; }
# The server is stopped across both saves, as a task can do to its own server with a request it does not finish
# (TD-078): it cannot look at anything in that time, and must know the middle version all the same.
slines() { ctl microvm unfs-list binds-idle | sed -n 's/^LIST [^ ]* .* s=\([0-9]*\)$/\1/p' | head -1; }
sleep 2; e0="$(eatt)"; epause="$(ctl microvm test-unfs-pause binds-idle | head -1)"; s0="$(slines)"
echo v1 > "${E}/hz/new"; mv "${E}/hz/secret" "${E}/hz/secret~"; mv "${E}/hz/new" "${E}/hz/secret"
e1="${e0}"; for _ in $(seq 1 60); do e1="$(eatt)"; [ "${e1}" -gt "${e0}" ] && break; sleep 0.25; done
echo v2 > "${E}/hz/new"; mv "${E}/hz/secret" "${E}/hz/secret~"; mv "${E}/hz/new" "${E}/hz/secret"
e2="${e1}"; for _ in $(seq 1 30); do e2="$(eatt)"; [ "${e2}" -gt "${e1}" ] && break; sleep 0.5; done
s2="$(slines)"; eres="$(ctl microvm test-unfs-resume binds-idle | head -1)"   # the list was told of both files while the server stood still
sleep 2; edrop="$(ctl microvm test-drop-caches | head -1)"; echo go > "${E}/go1"   # the first tries read go1 as their data
for _ in $(seq 1 60); do [ -s "${E}/out1" ] && grep -q tried "${E}/out1" && break; sleep 0.5; done
p0e="$(epid)"; ctl microvm test-unfs-kill binds-idle >/dev/null
p1e=""; for _ in $(seq 1 40); do case "$(ctl microvm unfs binds-idle | head -1)" in *"listen=yes"*) p1e="$(epid)"; [ -n "${p1e}" ] && [ "${p1e}" != "${p0e}" ] && break ;; esac; sleep 0.5; done
sleep 1; edrop2="$(ctl microvm test-drop-caches | head -1)"; sleep 1; echo go > "${E}/go2"
wait_stopped binds-idle 120 || ctl ctr -- kill binds-idle 30 >/dev/null
ctl ctr -- delete binds-idle >/dev/null; sleep 2
count() { printf '%s\n' "$1" | grep -c "$2"; }
eo1="$(cat "${E}/out1" 2>/dev/null)"; eo2="$(cat "${E}/out2" 2>/dev/null)"
ed="$(( $(count "${eo1}" 'DONE:') + $(count "${eo2}" 'DONE:') ))"; er1="$(count "${eo1}" 'refused:.*\[Read-only file system\]')"; er2="$(count "${eo2}" 'refused:.*\[Read-only file system\]')"
em="$(tr '\n' ' ' < "${E}/hz/secret~" 2>/dev/null)"
if [ "${st}" = "STARTED binds-idle" ] && [ "${e1}" -gt "${e0}" ] && [ "${e2}" -gt "${e1}" ] && [ "${epause}" = "SIGSTOP unfsd binds-idle" ] && [ "${eres}" = "SIGCONT unfsd binds-idle" ] && [ "${s0}" = 1 ] && [ "${s2:-0}" -ge 3 ] && [ "${edrop}" = DROPPED ] && [ "${edrop2}" = DROPPED ] && [ -n "${p1e}" ] && [ "${p1e}" != "${p0e}" ] \
  && [ "${ed}" = 0 ] && [ "${er1}" = 3 ] && [ "${er2}" = 3 ] && [ "${em}" = "v1 " ]; then
  record earlier_version_stays_read_only_for_an_idle_task true "macOS saved the read-only file twice with a backup, the second time as soon as the first was attached (${e2} attaches), while the task slept and the server was stopped (its list went from ${s0} to ${s2} told objects); after the server went on and a cache drop ${er1} changes to the backup, which was the bound file between the saves, were refused as read-only; after the server went from pid ${p0e} to ${p1e} and another cache drop, ${er2} more; macOS still reads '${em}'"
else
  record earlier_version_stays_read_only_for_an_idle_task false "${st}; attaches ${e0}, ${e1} then ${e2}; ${epause:-not stopped}, ${eres:-not continued}; told objects in the list ${s0:-?} then ${s2:-?} (want 1, then 3 or more); caches: ${edrop:-not dropped}, ${edrop2:-not dropped}; pid ${p0e} -> ${p1e:-none}; ${ed} went through: $(printf '%s\n%s\n' "${eo1}" "${eo2}" | grep 'DONE:' | tr '\n' ';'); refused as read-only ${er1} then ${er2} (want 3 and 3); out: $(printf '%s | %s' "${eo1}" "${eo2}" | tr '\n' ' ' | cut -c1-400); macOS hz/secret~='${em}' (want 'v1 ')"
fi

# --- 6m. A list the server cannot take refuses every change, and a start of the server mends it (TD-078). The list
# is what the protection rests on, so a line of no known kind must not be passed over: a number may be lost in it.
# A test command adds such a line. The task's write to its read-write bind is then refused. The server is killed and
# starts again with a list written anew, and the task writes ------------------------------------------------------
N="${F}/n78"; mkdir -p "${N}/ro"; echo k > "${N}/ro/keep"
BROKEN='try() { if e=$("$@" 2>&1); then echo "DONE: $*"; else echo "refused: $* [$(echo "$e" | tail -1 | sed "s/.*: //")]"; fi; }; w() { i=0; while [ $i -lt 120 ] && [ ! -e "$1" ]; do i=$((i+1)); sleep 0.5; done; }; w /d/go1; a="$(try sh -c "echo a > /d/one")"; w /d/go2; b="$(try sh -c "echo b > /d/two")"; { echo "$a"; echo "$b"; echo tried; } > /d/out; sync'
st="$(runb --bind="${N}:/d:rw" --bind="${N}/ro:/r:ro" --detach binds-broken "${BROKEN}")"
npid() { ctl microvm unfs-isolation binds-broken | sed -n 's/^UNFSISO [^ ]* pid=\([0-9]*\).*/\1/p' | head -1; }
sleep 2; nbrk="$(ctl microvm test-unfs-list-break binds-broken | head -1)"; touch "${N}/go1"; sleep 5
n0="$(npid)"; ctl microvm test-unfs-kill binds-broken >/dev/null
n1=""; for _ in $(seq 1 40); do case "$(ctl microvm unfs binds-broken | head -1)" in *"listen=yes"*) n1="$(npid)"; [ -n "${n1}" ] && [ "${n1}" != "${n0}" ] && break ;; esac; sleep 0.5; done
touch "${N}/go2"
wait_stopped binds-broken 90 || ctl ctr -- kill binds-broken 30 >/dev/null
ctl ctr -- delete binds-broken >/dev/null; sleep 2
no="$(cat "${N}/out" 2>/dev/null)"; none="$(cat "${N}/one" 2>/dev/null)"; ntwo="$(cat "${N}/two" 2>/dev/null)"
case "${st}|${nbrk}|$(printf '%s' "${no}" | tr '\n' '|')" in
  "STARTED binds-broken|BROKEN list of binds-broken|refused: "*"[Read-only file system]|DONE: "*"|tried"*)
    if [ -n "${n1}" ] && [ "${n1}" != "${n0}" ] && [ -z "${none}" ] && [ "${ntwo}" = b ]; then
      record broken_list_refuses_every_change true "with a line of no kind in its list the server refused a write to the read-write bind as read-only; after the server went from pid ${n0} to ${n1} with a list written anew, the task wrote (macOS: one absent, two='${ntwo}')"
    else
      record broken_list_refuses_every_change false "pid ${n0} -> ${n1:-none}; macOS one='${none}' (want absent) two='${ntwo}' (want b); out: $(printf '%s' "${no}" | tr '\n' ' ' | cut -c1-300)"
    fi ;;
  *) record broken_list_refuses_every_change false "${st}; ${nbrk:-list not broken}; pid ${n0} -> ${n1:-none}; out: $(printf '%s' "${no}" | tr '\n' ' ' | cut -c1-300); macOS one='${none}' two='${ntwo}'" ;;
esac

# --- 6n. An object the server is told of again is the newest again (TD-079). The server remembers 768 objects and
# drops the one that was last told or found longest ago. macOS saves the read-only file twice with a backup, so the
# first new file is the backup and at no protected place. A test command then tells the server of 760 made-up
# objects, of that backup again, and of 10 more made-up ones: more than the server remembers. By the order in which
# the server first learned them the backup is among the oldest and would go; told again, it is among the newest
# and stays. The task then tries to change it. Then macOS saves twice more, with the server's memory full: the
# backup is now the third new file, which the server learned last, and it must be refused too (a server that
# dropped its newest object, or took no new one when full, would let that through) ---------------------------------
R="${F}/r79"; mkdir -p "${R}/hz"; echo v0 > "${R}/hz/secret"
RETOLD='try() { if e=$("$@" 2>&1); then echo "DONE: $*"; else echo "refused: $* [$(echo "$e" | tail -1 | sed "s/.*: //")]"; fi; }; w() { i=0; while [ $i -lt 160 ] && [ ! -e "$1" ]; do i=$((i+1)); sleep 0.5; done; }; tries() { try dd if=/d/go1 of="/d/hz/secret~" conv=notrunc,fsync; try sh -c ": > /d/hz/secret~"; try rm "/d/hz/secret~"; echo tried; }; w /d/go1; tries > /d/out1 2>&1; sync; w /d/go2; tries > /d/out2 2>&1; sync'
st="$(runb --bind="${R}:/d:rw" --bind="${R}/hz/secret:/b:ro" --detach binds-retold "${RETOLD}")"
ratt() { ctl microvm unfs-log binds-retold 80 | grep -a -c 'another file is at its path'; }
rsl() { ctl microvm unfs-list binds-retold | sed -n 's/^LIST [^ ]* .* s=\([0-9]*\)$/\1/p' | head -1; }
rsave() { echo "$1" > "${R}/hz/new"; mv "${R}/hz/secret" "${R}/hz/secret~"; mv "${R}/hz/new" "${R}/hz/secret"; local b a; b="$(ratt)"; a="${b}"; for _ in $(seq 1 60); do a="$(ratt)"; [ "${a}" -gt "$2" ] && break; sleep 0.25; done; echo "${a}"; }
sleep 2; r0="$(ratt)"; r1="$(rsave v1 "${r0}")"; r2="$(rsave v2 "${r1}")"
# The second told object is the backup only when the list has just its three told objects: the file of the start
# and the two new files.
sleep 1; rs="$(rsl)"; rtold="$(ctl microvm test-unfs-list-retell binds-retold 2 760 10 | head -1)"; rlist="$(ctl microvm unfs-list binds-retold | head -1)"
echo go > "${R}/go1"
for _ in $(seq 1 60); do grep -q tried "${R}/out1" 2>/dev/null && break; sleep 0.5; done
rm1="$(cat "${R}/hz/secret~" 2>/dev/null | tr '\n' ' ')"
r3="$(rsave v3 "${r2}")"; r4="$(rsave v4 "${r3}")"; sleep 1; echo go > "${R}/go2"
wait_stopped binds-retold 120 || ctl ctr -- kill binds-retold 30 >/dev/null
ctl ctr -- delete binds-retold >/dev/null; sleep 2
cnt() { printf '%s\n' "$1" | grep -c "$2"; }
ro1="$(cat "${R}/out1" 2>/dev/null)"; ro2="$(cat "${R}/out2" 2>/dev/null)"
rd="$(( $(cnt "${ro1}" 'DONE:') + $(cnt "${ro2}" 'DONE:') ))"; rr1="$(cnt "${ro1}" 'refused:.*\[Read-only file system\]')"; rr2="$(cnt "${ro2}" 'refused:.*\[Read-only file system\]')"
rm2="$(cat "${R}/hz/secret~" 2>/dev/null | tr '\n' ' ')"
case "${st}|${rs}|${rtold}" in
  "STARTED binds-retold|3|RETOLD s "*)
    if [ "${r1}" -gt "${r0}" ] && [ "${r2}" -gt "${r1}" ] && [ "${r3}" -gt "${r2}" ] && [ "${r4}" -gt "${r3}" ] && [ "${rd}" = 0 ] && [ "${rr1}" = 3 ] && [ "${rr2}" = 3 ] && [ "${rm1}" = "v1 " ] && [ "${rm2}" = "v3 " ]; then
      record object_told_again_is_the_newest true "the backup of the read-only file was told again between 770 made-up objects (${rlist}); 3 changes to it were refused as read-only and macOS still read '${rm1}'; after two more saves with the memory full, 3 changes to the new backup were refused and macOS reads '${rm2}'"
    else
      record object_told_again_is_the_newest false "attaches ${r0}, ${r1}, ${r2}, ${r3}, ${r4}; ${rlist}; ${rd} went through: $(printf '%s\n%s\n' "${ro1}" "${ro2}" | grep 'DONE:' | tr '\n' ';'); refused as read-only ${rr1} then ${rr2} (want 3 and 3); macOS hz/secret~ '${rm1}' then '${rm2}' (want 'v1 ' then 'v3 ')"
    fi ;;
  *) record object_told_again_is_the_newest false "${st}; told objects before the command: ${rs:-?} (want 3: more would make another object the second); ${rtold:-not told again}; attaches ${r0}, ${r1}, ${r2}; out: $(printf '%s | %s' "${ro1}" "${ro2}" | tr '\n' ' ' | cut -c1-300)" ;;
esac

# --- 6o. A task cannot change what the path of a read-only bind names (TD-080). A read-only bind that lies inside a
# read-write bind is reached by a chain of directories from the read-write bind's root. A task that renames one of
# them aside and puts a decoy there makes the supervisor serve the decoy, pushes earlier versions of the true file
# out of the server's memory, and gets the decoy bound at its next launch. So the server refuses a rename or a
# removal of such a directory, by any name: its own, another letter case, a second read-write bind, a rename of
# another directory onto it, a raw REMOVE of it when it is empty. For a read-only DIRECTORY bind the directory at
# the bind's own path counts too, also right under the read-write bind's root. What lies in such a directory, and
# every other directory, stays as free as before.
# Part 1: the task tries, and shows what stays free. Part 2: macOS empties one chain's directory, replaces a
# directory of another chain and replaces a read-only directory bind's own directory; the supervisor must tell the
# server of the new directories (its log line, and more anchors in the list); the caches are dropped; the task
# tries again, the new directories too. Part 3: the server is killed and starts again from the paths; the task
# tries once more. Then a second launch must bind what is at the user's paths.
A="${F}/a80"; mkdir -p "${A}/hy" "${A}/top/mid/conf" "${A}/ed" "${A}/rootconf"; echo guarded > "${A}/hy/secret"; echo keep > "${A}/top/mid/conf/keep"; echo gone > "${A}/ed/leaf"; echo rk > "${A}/rootconf/keep"
ANCHOR='try() { if e=$("$@" 2>&1); then echo "DONE: $*"; else echo "refused: $* [$(echo "$e" | tail -1 | sed "s/.*: //")]"; fi; }; w() { i=0; while [ $i -lt 200 ] && [ ! -e "$1" ]; do i=$((i+1)); sleep 0.5; done; }; { try mv /d/hy /d/t1; [ -e /d/HY ] && try mv /d/HY /d/t1b; try mv /d/top /d/t2; try mv /d/top/mid /d/top/m2; try mv /t/mid /t/m3; try mv /d/top/mid/conf /d/top/mid/c2; try mv /d/rootconf /d/rc2; echo x > /d/hy/other && mv /d/hy/other /d/hy/other2 && rm /d/hy/other2 && echo inside-free; mkdir /d/top/sub && mv /d/top/sub /d/top/sub2 && rmdir /d/top/sub2 && echo dirs-inside-free; mkdir /d/free && mv /d/free /d/free2 && rmdir /d/free2 && echo other-dirs-free; echo tried; } > /d/out1 2>&1; sync; w /d/go1; { try mv /d/top/mid /d/top/m9; try mv /d/top/mid/conf /d/top/mid/c9; try mv /d/rootconf /d/rc9; try rmdir /d/ed; mkdir /d/e2; try mv -T /d/e2 /d/ed; rmdir /d/e2; try mv /d/hy /d/t9; echo tried; } > /d/out2 2>&1; sync; w /d/go2; { try mv /d/hy /d/t10; try mv /d/top/mid /d/top/m10; try mv /d/rootconf /d/rc10; echo tried; } > /d/out3 2>&1; sync'
ABINDS=(--bind="${A}:/d:rw" --bind="${A}/top:/t:rw" --bind="${A}/hy/secret:/b:ro" --bind="${A}/top/mid/conf:/c:ro" --bind="${A}/ed/leaf:/e:ro" --bind="${A}/rootconf:/rc:ro")
acase=no; [ -e "${A}/HY" ] && acase=yes; awant1=6; [ "${acase}" = yes ] && awant1=7
st="$(runb "${ABINDS[@]}" --detach binds-anchor "${ANCHOR}")"
anch() { ctl microvm unfs-list binds-anchor | sed -n 's/^LIST [^ ]* .* a=\([0-9]*\) .*$/\1/p' | head -1; }
atold() { ctl microvm unfs-log binds-anchor 80 | grep -a -c 'the server was told of the'; }
apid() { ctl microvm unfs-isolation binds-anchor | sed -n 's/^UNFSISO [^ ]* pid=\([0-9]*\).*/\1/p' | head -1; }
for _ in $(seq 1 60); do grep -q tried "${A}/out1" 2>/dev/null && break; sleep 0.5; done
aa0="$(anch)"; at0="$(atold)"
# macOS: the leaf of one chain goes, so its directory is empty; a directory of another chain is replaced, with the
# read-only directory in it; the directory of the read-only bind right under the root is replaced.
rm -f "${A}/ed/leaf"; mv "${A}/top/mid" "${A}/top/mid.old"; mkdir -p "${A}/top/mid/conf"; echo new > "${A}/top/mid/conf/keep"
mv "${A}/rootconf" "${A}/rootconf.old"; mkdir "${A}/rootconf"; echo rnew > "${A}/rootconf/keep"
# The watcher's next walks must tell the server: its log line, and more anchor lines in the list (each telling adds
# a line for every current anchor, so the count shows that it was told, not of what; the refused tries of part 2 on
# the new directories show that).
asec=0; at1="${at0}"; for _ in $(seq 1 30); do at1="$(atold)"; aa1="$(anch)"; [ "${at1}" -gt "${at0}" ] && [ "${aa1:-0}" -ge "$((aa0 + 3))" ] && break; asec=$((asec + 1)); sleep 0.5; done
araw="$(ctl microvm -- test-unfs-probe binds-anchor --remove ed | tr '\n' ';')"; adrop="$(ctl microvm test-drop-caches | head -1)"; sleep 1; touch "${A}/go1"
for _ in $(seq 1 60); do grep -q tried "${A}/out2" 2>/dev/null && break; sleep 0.5; done
# The file comes back, so that the next server finds its source; then the server is killed.
echo back > "${A}/ed/leaf"; sleep 3
ap0="$(apid)"; ctl microvm test-unfs-kill binds-anchor >/dev/null
ap1=""; for _ in $(seq 1 40); do case "$(ctl microvm unfs binds-anchor | head -1)" in *"listen=yes"*) ap1="$(apid)"; [ -n "${ap1}" ] && [ "${ap1}" != "${ap0}" ] && break ;; esac; sleep 0.5; done
aa2="$(anch)"; touch "${A}/go2"
wait_stopped binds-anchor 150 || ctl ctr -- kill binds-anchor 30 >/dev/null
ctl ctr -- delete binds-anchor >/dev/null; sleep 2
cnt() { printf '%s\n' "$1" | grep -c "$2"; }; ro='refused:.*\[Read-only file system\]'
ao1="$(cat "${A}/out1" 2>/dev/null)"; ao2="$(cat "${A}/out2" 2>/dev/null)"; ao3="$(cat "${A}/out3" 2>/dev/null)"
ad="$(( $(cnt "${ao1}" 'DONE:') + $(cnt "${ao2}" 'DONE:') + $(cnt "${ao3}" 'DONE:') ))"; ar1="$(cnt "${ao1}" "${ro}")"; ar2="$(cnt "${ao2}" "${ro}")"; ar3="$(cnt "${ao3}" "${ro}")"
aplace="$(cat "${A}/hy/secret" 2>/dev/null)/$(cat "${A}/top/mid.old/conf/keep" 2>/dev/null)/$(cat "${A}/top/mid/conf/keep" 2>/dev/null)/$(cat "${A}/rootconf/keep" 2>/dev/null)/$([ -d "${A}/ed" ] && echo ed)"
# A second launch binds what is at the user's paths: the true file, and the directories that macOS put there.
a2="$(runb --bind="${A}:/d:rw" --bind="${A}/hy/secret:/b:ro" --bind="${A}/top/mid/conf:/c:ro" --bind="${A}/rootconf:/rc:ro" binds-anchor2 'cat /b; cat /c/keep; cat /rc/keep' | tr '\n' ' ')"
ctl ctr -- delete binds-anchor2 >/dev/null
if [ "${st}" = "STARTED binds-anchor" ] && [ "${ad}" = 0 ] && [ "${ar1}" = "${awant1}" ] && [ "${ar2}" = 6 ] && [ "${ar3}" = 3 ] && [ "${aa0:-0}" -ge 6 ] && [ "${at1}" -gt "${at0}" ] && [ "${aa1:-0}" -ge "$((aa0 + 3))" ] \
  && [ "${adrop}" = DROPPED ] && [ -n "${ap1}" ] && [ "${ap1}" != "${ap0}" ] && [ "${aa2:-0}" -ge 6 ] && [ "${aplace}" = "guarded/keep/new/rnew/ed" ] \
  && case "${ao1}|${araw}|${a2}" in *inside-free*dirs-inside-free*other-dirs-free*"|REMOVE ed status=ROFS;"*"|"*"OUT guarded"*"OUT new"*"OUT rnew"*) true ;; *) false ;; esac; then
  record path_to_a_read_only_bind_cannot_be_changed true "volume ignores case: ${acase}; ${ar1} renames of directories on the way to a read-only bind, and of read-only directory binds, refused as read-only (by name, through a second bind); files and directories inside them, and other directories, stayed free; ${aa0} anchors in the server's list; after macOS replaced two of the directories and emptied a third, the supervisor told the server within $((asec / 2)) s (${aa1} anchor lines) and 6 more tries were refused, the new directories, a rename onto the empty one and its rmdir among them; a raw REMOVE of the empty one: ${araw} after a killed server (pid ${ap0} to ${ap1}, ${aa2} anchors) 3 more refused; a second launch read: ${a2}"
else
  record path_to_a_read_only_bind_cannot_be_changed false "${st}; volume ignores case: ${acase}; ${ad} went through: $(printf '%s\n%s\n%s\n' "${ao1}" "${ao2}" "${ao3}" | grep 'DONE:' | tr '\n' ';'); refused as read-only ${ar1}, ${ar2}, ${ar3} (want ${awant1}, 6, 3); anchors ${aa0:-?}, ${aa1:-?}, ${aa2:-?} (want 6 or more, 3 more, 6 or more); told lines ${at0} then ${at1}; raw REMOVE: ${araw:-none} caches: ${adrop:-not dropped}; pid ${ap0} -> ${ap1:-none}; macOS: ${aplace} (want guarded/keep/new/rnew/ed); second launch: ${a2}; out: $(printf '%s | %s | %s' "${ao1}" "${ao2}" "${ao3}" | grep -v 'refused:.*Read-only' | tr '\n' ' ' | cut -c1-300)"
fi

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
