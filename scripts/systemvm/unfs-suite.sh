#!/usr/bin/env bash
# GH #160 M3 / ADR 0009: one sandboxed unfs3 server per shim microVM, inside that microVM's share namespace.
#
# A fixture tree is made in a macOS share; a microVM is launched with a server exporting three sources from it
# (m3/proj rw, m3/proj/conf ro inside it, m3/ro ro). The suite checks the sandbox from outside (ids, capabilities,
# namespaces, mounts), runs vesper-unfs probe (an NFSv3 client) against the server for reads, writes refused where
# read-only, "..", symbolic links and forged handles, runs the escape checks inside a copy of the sandbox, checks
# that a container and another microVM cannot reach the server, kills the server and measures what old handles do
# after supervise-daemon restarts it, refuses a symlinked source, a ".." source and a mount that is not virtio-fs,
# checks that a launch fails at once on the server's fatal line and that no other line of the server fails it (TD-082),
# repeats the probe and the escape checks with the non-root (all_squash) server, and checks that cleanup leaves
# nothing. Needs `make systemvm-up-shares docker-up`. Report: run/systemvm/unfs-report.json.
set -uo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SHARE="${UNFS_SHARE:-${ROOT_DIR}/run/WOR-39/share}"
REPORT="${ROOT_DIR}/run/systemvm/unfs-report.json"
RAW="${ROOT_DIR}/run/systemvm/unfs-raw"   # the full probe and selfcheck output of each step
CTL=(cargo run -q --bin systemvmctl -- control)
ID=unfs-a
OTHER=unfs-b
SQ=unfs-q
cd "${ROOT_DIR}"; mkdir -p "$(dirname "${REPORT}")" "${RAW}"
steps=()
record() { local d; d="$(printf '%s' "$3" | tr '\n' ' ' | cut -c1-900 | sed 's/\\/\\\\/g; s/"/\\"/g')"; steps+=("{\"name\":\"$1\",\"ok\":$2,\"detail\":\"${d}\"}"); if [ "$2" = true ]; then echo "ok   $1"; else echo "FAIL $1: ${d}"; fi; }
ctl() { "${CTL[@]}" "$@" 2>&1; }
mvm() { ctl microvm -- "$@"; }
upid() { mvm unfs-isolation "$1" | head -1 | sed -n 's/.* pid=\([0-9]*\).*/\1/p'; }
# A forged handle can make unfs3 abort (TD-065); supervise-daemon starts it again within about a second.
wait_listen() { for _ in $(seq 1 40); do case "$(mvm unfs "$1")" in *listen=yes*) return 0 ;; esac; sleep 0.5; done; return 1; }
errline() { grep -m1 '^ERR' || echo "no ERR line"; }
cleanup() { for i in "${ID}" "${OTHER}" "${SQ}" unfs-x unfs-h unfs-d; do mvm cleanup "$i" --logs >/dev/null; done; mvm test-unfs-slow-next none >/dev/null; }
trap cleanup EXIT
ctl share list | grep -q " ${SHARE} virtiofs\| ${SHARE} mode=" || { echo "unfs-suite: ${SHARE} is not a share; run 'make systemvm-up-shares'" >&2; exit 2; }
cleanup

# --- fixture, made from macOS ----------------------------------------------------------------------------------
F="${SHARE}/m3"
rm -rf "${F}"
mkdir -p "${F}/proj/conf" "${F}/proj/dir700" "${F}/ro" "${F}/outside"
echo proj > "${F}/proj/file.txt"; echo s > "${F}/proj/secret600"; chmod 600 "${F}/proj/secret600"
echo inner > "${F}/proj/dir700/inner.txt"; chmod 700 "${F}/proj/dir700"
ln -s /etc "${F}/proj/link-out"; ln -s ../outside "${F}/proj/link-up"; ln -s /dev "${F}/proj/link-dev"; ln -s /etc "${F}/swap"
echo conf > "${F}/proj/conf/app.conf"; echo ro > "${F}/ro/r.txt"; echo secret > "${F}/outside/secret.txt"
# TD-073: links that reach the read-only nested directory, and a file in it, under another letter case.
ln -s CONF "${F}/proj/link-CONF"; ln -s CONF/APP.CONF "${F}/proj/link-FILE"
# Links that reach nothing protected: they must stay usable (a link to nowhere, and one to a name not made yet).
ln -s /nowhere/at/all "${F}/proj/link-nowhere"; ln -s made-through-link "${F}/proj/link-new"
SRCS=(--unfs-mount "${SHARE}" --unfs-src m3/proj:rw --unfs-src m3/proj/conf:ro --unfs-src m3/ro:ro)
# TD-082: a read-only source three directories deep in a read-write one (three anchors), and directories beside it.
X="${SHARE}/m3x"; rm -rf "${SHARE:?}/m3x"
mkdir -p "${X}/proj/an/in/conf" "${X}/proj/free/sub" "${X}/proj/other"; echo c > "${X}/proj/an/in/conf/c.txt"; echo w > "${X}/proj/w.txt"
# TD-082: 63 read-only sources, each 16 directories deep in one read-write source: 1,008 anchors, of the 1,024 that
# one server can have.
D="${SHARE}/m3d"; rm -rf "${SHARE:?}/m3d"; DEEP=1/2/3/4/5/6/7/8/9/10/11/12/13/14/conf
DSRC=(--unfs-mount "${SHARE}" --unfs-src m3d/proj:rw)
for i in $(seq -w 1 63); do mkdir -p "${D}/proj/c${i}/${DEEP}"; DSRC+=(--unfs-src "m3d/proj/c${i}/${DEEP}:ro"); done

