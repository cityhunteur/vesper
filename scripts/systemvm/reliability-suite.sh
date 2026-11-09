#!/usr/bin/env bash
# System VM bring-up reliability suite (GH #16): N boot cycles under the
# boot-time threshold, then on a kept-up VM the vsock control suite, the
# runtime services suite and the Docker socket smoke, with host resource
# leak checks before and after, merged into run/systemvm/reliability-report.json.
# Usage: reliability-suite.sh [CYCLES]   (default 50; CYCLES env also honoured)
set -uo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CYCLES="${1:-${CYCLES:-50}}"
RUN_DIR="${ROOT_DIR}/run/systemvm"
LOG="${RUN_DIR}/reliability.log"
REPORT="${RUN_DIR}/reliability-report.json"
cd "${ROOT_DIR}"
mkdir -p "${RUN_DIR}"
: > "${LOG}"
FAILURES=()
say() { echo "reliability: $*" | tee -a "${LOG}"; }
fail() { FAILURES+=("$1"); say "FAIL $1"; }

for need in build/vmlauncher/vesper-vmlauncher build/systemvm/launch-config.json build/systemvm/trivial-rootfs.tar; do
  [ -e "${need}" ] || { say "missing ${need}; run 'make systemvm-prepare systemvm-trivial-image'"; exit 2; }
done

snapshot() { # path
  python3 - "$1" "${ROOT_DIR}" <<'PY'
import json, os, subprocess, sys
out, root = sys.argv[1], sys.argv[2]
def count(pattern):
    r = subprocess.run(["pgrep", "-f", pattern], capture_output=True, text=True)
    pids = [p for p in r.stdout.split() if p and int(p) != os.getpid()]
    return len(pids), " ".join(pids)
socks = subprocess.run(["find", os.path.join(root, "run"), "-type", "s"], capture_output=True, text=True).stdout.split()
pids = subprocess.run(["find", os.path.join(root, "run"), "-name", "*.pid"], capture_output=True, text=True).stdout.split()
lp, lpd = count("vesper-vmlauncher --config")
pp, ppd = count("target/debug/vesper-proxy")
snap = {"launcher_processes": lp, "launcher_processes_detail": lpd, "proxy_processes": pp, "proxy_processes_detail": ppd,
        "sockets": len(socks), "sockets_detail": " ".join(os.path.relpath(s, root) for s in socks),
        "pid_files": len(pids), "pid_files_detail": " ".join(os.path.relpath(p, root) for p in pids)}
json.dump(snap, open(out, "w"), indent=1)
print(snap)
PY
}

say "cycles=${CYCLES} host=$(sw_vers -productVersion 2>/dev/null || uname -r) $(sysctl -n hw.model 2>/dev/null || true) commit=$(git rev-parse --short HEAD 2>/dev/null || echo unknown)"
make -s docker-down >/dev/null 2>&1 || true
make -s systemvm-down >/dev/null 2>&1 || true
say "leak snapshot before: $(snapshot "${RUN_DIR}/leak-before.json")"

say "boot stress: ${CYCLES} cycles"
if bash scripts/systemvm/boot-smoke.sh "${CYCLES}" 60 >> "${LOG}" 2>&1; then
  say "boot stress passed: $(python3 -c 'import json;d=json.load(open("run/systemvm/boot-report.json"));print("{}/{} max_boot_ms={} threshold={}".format(d["successful"], d["iterations"], d["max_boot_ms"], d.get("max_boot_allowed_ms")))')"
else
  fail "boot stress (see run/systemvm/boot.log and run/systemvm/console.log)"
fi
cp "${RUN_DIR}/boot-report.json" "${RUN_DIR}/reliability-boot-report.json" 2>/dev/null

rm -f "${RUN_DIR}/control-report.json" "${RUN_DIR}/services-report.json" "${RUN_DIR}/docker-smoke-report.json"
say "socket smoke: systemvm-up, control suite, services suite, docker-up, docker smoke"
if make -s systemvm-up >> "${LOG}" 2>&1; then
  cargo run -q --bin systemvmctl -- control suite --keepalive-seconds 10 >> "${LOG}" 2>&1 || fail "control suite (run/systemvm/control-report.json)"
  cargo run -q --bin systemvmctl -- control services-suite >> "${LOG}" 2>&1 || fail "services suite (run/systemvm/services-report.json)"
  if make -s docker-up >> "${LOG}" 2>&1; then
    bash scripts/systemvm/docker-smoke.sh >> "${LOG}" 2>&1 || fail "docker smoke (run/systemvm/docker-smoke-report.json)"
    make -s docker-down >> "${LOG}" 2>&1 || fail "docker-down"
  else
    fail "docker-up (run/vesper/vesper-proxy.log)"
  fi
  make -s systemvm-down >> "${LOG}" 2>&1 || fail "systemvm-down (run/systemvm/vmlauncher.log)"
else
  fail "systemvm-up (run/systemvm/vmlauncher.log)"
fi

say "leak snapshot after: $(snapshot "${RUN_DIR}/leak-after.json")"
ARGS=(--out "${REPORT}" --cycles "${CYCLES}" --boot-report "${RUN_DIR}/reliability-boot-report.json"
      --leak-before "${RUN_DIR}/leak-before.json" --leak-after "${RUN_DIR}/leak-after.json"
      --host "$(sw_vers -productVersion 2>/dev/null || uname -r) $(sysctl -n hw.model 2>/dev/null || true)"
      --commit "$(git rev-parse --short HEAD 2>/dev/null || echo unknown)")
[ -f "${RUN_DIR}/control-report.json" ] && ARGS+=(--control-report "${RUN_DIR}/control-report.json")
[ -f "${RUN_DIR}/services-report.json" ] && ARGS+=(--services-report "${RUN_DIR}/services-report.json")
[ -f "${RUN_DIR}/docker-smoke-report.json" ] && ARGS+=(--docker-report "${RUN_DIR}/docker-smoke-report.json")
for f in "${FAILURES[@]:-}"; do [ -n "${f}" ] && ARGS+=(--failure "${f}"); done
python3 -I scripts/systemvm/reliability_report.py "${ARGS[@]}" | tee -a "${LOG}"
