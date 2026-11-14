#!/bin/sh
# Spike 2 (GH #194): dockerd userns-remap, corrected probes. Each probe runs once and its own exit status is kept.
# One-off: it restarts dockerd twice and restores the daemon from a trap. Not a suite; do not rerun on a VM in use.
S=$(dirname "$0"); O=$S/out.txt
echo $$ > /sys/fs/cgroup/cgroup.procs || exit 1
exec > "$O" 2>&1
IMG=alpine:3.22
restore() {
  echo "=== restore"
  rm -f /etc/docker/daemon.json; rc-service docker restart >/dev/null 2>&1
  i=0; until docker info >/dev/null 2>&1 || [ $i -ge 60 ]; do sleep 1; i=$((i+1)); done
  docker info --format 'after: {{json .SecurityOptions}} root={{.DockerRootDir}}'
  ls -ld /var/lib/docker; chgrp root /var/lib/docker; ls -ld /var/lib/docker
  umount /run/spk-ns 2>/dev/null; rm -f /run/spk-ns /run/spike-secret /tmp/spike-alpine.tar
  rm -rf /var/lib/docker/100000.100000
  sed -i '/^dockremap:/d' /etc/subuid /etc/subgid 2>/dev/null; deluser dockremap 2>/dev/null; delgroup dockremap 2>/dev/null
  echo SPIKE-DONE
}
trap restore EXIT
t() { n=$1; shift; echo "=== $n: docker run $*"; out=$(timeout 90 docker run --rm "$@" 2>&1); rc=$?; printf '%s\n' "$out" | grep -v "^Run 'docker\|^$" | tail -40 | cut -c1-400; echo "docker_rc=$rc"; }
docker save $IMG -o /tmp/spike-alpine.tar
addgroup -S dockremap; adduser -S -D -H -G dockremap dockremap
echo dockremap:100000:65536 >> /etc/subuid; echo dockremap:100000:65536 >> /etc/subgid
mkdir -p /etc/docker; echo '{"userns-remap":"default"}' > /etc/docker/daemon.json
rc-service docker restart >/dev/null 2>&1; i=0; until docker info >/dev/null 2>&1 || [ $i -ge 60 ]; do sleep 1; i=$((i+1)); done
docker info --format 'during: {{json .SecurityOptions}} root={{.DockerRootDir}}'
docker load -i /tmp/spike-alpine.tar
touch /run/spk-ns; unshare --net=/run/spk-ns true
echo rootonly > /run/spike-secret; chmod 600 /run/spike-secret
MOD=; for m in nbd zram ifb bonding; do lsmod | grep -q "^$m " || { modinfo $m >/dev/null 2>&1 && MOD=$m && break; }; done
echo "module-under-test=$MOD ifb-loaded-before=$(lsmod | grep -c '^ifb ') $MOD-loaded-before=$(lsmod | grep -c "^$MOD ")"
ls -l /dev/kvm /dev/vsock /dev/net/tun /dev/fuse /dev/vda 2>&1
ls -ld /run/docker.sock /run/containerd/containerd.sock /var/log/vesper* /var/lib/vesper /var/lib/vesper/* /run/vesper /run/vesper/* 2>&1 | cut -c1-120
# One line per probe: OK or NO, the command, and the first 100 bytes it printed.
PROBE='p() { out=$(sh -c "$1" 2>&1); rc=$?; [ $rc -eq 0 ] && r=OK || r=NO; echo "$r rc=$rc  $1 :: $(printf "%s" "$out" | head -c 100 | tr "\n" " ")"; }
 grep CapEff /proc/self/status; cat /proc/self/uid_map
 p "cat /host/run/spike-secret"
 p "nsenter --net=/host/run/spk-ns true"
 p "head -c1 /host/dev/vda"
 p "head -c0 /host/dev/kvm"
 p "head -c0 /host/dev/vsock"
 p "head -c0 /host/dev/fuse"
 p "head -c0 /host/dev/net/tun"
 p "chroot /host /sbin/modprobe MODX"
 p "ip link add i0 type ifb"
 p "chroot /host docker ps"
 p "chroot /host ctr -a /run/containerd/containerd.sock version"
 p "echo PING | chroot /host timeout 3 socat -T2 - VSOCK-CONNECT:2:1024"
 p "echo PING | chroot /host timeout 3 socat -T2 - VSOCK-CONNECT:1:1024"
 p "echo PING | chroot /host timeout 3 socat -T2 - VSOCK-CONNECT:3:1024"
 p "cat /host/var/lib/docker/100000.100000/volumes/victim/_data/secret"
 p "hostname pwned"
 p "ls /host/var/lib/vesper/workloads"
 p "head -c 20 /host/var/log/docker.log"
 p "ls /host/run/vesper/microvm"'
PROBE=$(printf '%s' "$PROBE" | sed "s/MODX/$MOD/")
docker volume create victim >/dev/null; t victim-write -v victim:/v $IMG sh -c 'echo topsecret > /v/secret; chmod 600 /v/secret; ls -ln /v'
t default -v /:/host $IMG sh -c "$PROBE"
echo "after default: ifb=$(lsmod | grep -c '^ifb ') $MOD=$(lsmod | grep -c "^$MOD ")"
t capall --cap-add ALL --security-opt seccomp=unconfined --security-opt apparmor=unconfined -v /:/host $IMG sh -c "$PROBE"
echo "after capall: ifb=$(lsmod | grep -c '^ifb ') $MOD=$(lsmod | grep -c "^$MOD ")"
for f in --ipc=host --uts=host --cgroupns=host "--device /dev/kvm" "--device /dev/vsock" "--device-cgroup-rule=b 253:* rwm" --privileged --pid=host --network=host "--security-opt systempaths=unconfined"; do
  # shellcheck disable=SC2086
  eval "t 'flag[$f]' --cap-add ALL --security-opt seccomp=unconfined $f -v /:/host -v /sys/fs/cgroup:/cg $IMG sh -c 'cat /proc/self/uid_map; hostname pwned 2>&1; mkdir /cg/spk 2>&1; ls -l /dev/kvm /dev/vsock 2>&1; head -c0 /dev/kvm 2>&1 && echo KVM-OPEN; mknod /tmp/vda b 253 0 2>&1 && head -c1 /tmp/vda >/dev/null 2>&1 && echo DISK-READ; echo done'"
done
echo "=== build"; mkdir -p /tmp/spkb; printf 'FROM alpine:3.22\nRUN cat /proc/self/uid_map; cat /proc/self/status | grep CapEff\n' > /tmp/spkb/Dockerfile
docker buildx version 2>&1 | head -1
timeout 120 docker build --no-cache --progress=plain -t spkb /tmp/spkb 2>&1 | tail -15 | cut -c1-200; echo "build_rc=$?"
timeout 120 docker build --no-cache --progress=plain --network=host -t spkb2 /tmp/spkb 2>&1 | tail -8 | cut -c1-200
DOCKER_BUILDKIT=0 timeout 120 docker build --no-cache -t spkb3 /tmp/spkb 2>&1 | tail -8 | cut -c1-200
rm -rf /tmp/spkb
echo "=== usernshost (must be refused by Vesper)"; t usernshost --userns=host -v /:/host $IMG sh -c 'cat /proc/self/uid_map; cat /host/run/spike-secret'
docker volume rm victim >/dev/null 2>&1
