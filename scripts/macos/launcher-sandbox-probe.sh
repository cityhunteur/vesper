#!/usr/bin/env bash
# ADR 0012 slice 3: the launcher's seatbelt profile refuses what the launcher never needs. Runs the signed launcher's
# --sandbox-probe hook (no VM: it exits after the probes) with a temporary launch config, under the profile and with
# --no-sandbox, and checks each attempt is denied under it and works without it, so the probe can see a difference.
# Also: a share path with SBPL metacharacters cannot widen the profile, a share of the home directory moves the
# self-check to the temporary directory, and a share of / is refused. Usage: launcher-sandbox-probe.sh [LAUNCHER].
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LAUNCHER="${1:-${ROOT_DIR}/build/vmlauncher/vesper-vmlauncher}"
[ -x "${LAUNCHER}" ] || { echo "launcher-sandbox-probe: ${LAUNCHER} missing; run make vmlauncher-build" >&2; exit 2; }

WORK="$(mktemp -d /tmp/vlp.XXXXXX)"
LISTENER_PID=""
cleanup() {
  [ -n "${LISTENER_PID}" ] && kill "${LISTENER_PID}" 2>/dev/null || true
  rm -rf "${WORK:?}"
}
trap cleanup EXIT

# A loopback TCP listener and a Unix socket listener the profile does not name; the port goes to a file.
python3 -I - "${WORK}" <<'PY' &
import os, socket, sys, time
work = sys.argv[1]
t = socket.socket(); t.bind(("127.0.0.1", 0)); t.listen(16)
u = socket.socket(socket.AF_UNIX); u.bind(os.path.join(work, "o.sock")); u.listen(16)
with open(os.path.join(work, "port.tmp"), "w") as f: f.write(str(t.getsockname()[1]))
os.rename(os.path.join(work, "port.tmp"), os.path.join(work, "port"))
time.sleep(120)
PY
LISTENER_PID=$!
for _ in $(seq 1 50); do [ -f "${WORK}/port" ] && break; sleep 0.1; done
PORT="$(cat "${WORK}/port")"

mkdir -p "${WORK}/vm" "${WORK}/share" "${WORK}/run" "${WORK}/other"
for f in kernel initrd data.img; do : > "${WORK}/vm/${f}"; done
echo secret > "${WORK}/other/outside.txt"

# config SHARE_PATH -> writes ${WORK}/config.json
config() {
  python3 -I - "${WORK}" "$1" <<'PY'
import json, sys
w, share = sys.argv[1], sys.argv[2]
json.dump({
  "kernel_path": f"{w}/vm/kernel", "initrd_path": f"{w}/vm/initrd", "kernel_cmdline": "console=hvc0",
  "network_mode": "none", "cpu_count": 1, "memory_mib": 512, "console_log_path": f"{w}/run/console.log",
  "data_disk_path": f"{w}/vm/data.img", "vsock_bridge_path": f"{w}/run/b.sock", "bridge_allowed_ports": [1024],
  "shared_directories": [{"tag": "s0", "host_path": share, "read_only": False, "watch": True}],
}, open(f"{w}/config.json", "w"))
PY
}

probe() {
  "${LAUNCHER}" --config "${WORK}/config.json" --sandbox-probe --probe-tcp "${PORT}" \
    --probe-unix "${WORK}/o.sock" --probe-file "${WORK}/other/outside.txt" "$@" 2>"${WORK}/stderr"
}

fail() { echo "launcher-sandbox-probe: FAIL $*" >&2; cat "${WORK}/stderr" >&2 2>/dev/null || true; exit 1; }

config "${WORK}/share"
confined="$(probe)" || fail "the launcher did not run under its profile"
open="$(probe --no-sandbox)" || fail "the launcher did not run with --no-sandbox"
[ "$(grep -c '^PROBE .* denied$' <<<"${confined}")" = 7 ] || fail "under the profile, not every probe was denied: ${confined}"
[ "$(grep -c '^PROBE .* ALLOWED$' <<<"${open}")" = 7 ] || fail "without the profile, not every probe worked: ${open}"
grep -q "running without its sandbox profile" "${WORK}/stderr" || fail "--no-sandbox printed no warning"
echo "launcher-sandbox-probe: 7 probes denied under the profile, 7 allowed without it"

# A share path that tries to end its SBPL string and allow everything: the profile must still deny.
HOSTILE="${WORK}/sh\")(allow default)(\""
mkdir -p "${HOSTILE}"
config "${HOSTILE}"
hostile="$(probe)" || fail "the launcher did not run with a hostile share path"
[ "$(grep -c '^PROBE .* denied$' <<<"${hostile}")" = 7 ] || fail "a hostile share path widened the profile: ${hostile}"
echo "launcher-sandbox-probe: a share path with SBPL metacharacters does not widen the profile"

# A share of the home directory is allowed; the self-check then uses the temporary directory.
config "$(python3 -I -c 'import pwd,os;print(pwd.getpwuid(os.getuid()).pw_dir)')"
probe >/dev/null || fail "a share of the home directory was refused"
echo "launcher-sandbox-probe: a share of the home directory starts, with the self-check on the temporary directory"

# A share of / covers every canary: refused, with the reason.
config "/"
if probe >/dev/null; then fail "a share of / was accepted"; fi
grep -q "the self-check has nothing to test" "${WORK}/stderr" || fail "a share of / was refused for another reason"
echo "launcher-sandbox-probe: a share of / is refused"
echo "launcher-sandbox-probe: passed"