# --- 1. A microVM with a server ---------------------------------------------------------------------------------
launched="$(mvm launch "${ID}" --share-net "${SRCS[@]}" | tail -1)"
state="$(mvm unfs "${ID}")"
case "${launched}:${state}" in *"LAUNCHED ${ID} "*":"*"state=running"*"listen=yes"*) record server_starts_with_the_microvm true "${launched}; ${state}" ;;
  *) record server_starts_with_the_microvm false "${launched}; ${state}; log: $(mvm unfs-log "${ID}" 10)" ;; esac

# --- 2. The sandbox, seen from outside --------------------------------------------------------------------------
iso="$(mvm unfs-isolation "${ID}")"
want="uid=0 gid=0 capbnd=00000000000000db capeff=00000000000000db capamb=0000000000000000 nonewprivs=1"
mounts_ok=true
for m in "/s/0 /m3/proj rw,nosuid,nodev,noexec" "/s/1 /m3/proj/conf ro,nosuid,nodev,noexec" "/s/2 /m3/ro ro,nosuid,nodev,noexec" \
  "/s/0/conf /m3/proj/conf ro,nosuid,nodev,noexec" "/etc/exports.protect /vesper/microvm/unfs/${ID}.prot.list ro,nosuid,nodev,noexec" "/ / ro,nosuid,nodev,noexec" "/usr /usr ro,nosuid,nodev" "/lib /lib ro,nosuid,nodev"; do
  printf '%s\n' "${iso}" | grep -q "^UNFSMOUNT ${m}" || mounts_ok=false
done
case "${iso}" in *"${want} seccomp=2 mnt=own pid_ns=own ipc=own uts=own net=share mounts=/,/dev/null,/etc/exports.protect,/lib,/s/0,/s/0/conf,/s/1,/s/2,/usr,"*) ;; *) mounts_ok=false ;; esac
record sandbox_holds_only_the_exports "${mounts_ok}" "${iso}"

# --- 3. The probe: reads, read-only refusals, "..", symbolic links, forged handles --------------------------------
p0="$(upid "${ID}")"
out="$(mvm test-unfs-probe "${ID}")"; printf '%s\n' "${out}" > "${RAW}/probe-root.txt"
wait_listen "${ID}"; p1="$(upid "${ID}")"
restarted="no"; [ "${p0}" != "${p1}" ] && restarted="yes (unfsd ${p0} -> ${p1})"
sum="$(printf '%s\n' "${out}" | grep '^PROBE-SUMMARY')"
# The probe tried to change the read-only nested directory through links (TD-073): on macOS it must be as it was.
intact="$(cat "${F}/proj/conf/app.conf" 2>/dev/null):$(ls "${F}/proj/conf" | tr '\n' ' ')"
[ "${intact}" = "conf:app.conf " ] || sum="the read-only directory changed on macOS (${intact}); $(printf '%s' "${sum}" | sed 's/failed=0/failed=1-on-macOS/')"
case "${sum}" in *"failed=0"*) record probe_as_root_finds_nothing_outside true "${sum}; server restarted during the probe: ${restarted}; $(printf '%s\n' "${out}" | grep -E '^INFO|forged' | tr '\n' ';')" ;;
  *) record probe_as_root_finds_nothing_outside false "$(printf '%s\n' "${out}" | grep -E 'FAIL|SUMMARY' | tr '\n' ';')" ;; esac
# Apple's virtio-fs reports a file's owner as the caller, and the System VM caches that for about a second, so a
# caller sees the last one's uid until then (TD-038). The root probe above used to end with the server's restart
# (TD-065), which waited out the cache; the patched server does not restart (D-044), so the step waits.
sleep 3
out="$(mvm test-unfs-probe "${ID}" --uid 1000 --gid 1000)"; printf '%s\n' "${out}" > "${RAW}/probe-uid1000.txt"
wait_listen "${ID}"
sum="$(printf '%s\n' "${out}" | grep '^PROBE-SUMMARY')"; own="$(printf '%s\n' "${out}" | grep '^INFO read-0600')"
case "${sum}:${own}" in *"failed=0"*":"*"uid=1000 gid=1000"*) record probe_as_uid_1000_sees_its_own_files true "${sum}; ${own}" ;;
  *) record probe_as_uid_1000_sees_its_own_files false "${sum}; ${own}; $(printf '%s\n' "${out}" | grep FAIL | tr '\n' ';')" ;; esac

