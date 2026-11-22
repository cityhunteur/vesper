#!/usr/bin/env bash
# ADR 0012 evidence: what macOS does with each way of sandboxing an unbundled command-line binary. No arguments;
# macOS only; writes nothing outside a temporary directory, which it removes. Prints one line for each result.
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
T="$(mktemp -d)"; trap 'rm -rf "${T:?}"' EXIT
T="$(cd "$T" && pwd -P)"
mkdir -p "$T/ok"; echo hi > "$T/ok/f"; echo secret > "$T/denied.txt"
# 1. App Sandbox entitlement, no bundle identifier: the process is stopped before main.
printf 'int main(void){return 0;}\n' > "$T/empty.c"; cc -o "$T/empty" "$T/empty.c"
codesign --force --sign - --entitlements "${ROOT_DIR}/packaging/macos/vesper-proxy.entitlements" "$T/empty" 2>/dev/null
set +e; "$T/empty"; rc=$?; set -e
echo "app-sandbox without a bundle id: exit status ${rc} (133 is SIGTRAP: stopped at start)"
# 2. sandbox_init with a profile string, in the process.
cc -Wno-deprecated-declarations -o "$T/si" "${ROOT_DIR}/scripts/macos/sandbox-spike/sandbox_init_probe.c"
P="(version 1)(deny default)(import \"system.sb\")(allow file-read* (subpath \"$T/ok\"))"
echo "sandbox_init in the process:"; "$T/si" "$P" "$T/ok/f" "$T/denied.txt" "$T/new.txt" 2>/dev/null | sed 's/^/  /'
# 3. The same under a parent that is already sandboxed.
echo "sandbox_init under a sandboxed parent:"
sandbox-exec -p '(version 1)(allow default)' "$T/si" "$P" "$T/ok/f" "$T/denied.txt" "$T/new.txt" 2>/dev/null | head -3 | sed 's/^/  /'
echo "macOS $(sw_vers -productVersion)"
