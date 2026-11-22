#!/usr/bin/env bash
# TD-048 / D-051: a containerd task on the Vesper shim gets a microVM sized by its own limits.
#
# Through `ctr run --runtime io.containerd.vesper.v2` (vesper-ctr run-binds): a task with no limit keeps the default
# machine (1 vCPU, 256 MiB, a 352 MiB ceiling) and no limit in its cgroup; a memory limit gives a guest in which the
# workload has at least that much memory, the limit in the workload's cgroup, an allocation under it that succeeds
# and one over it that is killed; a CPU quota gives that many vCPUs and the quota in the cgroup; a CPU set gives its
# count; a workload that lifts its own quota inside the guest is still held by the microVM's cgroup in the System VM;
# a quota of more CPUs than the System VM has runs on the CPUs there are, while a CPU set of more, and more memory
# than the shared budget holds, are refused by name with nothing left behind. `vesper-microvm launch --snapshot-room`
# (TD-085) gives a ceiling and a booking one guest-size larger, and is refused with a rootfs.
# Needs `make systemvm-up` with at least 2 CPUs and 1.5 GiB of free memory budget (the default System VM has them).
# Report: run/systemvm/microvm-size-report.json.
set -uo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
REPORT="${ROOT_DIR}/run/systemvm/microvm-size-report.json"
CTL=(cargo run -q --bin systemvmctl -- control)
MIB=1048576
NAMES="size-a size-b size-c size-d size-e size-f size-g size-h size-i size-j size-k size-l size-m size-n size-o"
cd "${ROOT_DIR}"; mkdir -p "$(dirname "${REPORT}")"
steps=()
record() { local d; d="$(printf '%s' "$3" | tr '\n' ' ' | cut -c1-900 | sed 's/\\/\\\\/g; s/"/\\"/g')"; steps+=("{\"name\":\"$1\",\"ok\":$2,\"detail\":\"${d}\"}"); if [ "$2" = true ]; then echo "ok   $1"; else echo "FAIL $1: $3"; fi; }
ctl() { "${CTL[@]}" "$@" 2>&1; }
runb() { ctl ctr -- run-binds "$@"; }
budget() { ctl vesperd mem-budget | sed -n "s/.* $1=\([0-9]*\).*/\1/p" | head -1; }
field() { printf '%s' "$1" | tr ' ' '\n' | sed -n "s/^$2=//p" | head -1; }
# Whatever ends the suite, its tasks go: a detached one would hold its microVM and its share of the memory budget.
sweep() { local n; ctl microvm cleanup size-s >/dev/null; for n in ${NAMES}; do case "$(ctl ctr -- state "${n}")" in STATE*) ctl ctr -- kill "${n}" 20 >/dev/null; ctl ctr -- delete "${n}" >/dev/null ;; esac; done; }
trap sweep EXIT
# What the workload sees: its CPUs, the guest's memory and what of it is available (MiB), and its cgroup's limits.
VIEW='echo "cpus=$(nproc) mem=$(awk "/^MemTotal:/{print int(\$2/1024)}" /proc/meminfo) avail=$(awk "/^MemAvailable:/{print int(\$2/1024)}" /proc/meminfo) max=$(cat /sys/fs/cgroup/workload/memory.max) cpu=$(cat /sys/fs/cgroup/workload/cpu.max | tr " " /)"'
# machine NAME [limit options]: start a task that sleeps, read its machine from the System VM's side, remove it.
machine() {
  local name="$1"; shift
  local st m k d
  st="$(runb "$@" --detach "${name}" 'sleep 120')"
  m="$(ctl microvm machine "${name}" | head -1)"
  k="$(ctl ctr -- kill "${name}" 20 | head -1)"; d="$(ctl ctr -- delete "${name}" | head -1)"
  case "${k}:${d}" in "KILLED ${name}:DELETED ${name}") echo "${st}; ${m}" ;; *) echo "${st}; ${m}; NOT-REMOVED ${k}; ${d}" ;; esac
}
between() { [ -n "$1" ] && [ "$1" -ge "$2" ] 2>/dev/null && [ "$1" -le "$3" ]; }