# --- 4. The escape checks, inside a copy of the sandbox --------------------------------------------------------
out="$(mvm test-unfs-selfcheck "${ID}")"; printf '%s\n' "${out}" > "${RAW}/selfcheck-root.txt"
sum="$(printf '%s\n' "${out}" | grep '^SELF-SUMMARY')"
case "${sum}" in *"failed=0"*) record escape_checks_fail_as_root true "${sum}; $(printf '%s\n' "${out}" | grep -E 'INFO|userns|chroot' | tr '\n' ';')" ;;
  *) record escape_checks_fail_as_root false "$(printf '%s\n' "${out}" | grep -E 'FAIL|SUMMARY|INFO|vesper-unfs' | tr '\n' ';')" ;; esac

# --- 4b. A nested read-only source renamed and replaced before its bind is placed: the sandbox refuses to start --
race_out="$(mktemp)"
( mvm test-unfs-selfcheck "${ID}" 4000 > "${race_out}" 2>&1 ) &
rp=$!
sleep 1.5
mv "${F}/proj/conf" "${F}/proj/conf-old" && mkdir "${F}/proj/conf" && echo decoy > "${F}/proj/conf/app.conf"
wait "${rp}"
race="$(tr '\n' ' ' < "${race_out}")"; rm -f "${race_out}"
rm -rf "${F}/proj/conf"; mv "${F}/proj/conf-old" "${F}/proj/conf"
case "${race}" in *"vesper-unfs fatal: init: "*"not the source that was opened"*) record nested_source_swap_refused true "${race}" ;;
  *) record nested_source_swap_refused false "${race}" ;; esac

# --- 5. Nothing outside the microVM reaches the server ---------------------------------------------------------
other="$(mvm launch "${OTHER}" --share-net | tail -1)"
reach="$(mvm test-share-reach 2049 "${OTHER}")"
case "${other}:${reach}" in *"LAUNCHED ${OTHER} "*":"*"REACH root closed"*"REACH namespace:${OTHER} closed"*"REACH container-bridge closed"*"REACH container-host refused"*)
  record server_closed_to_containers_and_other_microvms true "$(printf '%s' "${reach}" | tr '\n' ';')" ;;
  *) record server_closed_to_containers_and_other_microvms false "${other}; $(printf '%s' "${reach}" | tr '\n' ';')" ;; esac
mvm cleanup "${OTHER}" --logs >/dev/null

# --- 6. A killed server comes back; what handles from before do ----------------------------------------------------
saved="$(mvm test-unfs-probe "${ID}" --save 2>&1)"
before="$(mvm unfs-isolation "${ID}" | head -1 | sed 's/.* pid=\([0-9]*\).*/\1/')"
killed="$(mvm test-unfs-kill "${ID}")"
back=""; for _ in $(seq 1 40); do s="$(mvm unfs "${ID}")"; case "${s}" in *listen=yes*) after="$(mvm unfs-isolation "${ID}" | head -1 | sed 's/.* pid=\([0-9]*\).*/\1/')"; [ -n "${after}" ] && [ "${after}" != "${before}" ] && { back="${s}"; break; } ;; esac; sleep 0.5; done
replay="$(mvm test-unfs-probe "${ID}" --replay 2>&1 | tr '\n' ';')"
if [ -n "${back}" ] && case "${killed}" in KILLED*) true ;; *) false ;; esac && case "${replay}" in *"HANDLE root "*"HANDLE file "*) true ;; *) false ;; esac; then
  record server_restarts_and_old_handles_measured true "${killed}; back as pid ${after}: ${back}; old handles: ${replay}"
else
  record server_restarts_and_old_handles_measured false "save: ${saved}; ${killed}; back: ${back:-no}; replay: ${replay}"
fi

# --- 7. Memory ---------------------------------------------------------------------------------------------------
mem="$(mvm unfs "${ID}")"
peak="$(printf '%s' "${mem}" | sed -n 's/.*mem_peak=\([0-9]*\).*/\1/p')"; max="$(printf '%s' "${mem}" | sed -n 's/.*mem_max=\([0-9]*\).*/\1/p')"
[ -n "${peak}" ] && [ -n "${max}" ] && [ "${peak}" -lt "${max}" ] && case "${mem}" in *oom_kills=0*) true ;; *) false ;; esac \
  && record server_memory_within_its_cap true "${mem}" || record server_memory_within_its_cap false "${mem}"

