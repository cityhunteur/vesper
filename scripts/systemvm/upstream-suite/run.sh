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
# compare (baselinectl --mode upstream-compare). Exit status is the comparator's.
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
export DOCKER_HOST="unix://$ROOT/run/vesper/docker.sock"

CLEAN=0
if [ "${1:-}" = --clean ]; then CLEAN=1; shift; fi
mkdir -p "$B" "$OUT"
exec > >(tee -a "$OUT/runner.log") 2>&1
say() { echo "[upstream-suite $(date -u +%H:%M:%S)] $*"; }

eval "$(cargo run -q --bin baselinectl -- --mode upstream-pins)"
SELECTION=("$@")
[ ${#SELECTION[@]} -gt 0 ] || read -r -a SELECTION <<< "$TEST_PATHS"
TAG="${RELEASE_ASSET#firecracker-}"; TAG="${TAG%%-*}"
ARCH="${ARTIFACTS_PREFIX##*/}"
ARTDIR="build/artifacts/${ARTIFACTS_PREFIX#firecracker-ci/}"
ARTDIR="${ARTDIR%/*}/$ARCH"
say "revision $FC_REVISION ($TAG), container $CONTAINER, artefacts $ARTIFACTS_PREFIX, selection: ${SELECTION[*]}"

# --- fetch (host side, cached) -----------------------------------------------------------------------------------
TREE="$B/firecracker"
if [ "$(git -C "$TREE" rev-parse HEAD 2>/dev/null || true)" != "$FC_REVISION" ]; then
  say "fetch upstream tree"
  rm -rf "${TREE:?}" && git init -q "$TREE"
  git -C "$TREE" fetch -q --depth 1 "https://github.com/$FC_REPO.git" "$FC_REVISION"
  git -C "$TREE" -c advice.detachedHead=false checkout -q FETCH_HEAD
fi
mkdir -p "$TREE/$ARTDIR"
LISTING="$(curl -fsS "https://s3.amazonaws.com/$ARTIFACTS_BUCKET?list-type=2&prefix=$ARTIFACTS_PREFIX/")"
case "$LISTING" in *"<IsTruncated>true</IsTruncated>"*) say "artefact listing is truncated"; exit 2 ;; esac
N=0
while IFS=' ' read -r key size; do
  [ -n "$key" ] || continue
  rel="${key#"$ARTIFACTS_PREFIX"/}"; dest="$TREE/$ARTDIR/$rel"; N=$((N + 1))
  case "$rel" in ""|/*|*..*) say "refusing artefact key $key"; exit 2 ;; esac
  if [ -f "$dest" ] && [ "$(wc -c < "$dest" | tr -d ' ')" = "$size" ]; then continue; fi
  say "GET $rel ($size bytes)"
  mkdir -p "$(dirname "$dest")"
  curl -fsS --retry 5 --retry-delay 5 -o "$dest.part" "https://s3.amazonaws.com/$ARTIFACTS_BUCKET/$key"
  [ "$(wc -c < "$dest.part" | tr -d ' ')" = "$size" ] || { say "size mismatch for $rel"; exit 2; }
  mv "$dest.part" "$dest"
done < <(printf '%s' "$LISTING" | python3 -I -c '
import re, sys
for c in re.findall(r"<Contents>(.*?)</Contents>", sys.stdin.read(), re.S):
    k = re.search(r"<Key>(.*?)</Key>", c).group(1); s = re.search(r"<Size>(\d+)</Size>", c).group(1)
    if not k.endswith("/"): print(k, s)')
[ "$N" -gt 0 ] || { say "no artefacts under $ARTIFACTS_PREFIX"; exit 2; }
echo "$ARTDIR" > "$TREE/build/current_artifacts"
say "artefacts: $N objects"

REL="$B/release"
# The tarball's digest as systemvm/archives.yaml pins it: the sha256 line right after its url.
WANT_SHA="$(grep -A1 "url: .*/$RELEASE_ASSET\$" systemvm/archives.yaml | sed -n 's/^ *sha256: *//p' | head -1)"
[ -n "$WANT_SHA" ] || { say "$RELEASE_ASSET is not pinned in systemvm/archives.yaml"; exit 2; }
mkdir -p "$REL"
if [ "$(shasum -a 256 "$REL/$RELEASE_ASSET" 2>/dev/null | cut -d' ' -f1)" != "$WANT_SHA" ]; then
  say "GET $RELEASE_ASSET"
  curl -fsSL --retry 5 -o "$REL/$RELEASE_ASSET" "https://github.com/$FC_REPO/releases/download/$TAG/$RELEASE_ASSET"
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
restore() {
  rc=$?
  set +e
  docker rm -f "$NAME" >/dev/null 2>&1
  docker volume rm -f "$SRV_VOL" >/dev/null 2>&1
  if [ "${UPSTREAM_KEEP_SIZE:-0}" != 1 ] && [ -f "$SAVED" ]; then
    say "restore the previous launch config and restart the System VM"
    make -s docker-down >/dev/null 2>&1; make -s systemvm-down >/dev/null 2>&1
    cp "$SAVED" "$CFG" && rm -f "$SAVED"
    make -s systemvm-up >/dev/null 2>&1 && make -s docker-up >/dev/null 2>&1 || say "restart failed: run make systemvm-up docker-up"
  fi
  exit "$rc"
}
trap restore EXIT
trap 'exit 130' INT TERM
if [ "${UPSTREAM_KEEP_SIZE:-0}" != 1 ]; then
  [ -f "$CFG" ] || { say "$CFG missing; run make systemvm-prepare"; exit 2; }
  [ -f "$SAVED" ] || cp "$CFG" "$SAVED"
  say "restart the System VM with ${UPSTREAM_CPUS:-4} vCPUs, ${UPSTREAM_MEMORY_MIB:-8192} MiB and $B shared"
  make -s docker-down >/dev/null 2>&1 || true; make -s systemvm-down >/dev/null 2>&1 || true
  python3 -I - "$SAVED" "$CFG" "$B" "${UPSTREAM_CPUS:-4}" "${UPSTREAM_MEMORY_MIB:-8192}" <<'PY'
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
  make -s systemvm-up && make -s docker-up
fi
docker info >/dev/null || { say "the System VM's Docker is not reachable at $DOCKER_HOST"; exit 2; }
FREE_GB="$(docker run --rm -v "$SRC_VOL":/v alpine:3.22 df -Pk /v | awk 'NR==2 {print int($4 / 1048576)}')"
MIN_GB="${UPSTREAM_MIN_FREE_GB:-45}"
[ "$FREE_GB" -ge "$MIN_GB" ] || { say "the data disk has ${FREE_GB} GB free; the run needs $MIN_GB (UPSTREAM_MIN_FREE_GB)"; exit 2; }
docker run --rm --privileged --pid=host alpine:3.22 nsenter -t 1 -m -u -n -i sh -c 'modprobe tun && test -c /dev/net/tun'
say "tun loaded; data disk ${FREE_GB} GB free"

# --- stage -------------------------------------------------------------------------------------------------------
if [ "$CLEAN" = 1 ]; then docker volume rm -f "$SRC_VOL" "$CARGO_VOL" >/dev/null; fi
docker pull -q "$CONTAINER" >/dev/null || { say "pull failed; retrying once"; docker pull -q "$CONTAINER" >/dev/null; }
say "stage the tree into $SRC_VOL"
docker run --rm -v "$TREE":/src:ro -v "$SRC_VOL":/firecracker alpine:3.22 sh -c '
  # Keep only build/cargo_target (the examples cache); setup-ci-artifacts.sh must start from the raw artefacts.
  cd /firecracker && find . -mindepth 1 -maxdepth 1 ! -name build -exec rm -rf {} + && mkdir -p build
  find build -mindepth 1 -maxdepth 1 ! -name cargo_target -exec rm -rf {} +
  cp -a /src/. /firecracker/ && echo staged'
docker run --rm -v "$SRC_VOL":/firecracker -w /firecracker "$CONTAINER" ./tools/setup-ci-artifacts.sh "$ARTDIR" > "$OUT/setup-ci-artifacts.log" 2>&1
TARGET="build/cargo_target/$ARCH-unknown-linux-musl/release"
say "build the examples and tools that tests expect in $TARGET"
docker run --rm -v "$SRC_VOL":/firecracker -v "$CARGO_VOL":/usr/local/rust/registry -w /firecracker \
  --env CARGO_TARGET_DIR=/firecracker/build/cargo_target "$CONTAINER" bash -c "
  set -e
  cargo build -q --release --target $ARCH-unknown-linux-musl --examples
  cargo build -q --release --target $ARCH-unknown-linux-musl -p clippy-tracing
  cargo build -q --release --target $ARCH-unknown-linux-musl -p log-instrument --examples" > "$OUT/build.log" 2>&1
docker run --rm -v "$SRC_VOL":/firecracker -v "$REL/bin":/rel:ro alpine:3.22 sh -c "
  set -e; t=/firecracker/$TARGET; mkdir -p \$t/examples
  cp /rel/* \$t/
  ls \$t | tr '\n' ' '" > "$OUT/staged-tools.txt"
say "staged: $(cat "$OUT/staged-tools.txt")"

# --- run ---------------------------------------------------------------------------------------------------------
RES="$B/results/$STAMP"
mkdir -p "$RES"
common=(--privileged --security-opt seccomp=unconfined --ulimit core=0 --ulimit nofile=4096:4096
  --ulimit memlock=-1:-1 --volume /dev:/dev --volume "$SRC_VOL":/firecracker --volume "$SRV_VOL":/srv
  --volume "$REL/bin":/binaries:ro --volume "$RES":/results -v /boot:/boot --env PYTHONDONTWRITEBYTECODE=1
  --workdir /firecracker)
say "collect the selection"
docker run --rm "${common[@]}" "$CONTAINER" ./tools/test.sh --binary-dir /binaries --collect-only -qqq \
  -p no:cacheprovider "${SELECTION[@]}" > "$OUT/collected.txt" 2>&1 || true
COLLECTED="$(grep -c '::' "$OUT/collected.txt" || true)"
[ "$COLLECTED" -gt 0 ] || { say "collected no tests (see collected.txt)"; exit 2; }
say "run $COLLECTED tests with $WORKERS workers, ${TEST_TIMEOUT_S} s per test, ${WALL_S} s in all"
docker rm -f "$NAME" >/dev/null 2>&1 || true
START=$(date +%s)
docker run -d --name "$NAME" "${common[@]}" "$CONTAINER" ./tools/test.sh --binary-dir /binaries -rA \
  -n "$WORKERS" --dist worksteal --timeout "$TEST_TIMEOUT_S" --timeout-method=thread \
  --junitxml=/results/junit.xml --json-report-file=/results/report.json "${SELECTION[@]}" >/dev/null
STOPPED=""
while [ "$(docker inspect -f '{{.State.Running}}' "$NAME" 2>/dev/null)" = true ]; do
  if [ $(( $(date +%s) - START )) -ge "$WALL_S" ]; then
    STOPPED="wall-clock limit of $WALL_S s"; say "stop: $STOPPED"; docker kill "$NAME" >/dev/null; break
  fi
  sleep 30
done
RC="$(docker wait "$NAME" 2>/dev/null || echo 255)"
docker logs "$NAME" > "$OUT/pytest.log" 2>&1
docker rm "$NAME" >/dev/null
echo "rc=$RC wall_s=$(( $(date +%s) - START )) collected=$COLLECTED stopped=${STOPPED:-no} selection=${SELECTION[*]}" | tee "$OUT/run.txt"

# --- collect, wipe, compare --------------------------------------------------------------------------------------
cp "$RES"/junit.xml "$OUT/" 2>/dev/null || true
if [ -s "$RES/report.json" ]; then
  python3 -I "$HERE/collect.py" "$OUT/results.json" --collected "$OUT/collected.txt" "$RES/report.json"
  gzip -c "$RES/report.json" > "$OUT/report.json.gz"
else
  say "no JSON report (the run was stopped); recovering finished tests from the progress log"
  python3 -I "$HERE/collect.py" "$OUT/results.json" --collected "$OUT/collected.txt" --log "$OUT/pytest.log"
fi
rm -rf "${RES:?}"
docker volume rm -f "$SRV_VOL" >/dev/null
docker run --rm -v "$SRC_VOL":/f alpine:3.22 rm -rf /f/test_results
say "wiped /srv and the test results in $SRC_VOL"
set +e
cargo run -q --bin baselinectl -- --mode upstream-compare --results "$OUT/results.json" \
  --report "$OUT/compare.json" | tee "$OUT/compare.txt"
CMP=${PIPESTATUS[0]}
set -e
say "evidence in ${OUT#"$ROOT"/}"
exit "$CMP"