ctl ctr -- import >/dev/null
sweep   # what an interrupted run left
CPUS="$(field "$(ctl microvm limits | head -1)" cpus)"
base="$(budget committed)"; room=$(( ($(budget budget) - base) / MIB ))
# Step 4 asks for one CPU more than the System VM has, which must still be a CPU count a microVM can have (32).
if ! between "${CPUS}" 2 31 || [ "${room}" -lt 1500 ]; then
  echo "microvm-size-suite: needs a System VM with 2 to 31 CPUs and 1500 MiB of free memory budget; it has ${CPUS:-?} CPUs and ${room} MiB (see VESPER_SYSTEMVM_CPUS and VESPER_SYSTEMVM_MEMORY_MIB)" >&2
  exit 2
fi

# --- 1. No limit: the machine and the cgroup are as before ---------------------------------------------------------
# No CPU limit on the microVM's cgroup reads "none" until some launch has set one, and "max" after: the jailer
# turns the cpu controller on for the first launch with a quota, and it stays on.
g="$(runb size-a "${VIEW}")"; m="$(machine size-b)"
if [ "$(field "${g}" cpus)" = 1 ] && between "$(field "${g}" mem)" 200 256 && [ "$(field "${g}" max)" = max ] && [ "$(field "${g}" cpu)" = max/100000 ] \
  && case "${m}" in *NOT-REMOVED*) false ;; *"MACHINE size-b vcpus=1 mem_max=$((352 * MIB)) "*" cpu_max=none "*|*"MACHINE size-b vcpus=1 mem_max=$((352 * MIB)) "*" cpu_max=max/100000 "*) true ;; *) false ;; esac; then
  record default_machine_unchanged true "guest: ${g}; System VM: ${m}"
else
  record default_machine_unchanged false "guest: ${g}; System VM: ${m}"
fi

# --- 2. A memory limit: the workload has that much memory, and the cgroup enforces it -----------------------------
# 512 MiB asks for a 680 MiB guest (the limit, 152 MiB for the guest itself, 1 MiB in 32 for what the kernel keeps)
# under a ceiling of 680 + 96 + 3 MiB. dd's block is anonymous memory that it fills: 496 MiB fits. 560 MiB is under
# the guest's memory, so the kernel grants the block, and over the limit: the cgroup kills dd when it has filled
# 512 MiB. The guest itself is not short then, because the step also asserts that more than 512 MiB was available.
L=$((512 * MIB))
g="$(runb --memory="${L}" size-c "${VIEW}; dd if=/dev/zero of=/dev/null bs=496M count=1 2>/dev/null && echo under-ok; dd if=/dev/zero of=/dev/null bs=560M count=1 2>/dev/null; echo over-rc=\$?")"
m="$(machine size-d --memory="${L}")"
if between "$(field "${g}" mem)" 600 680 && between "$(field "${g}" avail)" 512 680 && [ "$(field "${g}" max)" = "${L}" ] \
  && case "${g}" in *"OUT under-ok"*"OUT over-rc=137"*) true ;; *) false ;; esac \
  && case "${m}" in *NOT-REMOVED*) false ;; *"MACHINE size-d vcpus=1 mem_max=$((779 * MIB)) "*) true ;; *) false ;; esac; then
  record memory_limit_sizes_the_guest_and_is_enforced true "guest: ${g}; System VM: ${m}"
else
  record memory_limit_sizes_the_guest_and_is_enforced false "guest: ${g}; System VM: ${m}"
fi

# --- 2b. A limit smaller than the default guest's room keeps the default machine and is still enforced ----------
S=$((64 * MIB))
g="$(runb --memory="${S}" size-e "${VIEW}; dd if=/dev/zero of=/dev/null bs=40M count=1 2>/dev/null && echo under-ok; dd if=/dev/zero of=/dev/null bs=100M count=1 2>/dev/null; echo over-rc=\$?")"
if between "$(field "${g}" mem)" 200 256 && between "$(field "${g}" avail)" 64 256 && [ "$(field "${g}" max)" = "${S}" ] && case "${g}" in *"OUT under-ok"*"OUT over-rc=137"*) true ;; *) false ;; esac; then
  record small_limit_enforced_in_the_default_guest true "${g}"
else
  record small_limit_enforced_in_the_default_guest false "${g}"
fi