# --- 8. Sources that must be refused ---------------------------------------------------------------------------
r1="$(mvm launch unfs-x --share-net --unfs-mount "${SHARE}" --unfs-src m3/swap:rw | errline)"; mvm cleanup unfs-x --logs >/dev/null
r2="$(mvm launch unfs-x --share-net --unfs-mount "${SHARE}" --unfs-src m3/proj/../outside:ro | errline)"; mvm cleanup unfs-x --logs >/dev/null
r3="$(mvm launch unfs-x --share-net --unfs-mount /run --unfs-src .:rw | errline)"; mvm cleanup unfs-x --logs >/dev/null
case "${r1}|${r2}|${r3}" in *"m3/swap"*"symbolic links"*"|ERR"*"|"*"not a virtio-fs mount"*) record bad_sources_refused true "symlink: ${r1}; dotdot: ${r2}; not virtio-fs: ${r3}" ;;
  *) record bad_sources_refused false "symlink: ${r1}; dotdot: ${r2}; not virtio-fs: ${r3}" ;; esac

# --- 8a. What a launch takes for a failed start (TD-082) --------------------------------------------------------
# A run of the supervisor that ends with an error says so in a line that begins "vesper-unfs fatal:", and the launch
# gives up when it reads that line; it waits 10 s for a server that says nothing. 8 s is less than that wait, which a
# fatal line under another prefix would cost. The time is taken around the whole command, in whole seconds.
f0="$(date +%s)"; fr="$(mvm launch unfs-x --share-net --unfs-mount "${SHARE}" --unfs-src m3/swap:rw | errline)"; fsec=$(( $(date +%s) - f0 ))
mvm cleanup unfs-x --logs >/dev/null
case "${fr}" in
  *'log: vesper-unfs fatal: run: source "m3/swap"'*)
    if [ "${fsec}" -lt 8 ]; then record failed_start_fails_the_launch_at_once true "refused in ${fsec} s: $(printf '%s' "${fr}" | cut -c1-160)"
    else record failed_start_fails_the_launch_at_once false "the refusal took ${fsec} s (want less than 8): $(printf '%s' "${fr}" | cut -c1-300)"; fi ;;
  *) record failed_start_fails_the_launch_at_once false "refused (${fsec} s) without the fatal line: $(printf '%s' "${fr}" | cut -c1-300)" ;;
esac
# No other line fails a launch: the hook makes the next server listen 1.5 s late and say so in a line that begins as
# the supervisor's warnings do ("vesper-unfs run:"). Before TD-082 that line ended the wait.
slow="$(mvm test-unfs-slow-next unfs-x | tail -1)"
s0="$(date +%s)"; sout="$(mvm launch unfs-x --share-net --unfs-mount "${SHARE}" --unfs-src m3/ro:ro)"; ssec=$(( $(date +%s) - s0 ))
sl="$(printf '%s\n' "${sout}" | grep -m1 '^LAUNCHED' || printf '%s\n' "${sout}" | errline)"
sst="$(mvm unfs unfs-x)"; sfull="$(mvm unfs-log unfs-x 20 | grep -a '^vesper-unfs' | tr '\n' ';')"
mvm cleanup unfs-x --logs >/dev/null; mvm test-unfs-slow-next none >/dev/null
case "${slow}|${sl}|${sst}|${sfull}" in
  "SLOW next unfs-x|LAUNCHED unfs-x "*"|"*"listen=yes"*"|"*"vesper-unfs run: started with --test-pause-nests"*)
    record warning_line_does_not_fail_a_launch true "${sl} after ${ssec} s; ${sst}; the supervisor's lines: ${sfull}" ;;
  *) record warning_line_does_not_fail_a_launch false "hook: ${slow}; launch (${ssec} s): $(printf '%s' "${sl}" | cut -c1-330); ${sst}; the supervisor's lines: ${sfull}" ;;
esac

