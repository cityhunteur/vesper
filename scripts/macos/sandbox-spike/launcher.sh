#!/usr/bin/env bash
# ADR 0012 slice 3 evidence: boots the System VM once with the launcher under sandbox-exec and the profile the
# launcher now applies to itself (deny default; its files, console log, bridge socket and shares), then prints the
# launcher's last lines and launchd's denied lookups. DROP=fuse, DROP=files or DROP=fsevents leaves out one of the
# three rules beyond files and sockets, to show the boot or the share watcher fails without it:
#   fuse      (allow generic-issue-extension)            "Failed to issue Fuse sandbox extension."
#   files     (allow file-issue-extension ...)           "Failed to issue sandbox extension for file system device"
#   fsevents  (allow mach-lookup "com.apple.FSEvents")   launchd: denied lookup com.apple.FSEvents
# The System VM must be stopped first (make systemvm-down). Usage: launcher.sh [SECONDS] (default 60).
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "${ROOT_DIR}"
CONFIG="${ROOT_DIR}/build/systemvm/launch-config.json"
LAUNCHER="${ROOT_DIR}/build/vmlauncher/vesper-vmlauncher"
T="$(mktemp -d)"; trap 'rm -rf "${T:?}"' EXIT

python3 -I - "${CONFIG}" "${LAUNCHER}" "${DROP:-}" > "${T}/profile.sb" <<'PY'
import json, os, sys
cfg, launcher, drop = json.load(open(sys.argv[1])), sys.argv[2], sys.argv[3]
def q(p):
    p = os.path.abspath(p)
    return '"' + p.replace("\\", "\\\\").replace('"', '\\"') + '"'
files = [cfg.get(k) for k in ("kernel_path", "initrd_path", "boot_media_path")]
disks = [cfg.get(k) for k in ("rootfs_path", "data_disk_path")]
out = ['(version 1)', '(deny default)', '(import "system.sb")', '(allow signal (target self))',
       f'(allow process-exec file-read* (literal {q(launcher)}))', f'(allow file-read* (literal {q(sys.argv[1])}))']
out += [f'(allow file-read* (literal {q(p)}))' for p in files if p]
out += [f'(allow file-read* file-write* (literal {q(p)}))' for p in disks + [cfg["console_log_path"]] if p]
if cfg.get("vsock_bridge_path"):
    b = q(cfg["vsock_bridge_path"])
    out += [f'(allow file-read* file-write* (literal {b}))',
            f'(allow network-bind network-inbound (local unix-socket (path-literal {b})))']
shares = cfg.get("shared_directories") or []
out += [f'(allow file-read* file-write* (subpath {q(s["host_path"])}))' for s in shares]
if drop != "fuse":
    out.append('(allow generic-issue-extension)')
if drop != "files":
    out += [f'(allow file-issue-extension (literal {q(p)}))' for p in files + disks if p]
    out += [f'(allow file-issue-extension (subpath {q(s["host_path"])}))' for s in shares]
if drop != "fsevents" and any(s.get("watch") for s in shares):
    out.append('(allow mach-lookup (global-name "com.apple.FSEvents"))')
print("\n".join(out))
PY

T0="$(date '+%Y-%m-%d %H:%M:%S')"
set +e
timeout "${1:-60}" sandbox-exec -f "${T}/profile.sb" "${LAUNCHER}" --config "${CONFIG}" --no-sandbox --keep-running \
  --timeout-seconds 90 < /dev/null > "${T}/out.log" 2>&1
echo "rc=$? (124: still running at the time limit, as a VM that booted is)"
set -e
grep -v "running without its sandbox profile" "${T}/out.log" | tail -4 | cut -c1-240
sleep 2
log show --start "${T0}" --style compact --predicate 'process == "launchd" AND eventMessage CONTAINS "Sandbox restriction"' \
  2>/dev/null | grep "vesper-vmlaunch" | sed -E 's/^.*denied lookup: name = ([^,]+),.*$/denied lookup: \1/' | sort | uniq -c
echo "macOS $(sw_vers -productVersion); DROP=${DROP:-none}"