# --- 2c. Memory and CPUs together: each vCPU costs the guest memory, and the workload still has its limit --------
# 1 GiB on 2 vCPUs asks for a 1216 MiB guest (1024 + 152 + 32 + 8) under a ceiling of 1216 + 96 + 7 MiB.
G=$((1024 * MIB))
g="$(runb --memory="${G}" --cpus=2 size-m "${VIEW}")"; m="$(machine size-n --memory="${G}" --cpus=2)"
if [ "$(field "${g}" cpus)" = 2 ] && between "$(field "${g}" avail)" 1024 1216 && [ "$(field "${g}" max)" = "${G}" ] \
  && case "${m}" in *NOT-REMOVED*) false ;; *"MACHINE size-n vcpus=2 mem_max=$((1319 * MIB)) "*" cpu_max=225000/100000 "*) true ;; *) false ;; esac; then
  record memory_and_cpus_together true "guest: ${g}; System VM: ${m}"
else
  record memory_and_cpus_together false "guest: ${g}; System VM: ${m}"
fi

# --- 3. A CPU quota gives vCPUs and the quota; a CPU set gives its count -------------------------------------------
# Two vCPUs make the guest 264 MiB, under a 360 MiB ceiling. The microVM's own cgroup gets the quota plus a quarter
# of a CPU; a CPU set alone sets no quota anywhere.
g="$(runb --cpus=1.5 size-f "${VIEW}")"; m="$(machine size-g --cpus=1.5)"
gs="$(runb --cpuset=0-1 size-h "${VIEW}")"
if [ "$(field "${g}" cpus)" = 2 ] && [ "$(field "${g}" cpu)" = 150000/100000 ] && [ "$(field "${gs}" cpus)" = 2 ] && [ "$(field "${gs}" cpu)" = max/100000 ] \
  && case "${m}" in *NOT-REMOVED*) false ;; *"MACHINE size-g vcpus=2 mem_max=$((360 * MIB)) "*" cpu_max=175000/100000 "*) true ;; *) false ;; esac; then
  record cpu_limits_give_vcpus_and_a_quota true "--cpus=1.5: ${g}; System VM: ${m}; --cpuset=0-1: ${gs}"
else
  record cpu_limits_give_vcpus_and_a_quota false "--cpus=1.5: ${g}; System VM: ${m}; --cpuset=0-1: ${gs}"
fi

# --- 3b. A workload that lifts its own quota is held by the microVM's cgroup (council SEC-1) ---------------------
# The task has 1.5 CPUs on 2 vCPUs. It writes max to its cgroup's cpu.max and spins on both vCPUs. The microVM's
# cgroup in the System VM allows the quota plus a quarter of a CPU, so its CPU time over 8 s stays under 1.9 CPUs;
# it is over 1.55 CPUs, which shows the quota inside the guest was lifted.
st="$(runb --cpus=1.5 --detach size-l 'echo max > /sys/fs/cgroup/workload/cpu.max; while :; do :; done & while :; do :; done')"
sleep 4; m1="$(ctl microvm machine size-l | head -1)"; sleep 8; m2="$(ctl microvm machine size-l | head -1)"
ctl ctr -- kill size-l 20 >/dev/null; ctl ctr -- delete size-l >/dev/null
u1="$(field "${m1}" cpu_usec)"; u2="$(field "${m2}" cpu_usec)"; t1="$(field "${m1}" at_ms)"; t2="$(field "${m2}" at_ms)"
centi=0
if between "${u1}" 0 "${u2:-0}" && between "${t1}" 0 "$(( ${t2:-0} - 1 ))"; then centi=$(( (u2 - u1) / (t2 - t1) / 10 )); fi   # hundredths of a CPU
if between "${centi}" 155 190 && [ "$(field "${m2}" cpu_max)" = 175000/100000 ]; then
  record lifted_quota_held_by_the_microvm_cgroup true "${st}; ${centi}/100 CPUs over $((t2 - t1)) ms with the guest's quota lifted; ${m2}"
else
  record lifted_quota_held_by_the_microvm_cgroup false "${st}; ${centi}/100 CPUs; first: ${m1}; then: ${m2}"
fi

# --- 4. A quota of more CPUs than there are is clamped; a CPU set of more is refused -----------------------------
# A quota is an upper bound, so the task runs on every CPU the System VM has (council COR-1). A set names CPUs.
over=$((CPUS + 1))
q="$(runb --cpus="${over}" size-o "${VIEW}")"
r1="$(runb --cpuset="0-${CPUS}" size-i 'echo RAN')"; ctl ctr -- delete size-i >/dev/null
r2="$(runb --cpuset=0-32 size-j 'echo RAN')"; ctl ctr -- delete size-j >/dev/null
if [ "$(field "${q}" cpus)" = "${CPUS}" ] && [ "$(field "${q}" cpu)" = "${over}00000/100000" ] && case "${r1}|${r2}" in
    *RAN*) false ;;
    *"ERR cpus: the task asks for ${over} CPUs and the System VM has ${CPUS}"*"|"*"names more than 32 CPUs"*) true ;;
    *) false ;; esac; then
  record too_many_cpus_clamped_or_refused true "a quota of ${over} CPUs on ${CPUS}: ${q}; a set of ${over}: ${r1}; a set of 33: ${r2}"
