#!/usr/bin/env bash
# Run the upstream Firecracker integration suite on Backend A and explain every result (GH #172, D-023).
#
# Usage: scripts/systemvm/upstream-suite/run.sh [--clean] [pytest selection ...]
#   With no selection it runs the paths pinned in the baseline lock (baselines.firecracker.upstream_suite). A
#   selection such as `integration_tests/functional/test_api.py` makes a quick partial run; the comparator then judges
#   only the selected tests. --clean also drops the cached source volume and cargo cache.
#
# Steps, each logged under run/GH-172/<stamp>/: fetch the pinned tree, CI artefacts and release (host, cached in
# build/upstream-suite/); restart the System VM with 4 vCPUs, 8 GiB and build/upstream-suite shared; load `tun`; stage
# the tree into a data-disk volume (unsquashfs cannot write the virtio-fs share) and run upstream's
# setup-ci-artifacts.sh there; build the examples and tools some tests expect under build/cargo_target; run
# tools/test.sh in the pinned container with a per-test timeout that kills a hung worker (--timeout-method=thread)
# and a wall-clock watchdog; collect {nodeid: outcome}; wipe /srv; restore the previous launch config and restart;
# compare (baselinectl --mode upstream-compare, which also fails on pytest's own errors). Exit status is the
# comparator's. One run at a time: build/upstream-suite/runner.lock is a symlink to the runner's pid.
#
# Environment: UPSTREAM_WALL_S (default 14400) stops the run; UPSTREAM_MIN_FREE_GB (45) is the data-disk space it
# needs (the GH #166 trial's /srv peaked at 42 GB apparent, mostly sparse, on the default 64 GiB disk); UPSTREAM_CPUS (4) and UPSTREAM_MEMORY_MIB (8192) size
# the System VM; UPSTREAM_KEEP_SIZE=1 leaves the System VM as it is (no restart either way), which needs it to share
# build/upstream-suite already.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT"
HERE="scripts/systemvm/upstream-suite"
B="$ROOT/build/upstream-suite"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
OUT="$ROOT/run/GH-172/$STAMP"
CFG="$ROOT/build/systemvm/launch-config.json"
SAVED="$B/launch-config.saved.json"
SRC_VOL=vesper-upstream-src
SRV_VOL=vesper-upstream-srv
CARGO_VOL=vesper-upstream-cargo
NAME=vesper-upstream-run
WALL_S="${UPSTREAM_WALL_S:-14400}"
RERUN_MAX="${UPSTREAM_RERUN_MAX:-8}"
export DOCKER_HOST="unix://$ROOT/run/vesper/docker.sock"

CLEAN=0
if [ "${1:-}" = --clean ]; then CLEAN=1; shift; fi
for a in "$@"; do case "$a" in -*) echo "a selection is node ids or paths, not pytest options: $a" >&2; exit 2 ;; esac; done
mkdir -p "$B"
# The lock is a symlink to the runner's pid: `ln -s` creates it and records the pid in one atomic step. A lock whose
# runner is gone is not taken over, since two runners could both decide to take it; the message says how to clear it.
LOCK="$B/runner.lock"
if ! ln -s "$$" "$LOCK" 2>/dev/null; then
  holder="$(readlink "$LOCK" 2>/dev/null || true)"
  if [ -n "$holder" ] && kill -0 "$holder" 2>/dev/null; then echo "another run (pid $holder) holds $LOCK" >&2
  else echo "$LOCK names pid ${holder:-?}, which is not running; if no run is in progress, rm $LOCK and retry" >&2; fi
  exit 2
fi
release_lock() { [ "$(readlink "$LOCK" 2>/dev/null)" = "$$" ] && rm -f "$LOCK"; true; }
trap release_lock EXIT
mkdir -p "$OUT"
# tee ignores INT and TERM, so a Ctrl-C cannot kill it and turn restore's next line into a SIGPIPE.
exec > >(trap '' INT TERM; exec tee -a "$OUT/runner.log") 2>&1
say() { echo "[upstream-suite $(date -u +%H:%M:%S)] $*"; }
# step NAME LOG CMD...: run a step whose output goes to LOG, and on failure say which step failed and show its tail.
step() {
  local name=$1 log=$2; shift 2
  "$@" > "$log" 2>&1 && return 0
  say "step failed: $name (exit $?); last lines of ${log#"$ROOT"/}:"; tail -20 "$log"; exit 2
}