# --- 8b. One server for two shares (GH #207): the same relative path in each is two exports; the escape checks pass
# in the two-mount sandbox, started from the same record; malformed requests are refused --------------------------
SHARE2="${UNFS_SHARE2:-$(dirname "${SHARE}")/share-nowatch}"
# The escape checks expect the fixture's layout (/s/1 nested read-only at /s/0/conf, /s/2 read-only): here /s/2 is
# m3/ro of the second share, so the same relative path names a directory in each share.
rm -rf "${SHARE2}/m3"; mkdir -p "${SHARE2}/m3/ro"; echo second > "${SHARE2}/m3/ro/r.txt"
t1="$(mvm launch unfs-x --share-net --unfs-mount "${SHARE}" --unfs-src m3/proj:rw --unfs-src m3/proj/conf:ro --unfs-mount "${SHARE2}" --unfs-src m3/ro:ro | tail -1)"
tst="$(mvm unfs unfs-x)"; tiso="$(mvm unfs-isolation unfs-x | grep -c ' /s/')"
tself="$(mvm test-unfs-selfcheck unfs-x | grep '^SELF-SUMMARY')"
mvm cleanup unfs-x --logs >/dev/null
t2="$(mvm launch unfs-x --share-net --unfs-src m3/proj:rw --unfs-mount "${SHARE}" | errline)"; mvm cleanup unfs-x --logs >/dev/null
t3="$(mvm launch unfs-x --share-net --unfs-mount "${SHARE}" --unfs-src m3/proj:rw --unfs-mount "${SHARE}" --unfs-src m3/ro:ro | errline)"; mvm cleanup unfs-x --logs >/dev/null
case "${t1}|${tst}|${tself}|${t2}|${t3}" in "LAUNCHED unfs-x "*"|"*"listen=yes"*"|"*"failed=0"*"|ERR --unfs-src before any --unfs-mount|ERR"*"log: vesper-unfs fatal: run: --args: "*"twice"*)
  record two_shares_one_server true "${t1}; ${tst}; mounts under /s: ${tiso}; selfcheck ${tself}; src first: ${t2}; same mount twice: ${t3}" ;;
  *) record two_shares_one_server false "${t1}; ${tst}; selfcheck ${tself}; src first: ${t2}; same mount twice: ${t3}" ;; esac

# --- 9. The non-root server (all_squash): the same checks -------------------------------------------------------
mvm cleanup "${ID}" --logs >/dev/null
q="$(mvm launch "${SQ}" --share-net --unfs-squash "${SRCS[@]}" | tail -1)"
qiso="$(mvm unfs-isolation "${SQ}" | head -1)"
qp="$(mvm test-unfs-probe "${SQ}" --uid 1000 --gid 1000)"; wait_listen "${SQ}"
qs="$(mvm test-unfs-selfcheck "${SQ}")"; printf '%s\n' "${qp}" > "${RAW}/probe-squash.txt"; printf '%s\n' "${qs}" > "${RAW}/selfcheck-squash.txt"
qsum="$(printf '%s\n' "${qp}" | grep '^PROBE-SUMMARY')"; qown="$(printf '%s\n' "${qp}" | grep '^INFO read-0600')"
qself="$(printf '%s\n' "${qs}" | grep '^SELF-SUMMARY')"
case "${q}:${qiso}:${qsum}:${qself}" in *"LAUNCHED ${SQ} "*":"*"uid=65534 gid=65534 capbnd=0000000000000000 capeff=0000000000000000"*"nonewprivs=1 seccomp=2"*":"*"failed=0"*":"*"failed=0"*)
  record squash_server_confined_and_serving true "${qiso}; probe ${qsum}; ${qown}; selfcheck ${qself}" ;;
  *) record squash_server_confined_and_serving false "${q}; ${qiso}; ${qsum}; ${qself}; $(printf '%s\n%s\n' "${qp}" "${qs}" | grep FAIL | tr '\n' ';')" ;; esac

