#!/usr/bin/env bash
# Self-test for the launcher's share watcher (GH #150) without a VM: `vesper-vmlauncher --watch-dry-run DIR`
# prints the paths it would ask the guest to replay; this edits files in a temp share and checks the lines.
# The second half points the watcher at a Unix socket (`--watch-sink-socket`) to check delivery, reconnect
# and that a sink which stops reading cannot hold up SIGTERM. Needs python3 for the stand-in sink.
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
kill "$PID" 2>/dev/null; wait "$PID" 2>/dev/null

# --- The sink connection (no VM): --watch-sink-socket sends the lines to a Unix socket exactly as the
# launcher sends them to the guest. A stand-in sink either reads every line into a file or accepts the
# connection and never reads. /tmp keeps the socket path inside sun_path's 104 bytes.
S="$(mktemp -d /tmp/vsw.XXXXXX)"; SOCK="$S/sink.sock"; GOT="$S/got"; LOG="$S/log"; SINK_PID=""
trap 'kill "$PID" "$SINK_PID" 2>/dev/null; /bin/rm -rf "${T:?}" "${OTHER:?}" "${OUT:?}" "${S:?}"' EXIT
sink() { # sink drain|stall
  /bin/rm -f "$SOCK"
  python3 -c '
import socket, sys, time
mode, path, out = sys.argv[1:4]
s = socket.socket(socket.AF_UNIX); s.bind(path); s.listen(4)
while True:
    c, _ = s.accept()
    if mode == "stall":
        time.sleep(3600)
    with open(out, "ab", buffering=0) as f:
        while True:
            d = c.recv(65536)
            if not d: break
            f.write(d)
' "$1" "$SOCK" "$GOT" & SINK_PID=$!
  for _ in $(seq 1 50); do [ -S "$SOCK" ] && break; sleep 0.1; done
}
watch_to_sink() {
  : > "$LOG"
  "${LAUNCHER}" --watch-dry-run "$T" --watch-sink-socket "$SOCK" > "$LOG" 2>&1 & PID=$!
  for _ in $(seq 1 50); do grep -q 'connected to socket' "$LOG" && break; sleep 0.1; done
  sleep 1
}
# Waits up to $1 seconds for the launcher to exit after SIGTERM; prints the seconds taken or "hung".
term() {
  kill -TERM "$PID"; i=0
  while kill -0 "$PID" 2>/dev/null; do i=$((i+1)); [ "$i" -gt $(( $1 * 10 )) ] && { echo hung; kill -9 "$PID"; return; }; sleep 0.1; done
  echo "under-$1s"
}
field() { grep '^SUMMARY' "$LOG" | tr ' ' '\n' | sed -n "s/^$1=//p"; }
got() { sed "s#^$T#T#" "$GOT" 2>/dev/null | sort -u | wc -l | tr -d ' '; }

# A sink that reads: every path of a burst arrives once, in whole lines, and the counters agree.
sink drain; watch_to_sink
mkdir "$T/burst"; for i in $(seq 1 3000); do echo "$i" > "$T/burst/f$i"; done
for _ in $(seq 1 100); do [ "$(grep -c "^$T/burst/f" "$GOT" 2>/dev/null)" -ge 3000 ] && break; sleep 0.1; done
check sink_burst_delivered "$(grep "^$T/burst/f" "$GOT" | sort -u | wc -l | tr -d ' ')" "3000"
check sink_lines_whole "$(grep -cv "^$T/" "$GOT")" "0"
# The sink dies and comes back: the launcher reconnects and later edits arrive.
kill "$SINK_PID"; wait "$SINK_PID" 2>/dev/null; : > "$GOT"
echo x > "$T/burst/while-down-1"; sleep 0.3; echo x > "$T/burst/while-down-2"
sink drain
for _ in $(seq 1 60); do grep -q "connection 2" "$LOG" && break; sleep 0.1; done
echo x > "$T/burst/after-reconnect"
for _ in $(seq 1 50); do grep -q "after-reconnect" "$GOT" 2>/dev/null && break; sleep 0.1; done
check sink_reconnects "$(grep -c 'connection 2' "$LOG")" "1"
check sink_delivers_after_reconnect "$(grep -c 'burst/after-reconnect$' "$GOT")" "1"
check sink_resends_unsent_edit "$(grep -c 'burst/while-down-2$' "$GOT")" "1"
check sigterm_exits_with_summary "$(term 3) $(grep -c '^SUMMARY forwarded=' "$LOG")" "under-3s 1"
kill "$SINK_PID" 2>/dev/null; wait "$SINK_PID" 2>/dev/null

# A sink that never reads (hung guest): the watcher keeps the paths queued and SIGTERM still exits at once.
# The queue limit is lowered to 2,000 so the burst passes it: the oldest paths are dropped and counted.
sink stall; export VESPER_SHARE_WATCH_PENDING_LIMIT=2000; watch_to_sink; unset VESPER_SHARE_WATCH_PENDING_LIMIT
mkdir "$T/stall"; for i in $(seq 1 4000); do echo "$i" > "$T/stall/f$i"; done
sleep 2
check stalled_sink_sigterm_exits "$(term 3)" "under-3s"
stalls="$(field stalls)"; queued="$(field queued)"; dropped="$(field dropped)"; forwarded="$(field forwarded)"; sent="$(field sent)"
check stalled_sink_reported "$([ "${stalls:-0}" -gt 0 ] && [ "${queued:-0}" -gt 0 ] && echo yes || echo "stalls=$stalls queued=$queued")" "yes"
check queue_limit_drops_oldest "$([ "${queued:-0}" -le 2000 ] && [ "${dropped:-0}" -gt 0 ] && [ "$(( ${sent:-0} + ${queued:-0} + ${dropped:-0} ))" = "${forwarded:-x}" ] && echo yes || echo "forwarded=$forwarded sent=$sent queued=$queued dropped=$dropped")" "yes"
kill "$SINK_PID" 2>/dev/null
[ "$fails" = 0 ] && echo "share-watch-selftest: passed" || { echo "share-watch-selftest: $fails failed"; sed "s#^$T#T#" "$OUT"; tail -5 "$LOG"; exit 1; }
