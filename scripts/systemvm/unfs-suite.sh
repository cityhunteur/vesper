#!/usr/bin/env bash
# GH #160 M3 / ADR 0009: one sandboxed unfs3 server per shim microVM, inside that microVM's share namespace.
#
# A fixture tree is made in a macOS share; a microVM is launched with a server exporting three sources from it
# (m3/proj rw, m3/proj/conf ro inside it, m3/ro ro). The suite checks the sandbox from outside (ids, capabilities,
# namespaces, mounts), runs vesper-unfs probe (an NFSv3 client) against the server for reads, writes refused where
# read-only, "..", symbolic links and forged handles, runs the escape checks inside a copy of the sandbox, checks
# that a container and another microVM cannot reach the server, kills the server and measures what old handles do
# after supervise-daemon restarts it, refuses a symlinked source, a ".." source and a mount that is not virtio-fs,
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
cleanup() { for i in "${ID}" "${OTHER}" "${SQ}" unfs-x; do mvm cleanup "$i" --logs >/dev/null; done; }
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
  "/s/0/conf /m3/proj/conf ro,nosuid,nodev,noexec" "/ / ro,nosuid,nodev,noexec" "/usr /usr ro,nosuid,nodev" "/lib /lib ro,nosuid,nodev"; do
  printf '%s\n' "${iso}" | grep -q "^UNFSMOUNT ${m}" || mounts_ok=false
done
case "${iso}" in *"${want} seccomp=2 mnt=own pid_ns=own ipc=own uts=own net=share mounts=/,/dev/null,/lib,/s/0,/s/0/conf,/s/1,/s/2,/usr,"*) ;; *) mounts_ok=false ;; esac
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
case "${race}" in *"not the source that was opened"*) record nested_source_swap_refused true "${race}" ;;
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
case "${t1}|${tst}|${tself}|${t2}|${t3}" in "LAUNCHED unfs-x "*"|"*"listen=yes"*"|"*"failed=0"*"|ERR --unfs-src before any --unfs-mount|ERR"*"twice"*)
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

# --- 10. Cleanup leaves nothing ----------------------------------------------------------------------------------
c="$(mvm cleanup "${SQ}" --logs)"; u="$(mvm unfs "${SQ}")"
case "${c}:${u}" in *"CLEAN ${SQ}"*":"*"UNFS ${SQ} absent"*) record cleanup_removes_the_server true "${c}; ${u}" ;;
  *) record cleanup_removes_the_server false "${c}; ${u}" ;; esac

passed=true; for s in "${steps[@]}"; do case "$s" in *'"ok":false'*) passed=false ;; esac; done
printf '{"steps":[%s],"passed":%s}\n' "$(IFS=,; echo "${steps[*]}")" "${passed}" > "${REPORT}"
echo "unfs suite: passed=${passed} (report: ${REPORT})"
[ "${passed}" = true ]