# --- 9b. A request held unfinished is judged with the list as it is when it ends (TD-078, TD-079). A task can send
# a request to its own server and keep back the last bytes; the single-threaded server then waits and looks at
# nothing. Here a raw client sends a REMOVE of "hz/secret~", but for its last bytes, while that name does not exist
# yet. macOS then saves the read-only file hz/secret twice as an editor with a backup does, so the file that was
# the read-only file between the saves becomes hz/secret~. The supervisor tells the server's list of each new file
# before it attaches it. The client then sends the rest. The server must read its list inside that request's own
# check, after the arguments arrived: its look at the start of the request was before both saves. A second held
# request, an UNCHECKED CREATE of the same name across two more saves, does the same for the procedures that are
# checked in one place only (a REMOVE is checked in two). The client makes a marker file when its request is held,
# and the saves wait for it; after each held request it makes and removes a file of its own, which must go through.
HF="${F}/hold"; mkdir -p "${HF}/hz"; echo v0 > "${HF}/hz/secret"
hl="$(mvm launch unfs-h --share-net --unfs-mount "${SHARE}" --unfs-src m3/hold:rw --unfs-src m3/hold/hz/secret:rof | tail -1)"
hatt() { mvm unfs-log unfs-h 80 | grep -a -c 'another file is at its path'; }
hsave() { echo "$1" > "${HF}/hz/new"; mv "${HF}/hz/secret" "${HF}/hz/secret~"; mv "${HF}/hz/new" "${HF}/hz/secret"; }
# held KIND TAG V1 V2: a held request across two saves; prints "held-in-time attaches" and leaves the probe's output
held() {
  local before a1 a2 intime=no
  ( mvm test-unfs-probe unfs-h "--hold-$1" 'hz/secret~' --until "${HF}/go-$2" --held-marker "${HF}/held-$2" > "${RAW}/held-$2.txt" 2>&1 ) &
  local pid=$!
  for _ in $(seq 1 60); do [ -e "${HF}/held-$2" ] && { intime=yes; break; }; sleep 0.25; done
  before="$(hatt)"; hsave "$3"
  a1="${before}"; for _ in $(seq 1 30); do a1="$(hatt)"; [ "${a1}" -gt "${before}" ] && break; sleep 0.25; done
  hsave "$4"
  a2="${a1}"; for _ in $(seq 1 30); do a2="$(hatt)"; [ "${a2}" -gt "${a1}" ] && break; sleep 0.25; done
  touch "${HF}/go-$2"; wait "${pid}"
  [ "${a1}" -gt "${before}" ] && [ "${a2}" -gt "${a1}" ] && echo "${intime} two-attaches" || echo "${intime} attaches-${before}-${a1}-${a2}"
}
h1="$(held remove r v1 v2)"; hout1="$(cat "${RAW}/held-r.txt" 2>/dev/null | tr '\n' ';')"; hback1="$(cat "${HF}/hz/secret~" 2>/dev/null | tr '\n' ' ')"
h2="$(held create c v3 v4)"; hout2="$(cat "${RAW}/held-c.txt" 2>/dev/null | tr '\n' ';')"; hback2="$(cat "${HF}/hz/secret~" 2>/dev/null | tr '\n' ' ')"
hlist="$(mvm unfs-list unfs-h | head -1)"
case "${hl}|${h1}|${hout1}|${h2}|${hout2}" in
  "LAUNCHED unfs-h "*"|yes two-attaches|HELD sent remove hz/secret~ (absent);HELD remove hz/secret~ status=ROFS;HELD control status=OK err=<nil>;"*"|yes two-attaches|HELD sent create hz/secret~ (present);HELD create hz/secret~ status=ROFS;HELD control status=OK err=<nil>;"*)
    if [ "${hback1}" = "v1 " ] && [ "${hback2}" = "v3 " ]; then
      record held_request_judged_with_the_list_as_it_is_then true "a REMOVE of hz/secret~, held from before the first save, was refused as read-only when it ended after two saves, and macOS still read '${hback1}' in the backup; an UNCHECKED CREATE of it held across two more saves was refused too (by its status: such a create keeps a file's data, so the backup, '${hback2}', shows nothing); the client's own file went through each time (${hlist})"
    else
      record held_request_judged_with_the_list_as_it_is_then false "macOS hz/secret~ after the held remove '${hback1}' (want 'v1 '), after the held create '${hback2}' (want 'v3 '); ${hlist}; remove: ${hout1} create: ${hout2}"
    fi ;;
  *) record held_request_judged_with_the_list_as_it_is_then false "${hl}; remove: ${h1}: ${hout1} create: ${h2}: ${hout2} ${hlist}; macOS hz/secret~ '${hback1}' then '${hback2}'" ;;
esac
mvm cleanup unfs-h --logs >/dev/null

# --- 9c. Follow-ups of the anchors (TD-082) ---------------------------------------------------------------------
# A request whose name is "." or ".." names a directory too: the server refuses it when that directory is an anchor,
# and carries it out when it is not (the control), so the refusals are the anchor rule's.
probe1() { mvm test-unfs-probe unfs-x "$@" | tail -1; }
al="$(mvm launch unfs-x --share-net --unfs-mount "${SHARE}" --unfs-src m3x/proj:rw --unfs-src m3x/proj/an/in/conf:ro | tail -1)"
alist="$(mvm unfs-list unfs-x)"
d1="$(probe1 --remove an/.)"; d2="$(probe1 --rmdir an/.)"; d3="$(probe1 --rmdir an/in/..)"
d4="$(probe1 --rename an/. --to moved)"; d5="$(probe1 --rename an/in/.. --to moved)"; d6="$(probe1 --rename other --to an/in/..)"
dc="$(probe1 --rename free/sub/.. --to free2)"
for _ in $(seq 1 10); do [ -d "${X}/proj/free2/sub" ] && break; sleep 0.5; done
dstate="an:$([ -f "${X}/proj/an/in/conf/c.txt" ] && echo kept || echo GONE) moved:$([ -e "${X}/proj/moved" ] && echo MADE || echo absent) other:$([ -d "${X}/proj/other" ] && echo kept || echo GONE) free2:$([ -d "${X}/proj/free2/sub" ] && echo made || echo absent)"
case "${al}|${alist}|${d1}|${d2}|${d3}|${d4}|${d5}|${d6}|${dc}|${dstate}" in
  "LAUNCHED unfs-x "*"|"*" a=3 "*"|REMOVE an/. status=ROFS|RMDIR an/. status=ROFS|RMDIR an/in/.. status=ROFS|RENAME an/. moved status=ROFS|RENAME an/in/.. moved status=ROFS|RENAME other an/in/.. status=ROFS|RENAME free/sub/.. free2 status=OK|an:kept moved:absent other:kept free2:made")
    record dot_names_do_not_move_an_anchor true "${alist}; ${d1}; ${d2}; ${d3}; ${d4}; ${d5}; ${d6}; control: ${dc}; macOS: ${dstate}" ;;
  *) record dot_names_do_not_move_an_anchor false "${al}; ${alist}; ${d1}; ${d2}; ${d3}; ${d4}; ${d5}; ${d6}; control: ${dc}; macOS: ${dstate}; log: $(mvm unfs-log unfs-x 6)" ;;