else
  record too_many_cpus_clamped_or_refused false "a quota of ${over} CPUs on ${CPUS}: ${q}; a set of ${over}: ${r1}; a set of 33: ${r2}"
fi

# --- 5. A limit the memory budget cannot hold is refused by name, and takes nothing -------------------------------
# 1 TiB is the largest limit the shim takes; its guest would be 1,081,608 MiB.
r="$(runb --memory=$((1048576 * MIB)) size-k 'echo RAN')"; ctl ctr -- delete size-k >/dev/null
after="$(budget committed)"
case "${r}" in
  *RAN*) record memory_over_the_budget_refused false "${r}" ;;
  *"ERR memory: a 1081608 MiB microVM"*"does not fit"*"start the System VM with more memory"*)
    [ "${after}" = "${base}" ] && record memory_over_the_budget_refused true "${r}; committed ${base} before and ${after} after" \
      || record memory_over_the_budget_refused false "committed ${base} before and ${after} after; ${r}" ;;
  *) record memory_over_the_budget_refused false "${r}" ;;
esac

# --- 5b. --snapshot-room: the ceiling and the booking hold one more guest-size; never with a rootfs (TD-085) ------
# The default guest's ceiling is 352 MiB; with the room, 352 + 256. The booking is that ceiling and the jail's copies
# (the kernel and initramfs, about 44 MB), so it is at least 608 MiB and less than 672.
b0="$(budget committed)"
l="$(ctl microvm -- launch size-s --snapshot-room | head -1)"
b1="$(budget committed)"; m="$(ctl microvm machine size-s | head -1)"; c="$(ctl microvm cleanup size-s | head -1)"
r="$(ctl microvm -- launch size-s --snapshot-room --rootfs /nonexistent | head -1)"
booked=$(( (${b1:-0} - ${b0:-0}) / MIB ))
if case "${l}" in "LAUNCHED size-s "*) true ;; *) false ;; esac && case "${m}" in *"MACHINE size-s vcpus=1 mem_max=$((608 * MIB)) "*) true ;; *) false ;; esac \
  && between "${booked}" 608 671 && [ "${c}" = "CLEAN size-s" ] \
  && [ "${r}" = "ERR --snapshot-room is for the parity conformance check and excludes --rootfs" ]; then
  record snapshot_room_in_the_ceiling_and_the_booking true "${l}; ${m}; booked ${booked} MiB; ${c}; with a rootfs: ${r}"
else
  record snapshot_room_in_the_ceiling_and_the_booking false "${l}; ${m}; booked ${booked} MiB; ${c}; with a rootfs: ${r}"
fi

# --- 5c. A guest that stops before its marker is reported at once, not after the 30 s poll (TD-060) --------------
t0=$(date +%s)
f="$(ctl microvm -- launch size-x --test-crash | head -1)"
t1=$(date +%s); c="$(ctl microvm -- cleanup size-x --logs | head -1)"
if [ "${f}" = "FAILED size-x" ] && [ $((t1 - t0)) -lt 15 ] && [ "${c}" = "CLEAN size-x" ]; then
  record failed_launch_reported_at_once true "${f} in $((t1 - t0)) s; ${c}"
else
  record failed_launch_reported_at_once false "${f} in $((t1 - t0)) s; ${c}"
fi

# --- 6. Nothing is left -------------------------------------------------------------------------------------------
left="$(ctl ctr -- leftovers)"; after="$(budget committed)"
case "${left}:${after}" in *CLEAN*":${base}") record nothing_left true "${left}; committed ${after}, as before" ;; *) record nothing_left false "${left}; committed ${after}, before ${base}" ;; esac

passed=true; for s in "${steps[@]}"; do case "$s" in *'"ok":false'*) passed=false ;; esac; done
printf '{"steps":[%s],"passed":%s}\n' "$(IFS=,; echo "${steps[*]}")" "${passed}" > "${REPORT}"
echo "microvm-size suite: passed=${passed} (report: ${REPORT})"
[ "${passed}" = true ]
