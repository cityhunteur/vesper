#!/usr/bin/env bash
# Self-test for `vesper-share touch|notify` (GH #150) without a System VM: runs the overlay script against a
# fake mounts table (VESPER_SHARE_MOUNTS) in a temp directory and checks the path policy and that a replay
# changes neither contents nor mtime. Needs sh, awk, realpath and touch -r (macOS 13+ or Linux).
set -u
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TOOL="${ROOT_DIR}/systemvm/overlay/usr/local/bin/vesper-share"
T="$(realpath "$(mktemp -d)")"; trap '/bin/rm -rf "${T:?}"' EXIT
mkdir -p "$T/rw/sub" "$T/ro" "$T/with space" "$T/outside"
echo a > "$T/rw/sub/a.txt"; echo r > "$T/ro/r.txt"; echo s > "$T/with space/s.txt"; echo o > "$T/outside/o.txt"
ln -s "$T/outside/o.txt" "$T/rw/escape"; ln -s "$T/rw/sub/a.txt" "$T/rw/inside-link"
{ echo "share0 $T/rw virtiofs rw,relatime 0 0"; echo "share1 $T/ro virtiofs ro,relatime 0 0"
  echo "share2 $T/with\\040space virtiofs rw,relatime 0 0"; echo "/dev/vda / ext4 rw 0 0"; } > "$T/mounts"
export VESPER_SHARE_MOUNTS="$T/mounts"
fails=0
check() { if [ "$2" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1: got '$2' want '$3'"; fails=$((fails+1)); fi; }
mt() { stat -c '%y' "$1" 2>/dev/null || stat -f '%Fm' "$1"; }
one() { sh "$TOOL" touch "$1" | sed "s#$T#T#g"; }

before="$(mt "$T/rw/sub/a.txt")"
check file_in_rw_share_touched "$(one "$T/rw/sub/a.txt")" "TOUCHED T/rw/sub/a.txt"
check mtime_unchanged "$(mt "$T/rw/sub/a.txt")" "$before"
check contents_unchanged "$(cat "$T/rw/sub/a.txt")" "a"
check directory_touched "$(one "$T/rw/sub")" "TOUCHED T/rw/sub"
check path_with_space_touched "$(one "$T/with space/s.txt")" "TOUCHED T/with space/s.txt"
check symlink_inside_share_touched "$(one "$T/rw/inside-link")" "TOUCHED T/rw/inside-link"
check share_root_skipped "$(one "$T/rw")" "SKIPPED T/rw share-root"
check read_only_share_skipped "$(one "$T/ro/r.txt")" "SKIPPED T/ro/r.txt read-only-share"
check missing_path_reported "$(one "$T/rw/gone.txt")" "MISSING T/rw/gone.txt"
check missing_not_created "$([ -e "$T/rw/gone.txt" ] && echo created || echo absent)" "absent"
check outside_shares_rejected "$(one "$T/outside/o.txt")" "REJECTED T/outside/o.txt outside-shares"
check symlink_leaving_share_rejected "$(one "$T/rw/escape")" "REJECTED T/rw/escape outside-shares"
check dotdot_rejected "$(one "$T/rw/../outside/o.txt")" "REJECTED T/rw/../outside/o.txt dotdot"
check relative_rejected "$(one "rw/sub/a.txt")" "REJECTED rw/sub/a.txt not-absolute"
check prefix_sibling_rejected "$(mkdir -p "$T/rw2" && echo x > "$T/rw2/x" && one "$T/rw2/x")" "REJECTED T/rw2/x outside-shares"
sh "$TOOL" touch "$T/rw/sub/a.txt" >/dev/null; check touch_rc_ok "$?" "0"
sh "$TOOL" touch "$T/outside/o.txt" >/dev/null; check touch_rc_rejected "$?" "1"
out="$(printf '%s\n' "$T/rw/sub/a.txt" "$T/rw" "$T/ro/r.txt" "$T/rw/gone.txt" "$T/outside/o.txt" "" "$T/rw/escape" "$T/with space/s.txt" | sh "$TOOL" notify | tr '\n' '|')"
check notify_stream_counts "$out" "VESPER-SHARE-NOTIFY 1|NOTIFY touched=2 skipped=2 missing=1 rejected=2 errors=0|"
check outside_file_untouched "$(cat "$T/outside/o.txt")" "o"
[ "$fails" = 0 ] && echo "share-notify-selftest: passed" || { echo "share-notify-selftest: $fails failed"; exit 1; }