esac

# macOS puts a symbolic link where a directory of the chain was. The chain cannot be walked: the supervisor says so
# once, and the server keeps what it was last told, so the directory that was on the chain stays an anchor under its
# new name, and so does the one below it. The link itself is no anchor (a stated limit of D-058).
mv "${X}/proj/an" "${X}/proj/an.real"; ln -s an.real "${X}/proj/an"
lk=no; for _ in $(seq 1 20); do mvm unfs-log unfs-x 40 | grep -q 'm3x/proj/an/in/conf.*cannot be looked at now' && { lk=yes; break; }; sleep 0.5; done
l1="$(probe1 --rename an.real --to zz)"; l2="$(probe1 --rename an.real/in --to zz)"
sleep 3; lcount="$(mvm unfs-log unfs-x 80 | grep -c 'cannot be looked at now')"
rm "${X}/proj/an"; mv "${X}/proj/an.real" "${X}/proj/an"; sleep 2
ltold="$(mvm unfs-log unfs-x 80 | grep -c 'was replaced; the server was told')"
case "${lk}|${l1}|${l2}|${lcount}|${ltold}" in
  "yes|RENAME an.real zz status=ROFS|RENAME an.real/in zz status=ROFS|1|0")
    record link_on_the_way_keeps_the_last_chain true "the supervisor said once that the chain cannot be looked at; ${l1}; ${l2}; nothing was told for the link, nor when the directory was back" ;;
  *) record link_on_the_way_keeps_the_last_chain false "said: ${lk}, ${lcount} times; ${l1}; ${l2}; told ${ltold} times; log: $(mvm unfs-log unfs-x 8)" ;;
esac

# The server remembers its newest 2,048 anchors. With the 3 of this server and 2,045 made-up ones all are known; one
# more, and the one told longest ago goes: the first directory of the chain can then be renamed (the stated limit;
# the supervisor never tells more than 1,024, and tells all of them again whenever one changed). Told again, an
# anchor is the newest.
m1="$(mvm test-unfs-list-anchors unfs-x 2045 | tail -1)"; a1="$(probe1 --rename an --to an-x)"
m2="$(mvm test-unfs-list-anchors unfs-x 1 | tail -1)"; a2="$(probe1 --rename an --to an-x)"; a2b="$(probe1 --rename an-x --to an)"
m3="$(mvm test-unfs-list-anchors unfs-x 0 again | tail -1)"; a3="$(probe1 --rename an --to an-x)"
case "${m1}|${a1}|${m2}|${a2}|${a2b}|${m3}|${a3}" in
  "ANCHORS unfs-x made-up=2045 real=3 again=no|RENAME an an-x status=ROFS|ANCHORS unfs-x made-up=2046 real=3 again=no|RENAME an an-x status=OK|RENAME an-x an status=OK|ANCHORS unfs-x made-up=2046 real=3 again=again|RENAME an an-x status=ROFS")
    record server_keeps_its_newest_2048_anchors true "2,048 told: ${a1}; 2,049 told: ${a2} (and back: ${a2b}); the 3 told again: ${a3}" ;;
  *) record server_keeps_its_newest_2048_anchors false "${m1}; ${a1}; ${m2}; ${a2}; ${a2b}; ${m3}; ${a3}" ;;
esac
mvm cleanup unfs-x --logs >/dev/null

