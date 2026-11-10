#!/usr/bin/env bash
# Self-test for the launcher's share watcher (GH #150) without a VM: `vesper-vmlauncher --watch-dry-run DIR`
# prints the paths it would ask the guest to replay; this edits files in a temp share and checks the lines.
# Needs macOS and `make vmlauncher-build`.
set -u
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LAUNCHER="${VMLAUNCHER:-${ROOT_DIR}/build/vmlauncher/vesper-vmlauncher}"
[ -x "${LAUNCHER}" ] || { echo "share-watch-selftest: ${LAUNCHER} missing; run 'make vmlauncher-build'" >&2; exit 2; }
# mktemp gives a path under /var, a symlink to /private/var: FSEvents reports the resolved path and the
# watcher must answer with the configured one.
T="$(mktemp -d)"; OUT="$(mktemp)"; OTHER="$(mktemp -d)"
mkdir -p "$T/sub"; echo a > "$T/sub/a.txt"
"${LAUNCHER}" --watch-dry-run "$T" > "$OUT" 2>&1 & PID=$!
trap 'kill "$PID" 2>/dev/null; /bin/rm -rf "${T:?}" "${OTHER:?}" "${OUT:?}"' EXIT
for _ in $(seq 1 50); do grep -q '^WATCHING' "$OUT" && break; sleep 0.1; done
sleep 1
fails=0
check() { if [ "$2" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1: got '$2' want '$3'"; fails=$((fails+1)); fi; }
lines() { tail -n +2 "$OUT" | sed "s#^$T#T#" | tr '\n' ' ' | sed 's/ $//'; }
step() { : > "$OUT.mark"; BEFORE="$(wc -l < "$OUT")"; }
new() { sleep 1; tail -n +"$((BEFORE+1))" "$OUT" | sed "s#^$T#T#" | sort -u | tr '\n' ' ' | sed 's/ $//'; }

check greeting "$(head -1 "$OUT")" "WATCHING 1"
step; echo more >> "$T/sub/a.txt"; check host_append_sent "$(new)" "T/sub/a.txt"
step; touch -m -r "$T/sub/a.txt" "$T/sub/a.txt"; check replay_echo_dropped "$(new)" ""
step; echo b > "$T/sub/b.txt"; check host_create_sent "$(new)" "T/sub/b.txt"
step; echo v2 > "$T/sub/.a.tmp"; mv "$T/sub/.a.tmp" "$T/sub/a.txt"; case " $(new) " in *" T/sub/a.txt "*) r=yes ;; *) r=no ;; esac; check atomic_save_sent "$r" "yes"
step; /bin/rm "$T/sub/b.txt"; check host_delete_sends_parent "$(new)" "T/sub"
step; echo r > "$T/root.txt"; check create_in_share_root_sent "$(new)" "T/root.txt"
step; /bin/rm "$T/root.txt"; check delete_in_share_root_not_sent "$(new)" ""
step; echo o > "$OTHER/o.txt"; check outside_share_not_sent "$(new)" ""
step; mkdir "$T/newdir"; echo n > "$T/newdir/n.txt"; check new_directory_and_file_sent "$(new)" "T/newdir T/newdir/n.txt"
check no_resolved_prefix_leaks "$(grep -c '^/private' "$OUT")" "0"
/bin/rm -f "$OUT.mark"
[ "$fails" = 0 ] && echo "share-watch-selftest: passed" || { echo "share-watch-selftest: $fails failed"; sed "s#^$T#T#" "$OUT"; exit 1; }