# with_timeout SECS CMD...: macOS has no timeout(1); kill CMD if it outlives SECS.
with_timeout() {
  local secs=$1; shift
  "$@" & local pid=$!
  ( sleep "$secs" && kill "$pid" 2>/dev/null ) & local dog=$!
  wait "$pid"; local rc=$?
  kill "$dog" 2>/dev/null; wait "$dog" 2>/dev/null
  return "$rc"
}

eval "$(cargo run -q --bin baselinectl -- --mode upstream-pins)"
SELECTION=("$@")
PARTIAL=()
if [ ${#SELECTION[@]} -gt 0 ]; then PARTIAL=(--partial); else read -r -a SELECTION <<< "$TEST_PATHS"; fi
TAG="${RELEASE_ASSET#firecracker-}"; TAG="${TAG%%-*}"
ARCH="${ARTIFACTS_PREFIX##*/}"
ARTDIR="build/artifacts/${ARTIFACTS_PREFIX#firecracker-ci/}"
ARTDIR="${ARTDIR%/*}/$ARCH"
say "revision $FC_REVISION ($TAG), container $CONTAINER, artefacts $ARTIFACTS_PREFIX, selection: ${SELECTION[*]}"

# --- fetch (host side, cached) -----------------------------------------------------------------------------------
TREE="$B/firecracker"
# Steps that only need the tree mount the volume's firecracker/ at /firecracker (test containers: see "run").
FCVOL=(--mount "type=volume,src=$SRC_VOL,dst=/firecracker,volume-subpath=firecracker")
if [ "$(git -C "$TREE" rev-parse HEAD 2>/dev/null || true)" != "$FC_REVISION" ]; then
  say "fetch upstream tree"
  rm -rf "${TREE:?}" && git init -q "$TREE"
  GIT_HTTP_LOW_SPEED_LIMIT=1000 GIT_HTTP_LOW_SPEED_TIME=60 \
    git -C "$TREE" fetch -q --depth 1 "https://github.com/$FC_REPO.git" "$FC_REVISION"
  git -C "$TREE" -c advice.detachedHead=false checkout -q FETCH_HEAD
fi
mkdir -p "$TREE/$ARTDIR"
LISTING="$(curl -fsS --connect-timeout 20 --max-time 120 "https://s3.amazonaws.com/$ARTIFACTS_BUCKET?list-type=2&prefix=$ARTIFACTS_PREFIX/")"
case "$LISTING" in *"<IsTruncated>true</IsTruncated>"*) say "artefact listing is truncated"; exit 2 ;; esac
N=0
while IFS=' ' read -r key size; do
  [ -n "$key" ] || continue
  rel="${key#"$ARTIFACTS_PREFIX"/}"; dest="$TREE/$ARTDIR/$rel"; N=$((N + 1))
  case "$rel" in ""|/*|*..*) say "refusing artefact key $key"; exit 2 ;; esac
  if [ -f "$dest" ] && [ "$(wc -c < "$dest" | tr -d ' ')" = "$size" ]; then continue; fi
  say "GET $rel ($size bytes)"
  mkdir -p "$(dirname "$dest")"
  curl -fsS --connect-timeout 20 --max-time 1800 --retry 5 --retry-delay 5 -o "$dest.part" "https://s3.amazonaws.com/$ARTIFACTS_BUCKET/$key"
  [ "$(wc -c < "$dest.part" | tr -d ' ')" = "$size" ] || { say "size mismatch for $rel"; exit 2; }
  mv "$dest.part" "$dest"
done < <(printf '%s' "$LISTING" | python3 -I -c '
import re, sys
for c in re.findall(r"<Contents>(.*?)</Contents>", sys.stdin.read(), re.S):
    k = re.search(r"<Key>(.*?)</Key>", c).group(1); s = re.search(r"<Size>(\d+)</Size>", c).group(1)
    if not k.endswith("/"): print(k, s)')
[ "$N" -gt 0 ] || { say "no artefacts under $ARTIFACTS_PREFIX"; exit 2; }
# Sizes come from the bucket; digests from the committed manifest, which must name exactly the listed objects.
MANIFEST="docs/specs/upstream-suite-artifacts.sha256"
python3 -I - "$TREE/$ARTDIR" "$MANIFEST" <<'PY' || { say "artefacts do not match $MANIFEST"; exit 2; }
import hashlib, os, sys
root, manifest = sys.argv[1], sys.argv[2]
want = {}
for line in open(manifest):
    if line.strip() and not line.startswith("#"):
        key, size, digest = line.split()
        want[key] = (int(size), digest)
have = sorted(os.path.relpath(os.path.join(d, f), root) for d, _, fs in os.walk(root) for f in fs)
bad = sorted(set(have) ^ set(want))
for key in sorted(set(have) & set(want)):
    h = hashlib.sha256()
    with open(os.path.join(root, key), "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    if (os.path.getsize(os.path.join(root, key)), h.hexdigest()) != want[key]:
        bad.append(key)
if bad:
    sys.exit("mismatch: " + " ".join(bad))
PY
echo "$ARTDIR" > "$TREE/build/current_artifacts"
say "artefacts: $N objects"

REL="$B/release"
# The tarball's digest as systemvm/archives.yaml pins it: the sha256 line right after its url.
WANT_SHA="$(grep -A1 "url: .*/$RELEASE_ASSET\$" systemvm/archives.yaml | sed -n 's/^ *sha256: *//p' | head -1)"
[ -n "$WANT_SHA" ] || { say "$RELEASE_ASSET is not pinned in systemvm/archives.yaml"; exit 2; }
mkdir -p "$REL"
if [ "$(shasum -a 256 "$REL/$RELEASE_ASSET" 2>/dev/null | cut -d' ' -f1)" != "$WANT_SHA" ]; then
  say "GET $RELEASE_ASSET"
  curl -fsSL --connect-timeout 20 --max-time 1800 --retry 5 -o "$REL/$RELEASE_ASSET" "https://github.com/$FC_REPO/releases/download/$TAG/$RELEASE_ASSET"
  [ "$(shasum -a 256 "$REL/$RELEASE_ASSET" | cut -d' ' -f1)" = "$WANT_SHA" ] || { say "sha256 mismatch for $RELEASE_ASSET"; exit 2; }
fi
rm -rf "${REL:?}/bin" "$REL/x" && mkdir -p "$REL/bin" "$REL/x"
tar -xzf "$REL/$RELEASE_ASSET" -C "$REL/x"
for f in "$REL"/x/release-*/*-"$TAG"-"$ARCH" "$REL"/x/release-*/*-"$TAG"-"$ARCH".debug; do
  [ -f "$f" ] || continue
  base="${f##*/}"; base="${base/-$TAG-$ARCH/}"
  cp "$f" "$REL/bin/$base"
done
chmod 755 "$REL"/bin/*
say "release binaries: $(cd "$REL/bin" && ls | tr '\n' ' ')"

# --- System VM: size, share, restore on every exit ---------------------------------------------------------------
# $SAVED is the config before the resize; $SAVED.resized is the sha256 of the config this runner wrote. A saved copy
# is used only while the live config is still the one written here, so a config regenerated since (for example by
# make systemvm-up-snapshot) is never overwritten by a stale backup.
sha() { shasum -a 256 "$1" 2>/dev/null | cut -d' ' -f1; }
# The source volume's root is the tests' /srv and holds the tree in firecracker/ (see "run" below). Wipe everything
# else at its root, and the tree's test results, if the volume exists.
wipe_srv() {
  docker volume inspect "$SRC_VOL" >/dev/null 2>&1 || return 0
  docker run --rm -v "$SRC_VOL":/v "$HELPER_IMAGE" sh -c \
    'find /v -mindepth 1 -maxdepth 1 ! -name firecracker -exec rm -rf {} + && rm -rf /v/firecracker/test_results'
}
restore() {
  rc=$?
  trap '' INT TERM PIPE
  set +e
  docker rm -f "$NAME" >/dev/null 2>&1
  docker volume rm -f "$SRV_VOL" >/dev/null 2>&1
  wipe_srv
  if [ "${UPSTREAM_KEEP_SIZE:-0}" != 1 ] && [ -f "$SAVED" ]; then
    live="$(sha "$CFG")"
    if [ "$live" = "$(sha "$SAVED")" ]; then
      # The resize never replaced the config (an exit before it), but the System VM may be down: bring it back.
      say "the launch config is unchanged; restart the System VM with it"
      make -s docker-down >/dev/null 2>&1; make -s systemvm-down >/dev/null 2>&1
      if make -s systemvm-up >/dev/null 2>&1 && make -s docker-up >/dev/null 2>&1; then rm -f "$SAVED" "$SAVED.resized"
      else say "restart failed: run make systemvm-up docker-up (the saved config is in ${SAVED#"$ROOT"/})"; fi
    elif [ "$live" != "$(cat "$SAVED.resized" 2>/dev/null)" ]; then
      say "the launch config changed during the run; keeping it and discarding the saved copy"
      rm -f "$SAVED" "$SAVED.resized"
    else
      say "restore the previous launch config and restart the System VM"
      make -s docker-down >/dev/null 2>&1
      if ! make -s systemvm-down >/dev/null 2>&1; then
        say "systemvm-down failed; the saved config stays in ${SAVED#"$ROOT"/}: run make systemvm-down, copy it to ${CFG#"$ROOT"/}, then make systemvm-up docker-up"
      elif cp "$SAVED" "$CFG" && make -s systemvm-up >/dev/null 2>&1 && make -s docker-up >/dev/null 2>&1; then
        rm -f "$SAVED" "$SAVED.resized"
      else
        say "restart failed: run make systemvm-up docker-up (the saved config is in ${SAVED#"$ROOT"/})"
      fi
    fi
  fi
  release_lock
  exit "$rc"
}
trap restore EXIT
trap 'exit 130' INT TERM
if [ "${UPSTREAM_KEEP_SIZE:-0}" != 1 ]; then
  [ -f "$CFG" ] || { say "$CFG missing; run make systemvm-prepare"; exit 2; }
  if [ -f "$SAVED" ] && [ "$(sha "$CFG")" != "$(cat "$SAVED.resized" 2>/dev/null)" ]; then
    say "discarding a saved launch config from an earlier run: the live config was regenerated since"
    rm -f "$SAVED" "$SAVED.resized"
  fi
  [ -f "$SAVED" ] || cp "$CFG" "$SAVED"
  say "restart the System VM with ${UPSTREAM_CPUS:-4} vCPUs, ${UPSTREAM_MEMORY_MIB:-8192} MiB and $B shared"
  make -s docker-down >/dev/null 2>&1 || true
  make -s systemvm-down >/dev/null 2>&1 || { say "systemvm-down failed; the run needs a resized System VM"; exit 2; }
  # Write the resized config beside the live one and record its hash before it replaces the live one, so restore
  # can always tell which config is live.
  python3 -I - "$SAVED" "$CFG.new" "$B" "${UPSTREAM_CPUS:-4}" "${UPSTREAM_MEMORY_MIB:-8192}" <<'PY'
import json, sys
src, dst, share, cpus, mem = sys.argv[1:6]
c = json.load(open(src))
c["cpu_count"], c["memory_mib"] = int(cpus), int(mem)
shares = [s for s in c.get("shared_directories", []) if s.get("host_path") != share]
shares.append({"tag": f"share{len(shares)}", "host_path": share, "read_only": False, "watch": False})
for i, s in enumerate(shares):
    s["tag"] = f"share{i}"
c["shared_directories"] = shares
json.dump(c, open(dst, "w"), indent=2)
PY
  sha "$CFG.new" > "$SAVED.resized"
  mv "$CFG.new" "$CFG"
  make -s systemvm-up && make -s docker-up
fi
docker info >/dev/null || { say "the System VM's Docker is not reachable at $DOCKER_HOST"; exit 2; }
if [ "$CLEAN" = 1 ]; then docker volume rm -f "$SRC_VOL" "$CARGO_VOL" >/dev/null; fi
with_timeout 600 docker pull -q "$HELPER_IMAGE" >/dev/null || { say "pull of $HELPER_IMAGE failed or took over 600 s"; exit 2; }
FREE_GB="$(docker run --rm -v "$SRC_VOL":/v "$HELPER_IMAGE" df -Pk /v | awk 'NR==2 {print int($4 / 1048576)}')"
MIN_GB="${UPSTREAM_MIN_FREE_GB:-45}"
[ "$FREE_GB" -ge "$MIN_GB" ] || { say "the data disk has ${FREE_GB} GB free; the run needs $MIN_GB (UPSTREAM_MIN_FREE_GB)"; exit 2; }
docker run --rm --privileged --pid=host "$HELPER_IMAGE" nsenter -t 1 -m -u -n -i sh -c 'modprobe tun && test -c /dev/net/tun'
say "tun loaded; data disk ${FREE_GB} GB free"

# --- stage -------------------------------------------------------------------------------------------------------
with_timeout 1800 docker pull -q "$CONTAINER" >/dev/null || { say "pull failed; retrying once"
  with_timeout 1800 docker pull -q "$CONTAINER" >/dev/null || { say "pull of $CONTAINER failed"; exit 2; }; }
say "stage the tree into $SRC_VOL"
step stage "$OUT/stage.log" \
  docker run --rm -v "$TREE":/src:ro -v "$SRC_VOL":/v "$HELPER_IMAGE" timeout 1800 sh -c '
  # The tree lives in firecracker/ (a volume staged before that layout had it at the root: keep its cargo cache).
  set -e
  if [ ! -d /v/firecracker ] && [ -d /v/build/cargo_target ]; then
    mkdir -p /v/firecracker/build && mv /v/build/cargo_target /v/firecracker/build/
  fi
  find /v -mindepth 1 -maxdepth 1 ! -name firecracker -exec rm -rf {} +
  # Keep only build/cargo_target (the examples cache); setup-ci-artifacts.sh must start from the raw artefacts.
  mkdir -p /v/firecracker/build && cd /v/firecracker
  find . -mindepth 1 -maxdepth 1 ! -name build -exec rm -rf {} +
  find build -mindepth 1 -maxdepth 1 ! -name cargo_target -exec rm -rf {} +
  cp -a /src/. /v/firecracker/ && echo staged'
step setup-ci-artifacts "$OUT/setup-ci-artifacts.log" \
  docker run --rm "${FCVOL[@]}" -w /firecracker "$CONTAINER" timeout 1800 ./tools/setup-ci-artifacts.sh "$ARTDIR"
TARGET="build/cargo_target/$ARCH-unknown-linux-musl/release"
say "build the examples and tools that tests expect in $TARGET"
step build-examples "$OUT/build.log" \
  docker run --rm "${FCVOL[@]}" -v "$CARGO_VOL":/usr/local/rust/registry -w /firecracker \
  --env CARGO_TARGET_DIR=/firecracker/build/cargo_target "$CONTAINER" timeout 3600 bash -c "
  set -e
  cargo build -q --release --target $ARCH-unknown-linux-musl --examples
  cargo build -q --release --target $ARCH-unknown-linux-musl -p clippy-tracing
  cargo build -q --release --target $ARCH-unknown-linux-musl -p log-instrument --examples"
docker run --rm "${FCVOL[@]}" -v "$REL/bin":/rel:ro "$HELPER_IMAGE" sh -c "
  set -e; t=/firecracker/$TARGET; mkdir -p \$t/examples
  cp /rel/* \$t/
  ls \$t | tr '\n' ' '" > "$OUT/staged-tools.txt"
say "staged: $(cat "$OUT/staged-tools.txt")"

# --- run ---------------------------------------------------------------------------------------------------------
RES="$B/results/$STAMP"
mkdir -p "$RES"
common=(--privileged --security-opt seccomp=unconfined --ulimit core=0 --ulimit nofile=4096:4096
  --ulimit memlock=-1:-1 --volume /dev:/dev --volume "$SRC_VOL":/srv
  --volume "$RES":/results -v /boot:/boot --env PYTHONDONTWRITEBYTECODE=1
  --workdir /)
say "collect the selection"
# The framework hard-links files into each jail under /srv: the binaries and uffd examples from --binary-dir, and
# tests/framework files such as vm_config.json (test_invalid_bpf). link(2) fails with EXDEV across mounts, so the
# tree and /srv must share one mount. /srv must be a real directory, not a symlink: the jailer reports canonical
# paths, and test_jail's hard-link and symlink tests match its error against the /srv path. So each test container
# mounts the volume's root at /srv and makes /firecracker a symlink to /srv/firecracker.
SRVDIR=/srv
INSRV=(sh -c 'rm -rf /firecracker && ln -s /srv/firecracker /firecracker && cd /firecracker && exec "$@"' sh)
BIN=/srv/binaries
docker volume rm -f "$SRV_VOL" >/dev/null 2>&1 || true
wipe_srv
docker run --rm -v "$SRC_VOL":/srv "$HELPER_IMAGE" sh -c "
  set -e; t=/srv/firecracker/$TARGET; BIN=$SRVDIR/binaries; mkdir -p \$BIN/examples
  for f in \$t/*; do if [ -f \"\$f\" ]; then cp \"\$f\" \$BIN/; fi; done
  for f in \$t/examples/*; do if [ -f \"\$f\" ] && [ -x \"\$f\" ]; then cp \"\$f\" \$BIN/examples/; fi; done
  ls \$BIN \$BIN/examples | tr '\n' ' '" > "$OUT/binaries.txt"
docker run --rm "${common[@]}" "$CONTAINER" "${INSRV[@]}" timeout 900 ./tools/test.sh --binary-dir "$BIN" --collect-only -qqq \
  -p no:cacheprovider -- "${SELECTION[@]}" > "$OUT/collected.txt" 2>&1 || true
COLLECTED="$(grep -c '::' "$OUT/collected.txt" || true)"
[ "$COLLECTED" -gt 0 ] || { say "collected no tests (see collected.txt)"; exit 2; }
say "run $COLLECTED tests with $WORKERS workers, ${TEST_TIMEOUT_S} s per test, ${WALL_S} s in all"
docker rm -f "$NAME" >/dev/null 2>&1 || true
START=$(date +%s)
docker run -d --name "$NAME" "${common[@]}" "$CONTAINER" "${INSRV[@]}" ./tools/test.sh --binary-dir "$BIN" -rA \
  -n "$WORKERS" --dist worksteal --timeout "$TEST_TIMEOUT_S" --timeout-method=thread \
  --junitxml=/results/junit.xml --json-report-file=/results/report.json -- "${SELECTION[@]}" >/dev/null
STOPPED=""
while [ "$(docker inspect -f '{{.State.Running}}' "$NAME" 2>/dev/null)" = true ]; do
  if [ $(( $(date +%s) - START )) -ge "$WALL_S" ]; then
    STOPPED="wall-clock limit of $WALL_S s"; say "stop: $STOPPED"; docker kill "$NAME" >/dev/null; break
  fi
  sleep 30
done
RC="$(with_timeout 300 docker wait "$NAME" 2>/dev/null || echo 255)"
# A full run's log is hundreds of MB (tracebacks print every local), so it is written compressed.
docker logs "$NAME" 2>&1 | gzip -6 > "$OUT/pytest.log.gz"
docker rm "$NAME" >/dev/null
echo "rc=$RC wall_s=$(( $(date +%s) - START )) collected=$COLLECTED stopped=${STOPPED:-no} selection=${SELECTION[*]}" | tee "$OUT/run.txt"

# --- collect, wipe, compare --------------------------------------------------------------------------------------
[ -f "$RES/junit.xml" ] && gzip -9 -c "$RES/junit.xml" > "$OUT/junit.xml.gz"
# pytest-progress.log keeps each result line, the worker crashes and the short summary: small enough to commit
# beside results.json, unlike pytest.log.gz and report.json.gz.
{ gzip -dc "$OUT/pytest.log.gz" | grep -E '^\[gw[0-9]+\] |^replacing crashed|^worker .* crashed'
  gzip -dc "$OUT/pytest.log.gz" | awk '/short test summary info/{f=1} f'; } > "$OUT/pytest-progress.log" || true
if [ -s "$RES/report.json" ]; then
  python3 -I "$HERE/collect.py" "$OUT/results.json" --collected "$OUT/collected.txt" --meta "$OUT/meta.json" \
    --exitcode "$RC" --log "$OUT/pytest-progress.log" "$RES/report.json"
  gzip -c "$RES/report.json" > "$OUT/report.json.gz"
else
  say "no JSON report (the run was stopped); recovering finished tests from the progress log"
  python3 -I "$HERE/collect.py" "$OUT/results.json" --collected "$OUT/collected.txt" --meta "$OUT/meta.json" \
    --exitcode "$RC" --log "$OUT/pytest-progress.log"
fi
judge() {
  set +e
  cargo run -q --bin baselinectl -- --mode upstream-compare --results "$OUT/results.json" \
    --meta "$OUT/meta.json" ${PARTIAL[@]+"${PARTIAL[@]}"} --report "$OUT/compare.json" 2>&1 | tee "$OUT/compare.txt"
  CMP=${PIPESTATUS[0]}
  set -e
}
judge
# Isolated rerun (decision D-031): a few tests fail in each full run only because three workers load the host (an
# SSH or API timeout in a test that passes in other runs). The tests that matched no class run once more, one at a
# time; one that passes is recorded as passed with its result under load, and the comparator lists it. More than
# $RERUN_MAX unmatched tests is not timing, so nothing is rerun and the run fails.
python3 -I - "$OUT/compare.json" > "$OUT/unmatched.txt" <<'PY' || true
import json, sys
for u in json.load(open(sys.argv[1])).get("unmatched", []):
    print(u["nodeid"])
PY
N_UNMATCHED="$(grep -c . "$OUT/unmatched.txt" || true)"
if [ "$N_UNMATCHED" -gt 0 ] && [ -z "$STOPPED" ] && [ -s "$RES/report.json" ]; then
  if [ "$N_UNMATCHED" -gt "$RERUN_MAX" ]; then
    say "$N_UNMATCHED results match no class, more than UPSTREAM_RERUN_MAX ($RERUN_MAX): no isolated rerun"
  else
    say "rerun $N_UNMATCHED unmatched test(s) alone, one at a time"
    cp "$OUT/results.json" "$OUT/results-under-load.json"
    cp "$OUT/compare.json" "$OUT/compare-under-load.json"
    cp "$OUT/compare.txt" "$OUT/compare-under-load.txt"
    AGAIN=()
    while IFS= read -r n; do AGAIN+=("$n"); done < "$OUT/unmatched.txt"
    docker rm -f "$NAME" >/dev/null 2>&1 || true
    docker run -d --name "$NAME" "${common[@]}" "$CONTAINER" "${INSRV[@]}" ./tools/test.sh --binary-dir "$BIN" -rA \
      --timeout "$TEST_TIMEOUT_S" --timeout-method=thread \
      --json-report-file=/results/alone.json -- "${AGAIN[@]}" >/dev/null
    with_timeout $(( (TEST_TIMEOUT_S + 60) * N_UNMATCHED + 600 )) docker wait "$NAME" >/dev/null 2>&1 || docker kill "$NAME" >/dev/null 2>&1 || true
    docker logs "$NAME" 2>&1 | gzip -6 > "$OUT/pytest-alone.log.gz"
    docker rm -f "$NAME" >/dev/null 2>&1 || true
    if [ -s "$RES/alone.json" ]; then
      python3 -I "$HERE/collect.py" "$OUT/results.json" --collected "$OUT/collected.txt" --meta "$OUT/meta.json" \
        --exitcode "$RC" --log "$OUT/pytest-progress.log" --alone "$RES/alone.json" "$RES/report.json"
      gzip -c "$RES/alone.json" > "$OUT/alone.json.gz"
      judge
    else
      say "the isolated rerun wrote no report; the first verdict stands"
    fi
  fi
fi
rm -rf "${RES:?}"
wipe_srv
say "wiped /srv and the test results in $SRC_VOL"
say "evidence in ${OUT#"$ROOT"/}"
# A stopped run fails even if every finished test is explained: the tests it never reached prove nothing.
if [ -n "$STOPPED" ]; then say "the run was stopped ($STOPPED): failing it"; exit 2; fi
exit "$CMP"