# The supervisor's open files, with 63 read-only sources that are each 16 directories deep in a read-write one. It
# holds a descriptor of each anchor, and for a moment two sets of them when macOS replaced a directory: twice what
# it has open must fit its limit, and after the change it has as many open as before.
fl="$(mvm launch unfs-d --share-net "${DSRC[@]}" | tail -1)"
flist="$(mvm unfs-list unfs-d)"; fsup="$(mvm unfs-isolation unfs-d | grep '^UNFSSUP')"
mv "${D}/proj/c01/1" "${D}/proj/c01/1.old"; mkdir -p "${D}/proj/c01/${DEEP}"
ftold=no; for _ in $(seq 1 30); do mvm unfs-log unfs-d 40 | grep -q 'the server was told of the 1008 directories now on the way' && { ftold=yes; break; }; sleep 0.5; done
fsup2="$(mvm unfs-isolation unfs-d | grep '^UNFSSUP')"; ferr="$(mvm unfs-log unfs-d 120 | grep -c 'too many open files')"
soft="$(printf '%s' "${fsup}" | sed -n 's/.* nofile=\([0-9]*\)\/.*/\1/p')"; fds="$(printf '%s' "${fsup}" | sed -n 's/.* fds=\([0-9]*\).*/\1/p')"; fds2="$(printf '%s' "${fsup2}" | sed -n 's/.* fds=\([0-9]*\).*/\1/p')"
case "${fl}|${flist}" in "LAUNCHED unfs-d "*"|"*" a=1008 "*) fok=true ;; *) fok=false ;; esac
[ "${fok}" = true ] && [ "${ftold}" = yes ] && [ "${ferr}" = 0 ] && [ -n "${soft}" ] && [ -n "${fds}" ] && [ "${fds}" -ge 1008 ] && [ $((fds * 2)) -lt "${soft}" ] && [ "${fds2}" = "${fds}" ] \
  && record supervisor_open_files_fit_its_limit true "64 sources, ${flist}; ${fsup}; after macOS replaced a directory the server was told of all 1,008 again and the supervisor has ${fds2} open; twice ${fds} is under the limit of ${soft}" \
  || record supervisor_open_files_fit_its_limit false "${fl}; ${flist}; ${fsup}; told again: ${ftold}; then ${fsup2}; 'too many open files' in the log: ${ferr}; log: $(mvm unfs-log unfs-d 6)"
mvm cleanup unfs-d --logs >/dev/null

# Two binds that spell one directory in two ways (a macOS volume that ignores letter case): the read-only one gets
# its mount on its name inside the read-write one, as with one spelling, and a directory bound both ways stays
# writable through its read-write bind.
if [ ! -d "${SHARE}/M3X/PROJ" ]; then
  record two_spellings_of_one_directory true "not run: this volume keeps letter case apart, so the two spellings are two names"
else
  sp1="$(mvm launch unfs-x --share-net --unfs-mount "${SHARE}" --unfs-src m3x/proj:rw --unfs-src M3X/PROJ/an/in/conf:ro | tail -1)"
  spn="$(mvm unfs-isolation unfs-x | grep -c '^UNFSMOUNT /s/0/an/in/conf .* ro,')"
  splog="$(mvm unfs-log unfs-x 30 | grep -c 'under another spelling of the path; it is taken as "m3x/proj/an/in/conf"')"
  spro="$(probe1 --remove an/in/conf/c.txt)"
  mvm cleanup unfs-x --logs >/dev/null
  sp2="$(mvm launch unfs-x --share-net --unfs-mount "${SHARE}" --unfs-src m3x/proj:rw --unfs-src M3X/PROJ:ro | tail -1)"
  spl="$(mvm unfs-list unfs-x)"; spw="$(probe1 --remove w.txt)"
  for _ in $(seq 1 10); do [ ! -e "${X}/proj/w.txt" ] && break; sleep 0.5; done
  spgone="$([ -e "${X}/proj/w.txt" ] && echo still-there || echo removed)"
  mvm cleanup unfs-x --logs >/dev/null
  case "${sp1}|${spn}|${splog}|${spro}|${sp2}|${spl}|${spw}|${spgone}" in
    "LAUNCHED unfs-x "*"|1|1|REMOVE an/in/conf/c.txt status=ROFS|LAUNCHED unfs-x "*"|"*" p=0 i=0 a=0 s=0|REMOVE w.txt status=OK|removed")
      record two_spellings_of_one_directory true "read-only bind under another letter case: its mount is on its name in the read-write export, the supervisor's log names the spelling taken, ${spro}; the same directory both ways: ${spl}, ${spw}" ;;
    *) record two_spellings_of_one_directory false "${sp1}; nests ${spn}; log lines ${splog}; ${spro}; ${sp2}; ${spl}; ${spw}; w.txt ${spgone}" ;;
  esac
fi

# --- 10. Cleanup leaves nothing ----------------------------------------------------------------------------------
c="$(mvm cleanup "${SQ}" --logs)"; u="$(mvm unfs "${SQ}")"
case "${c}:${u}" in *"CLEAN ${SQ}"*":"*"UNFS ${SQ} absent"*) record cleanup_removes_the_server true "${c}; ${u}" ;;
  *) record cleanup_removes_the_server false "${c}; ${u}" ;; esac

passed=true; for s in "${steps[@]}"; do case "$s" in *'"ok":false'*) passed=false ;; esac; done
printf '{"steps":[%s],"passed":%s}\n' "$(IFS=,; echo "${steps[*]}")" "${passed}" > "${REPORT}"
echo "unfs suite: passed=${passed} (report: ${REPORT})"
[ "${passed}" = true ]
