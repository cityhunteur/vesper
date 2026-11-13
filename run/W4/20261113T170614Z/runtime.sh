#!/bin/bash
# W4 runtime verification: restart with the snapshot share, hot-patch W4's helpers and shim, run the new bounds suite
# and the regression suites. Run from the main checkout (its run/ sockets and share); binaries come from ../vesper-w4.
set -u
MAIN=/Volumes/Workspace/github.com/cityhunteur/vesper
W4=/Volumes/Workspace/github.com/cityhunteur/vesper-w4
S=/private/tmp/claude-501/-Volumes-Workspace-github-com-cityhunteur-vesper/3347cb86-4f04-4eee-abd6-5fcb4ee67780/scratchpad/w4rt
mkdir -p "$S"
cd "$MAIN" || exit 1
export DOCKER_HOST=unix://$MAIN/run/vesper/docker.sock
H=$W4/systemvm/overlay/usr/local/bin

(cd "$W4" && cargo build -q --bins && bash scripts/systemvm/build-shim.sh) || { echo "build failed"; exit 1; }

make -s docker-down >/dev/null 2>&1; make -s systemvm-down >/dev/null 2>&1
make -s systemvm-up-snapshot && make -s docker-up || { echo "restart failed"; exit 1; }

put() { # put SRC DEST: copy with a timeout (stdin EOF through the proxy was lost once), check the hash, retry
  local want got try p dog
  want=$(shasum -a 256 "$1" | cut -d' ' -f1)
  for try in 1 2 3; do
    docker run --rm -i --privileged --net=host --pid=host -v /:/host alpine:3.22 chroot /host sh -c \
      "cat > $2.new && chmod 755 $2.new" < "$1" & p=$!
    ( sleep 60; kill $p 2>/dev/null ) & dog=$!
    wait $p; kill $dog 2>/dev/null
    got=$(docker run --rm --privileged -v /:/host alpine:3.22 sha256sum "/host$2.new" 2>/dev/null | cut -d' ' -f1)
    if [ "$got" = "$want" ]; then
      docker run --rm --privileged -v /:/host alpine:3.22 mv "/host$2.new" "/host$2" && return 0
    fi
    echo "put $2: try $try failed (hash ${got:-none})"
  done
  return 1
}
for f in vesper-boundlog vesper-memadmit vesper-vesperd vesper-microvm vesper-workload vesper-microvm-init; do
  put "$H/$f" "/usr/local/bin/$f" || { echo "put $f failed"; exit 1; }
done
put "$W4/build/shim/containerd-shim-vesper-v2" /usr/bin/containerd-shim-vesper-v2 || { echo "put shim failed"; exit 1; }
# The reaper is a long-running shell started at boot: restart it so it runs the patched vesper-vesperd.
docker run --rm --privileged --cgroupns=host --net=host --pid=host -v /:/host alpine:3.22 chroot /host \
  rc-service vesper-vesperd-reaper restart >/dev/null 2>&1 || { echo "reaper restart failed"; exit 1; }
echo patched

suite() { # suite NAME [ARGS...]
  local name=$1; shift
  "$W4/target/debug/systemvmctl" control "$name" --report "$S/$name.json" "$@" > "$S/$name.txt" 2>&1 & local p=$!
  ( sleep 1800; kill $p 2>/dev/null && echo "$name: killed after 1800 s" ) & local dog=$!
  wait $p; local rc=$?; kill $dog 2>/dev/null
  echo "$name exit=$rc"
}
V=(--vesperd "$W4/target/debug/vesperd")
suite bounds-suite "${V[@]}"
suite snapshot-suite "${V[@]}"
suite vesperd-suite "${V[@]}"
suite egress-suite "${V[@]}"
suite launchapi-suite
suite workload-suite
suite shim-suite
# Council gap: a guest must not stall the System VM if its console bounder dies (EPIPE on the FIFO).
docker run --rm --privileged --cgroupns=host --net=host --pid=host -v /:/host alpine:3.22 chroot /host sh -c '
  vesper-memadmit status
  vesper-microvm launch w4-bl-kill | head -1
  bl=$(pgrep -f "[v]esper-boundlog /var/log/vesper-microvm-w4-bl-kill.log" | head -1); echo "boundlog pid=$bl"
  kill -KILL $(pgrep -P "$bl") "$bl" 2>/dev/null; sleep 5
  fc=$(pgrep -f "[f]irecracker --id w4-bl-kill " | head -1); echo "firecracker pid=${fc:-none} after the bounder died"
  vesper-microvm status w4-bl-kill; uptime
  vesper-microvm cleanup w4-bl-kill --logs; vesper-memadmit status' > "$S/boundlog-kill.txt" 2>&1
echo "boundlog-kill: $(grep -c "firecracker pid=[0-9]" "$S/boundlog-kill.txt") alive"
echo done
