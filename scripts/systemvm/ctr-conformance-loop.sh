#!/usr/bin/env bash
# W6 (D-029): run ctr-conformance N times (default 10); a run longer than LIMIT seconds (default 1200) is a hang:
# its state (host processes, containerd tasks and containers, leftovers, System VM and containerd logs) is written
# to run/W6/iter-<n>/state.txt before it is killed. Usage: ctr-conformance-loop.sh [N] [LIMIT]; needs `make systemvm-up`.
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"; cd "${ROOT_DIR}"
export DOCKER_HOST=unix://$PWD/run/vesper/docker.sock
C() { cargo run -q --bin systemvmctl -- control "$@" 2>&1; }
N=${1:-10}; LIMIT=${2:-1200}; mkdir -p run/W6; : > run/W6/summary.txt
for i in $(seq 1 "$N"); do
  d=run/W6/iter-$i; mkdir -p "$d"
  t0=$(date +%s)
  bash scripts/systemvm/ctr-conformance.sh > "$d/out.log" 2>&1 &
  pid=$!
  while kill -0 "$pid" 2>/dev/null; do
    if [ $(( $(date +%s) - t0 )) -gt "$LIMIT" ]; then
      echo "HANG iter $i after ${LIMIT}s" | tee "$d/HANG"
      { echo "== host processes"; ps -o pid,ppid,etime,args -ax | grep -E 'systemvmctl|cargo|ctr-conformance' | grep -v grep
        echo "== ctr tasks"; C ctr -- try task ls; echo "== containers"; C ctr -- try containers ls
        echo "== leftovers"; C ctr -- leftovers; echo "== messages"; C logs messages --lines 200
        echo "== containerd"; C logs containerd --lines 200; } > "$d/state.txt" 2>&1
      pkill -P "$pid"; kill "$pid"; exit 3
    fi
    sleep 10
  done
  wait "$pid"; rc=$?
  echo "iter $i rc=$rc secs=$(( $(date +%s) - t0 ))" | tee -a run/W6/summary.txt
  cp run/systemvm/ctr-conformance-report.json "$d/" 2>/dev/null
  [ "$rc" = 0 ] || { echo "FAIL iter $i" >> run/W6/summary.txt; exit 4; }
done
echo "W6 done: $N runs, no hang" | tee -a run/W6/summary.txt
