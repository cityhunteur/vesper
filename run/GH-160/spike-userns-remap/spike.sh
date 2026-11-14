#!/bin/sh
# Spike: dockerd userns-remap in the live System VM. Restores the daemon at the end.
S=$(dirname "$0"); O=$S/out.txt; SH=$(dirname "$S")
echo $$ > /sys/fs/cgroup/cgroup.procs
exec > "$O" 2>&1
t() { n=$1; shift; echo "=== $n: docker run $*"; timeout 60 docker run --rm "$@" 2>&1 | tail -25 | cut -c1-600; echo "rc=$?"; }
set -x
docker save alpine:3.22 -o /tmp/spike-alpine.tar
addgroup -S dockremap; adduser -S -D -H -G dockremap dockremap
echo dockremap:100000:65536 >> /etc/subuid; echo dockremap:100000:65536 >> /etc/subgid
mkdir -p /etc/docker; echo '{"userns-remap":"default"}' > /etc/docker/daemon.json
rc-service docker restart; i=0; until docker info >/dev/null 2>&1 || [ $i -ge 60 ]; do sleep 1; i=$((i+1)); done
docker info --format '{{json .SecurityOptions}} root={{.DockerRootDir}}'
docker load -i /tmp/spike-alpine.tar
touch /run/spk-ns; unshare --net=/run/spk-ns true; ls -ld /var/lib/docker /var/lib/docker/100000.100000
echo rootonly > /run/spike-secret; chmod 600 /run/spike-secret
mkdir -p "$S/d"; echo secret > "$S/d/f600"; chmod 600 "$S/d/f600"
set +x
PROBE='id; cat /proc/self/uid_map; grep Cap /proc/self/status | tr "\n" " "; echo;
 for c in "cat /host/run/spike-secret" "cat /host/etc/shadow" "nsenter --net=/host/run/spk-ns true" "nsenter -t 1 -n true" "nsenter -t 1 -m true" "head -c1 /dev/vda" "head -c0 /dev/kvm" "ls /host/run/vesper/microvm" "cat /host/proc/1/environ" "mount -t tmpfs x /mnt" "ip link add d0 type dummy" "chroot /host /sbin/modprobe dummy" "ls /host/sys/kernel/security"; do
   if out=$(sh -c "$c" 2>&1 | head -c 120 | tr "\n" " "); then :; fi; sh -c "$c" >/dev/null 2>&1 && echo "ALLOWED  $c" || echo "DENIED   $c :: $out"; done'
t plain alpine:3.22 sh -c 'id; cat /proc/self/uid_map'
t share -v "$S/d:/s" alpine:3.22 sh -c 'ls -ln /s; cat /s/f600; echo w > /s/new && echo WRITE-OK; ls -ln /s'
t share-ro -v "$S/d:/s:ro" alpine:3.22 sh -c 'echo w > /s/new2 || echo RO-HELD'
t hostbind -v /:/host alpine:3.22 sh -c "$PROBE"
t privileged --privileged -v /:/host alpine:3.22 sh -c "$PROBE"
t pidhost --pid=host -v /:/host alpine:3.22 sh -c "$PROBE"
t nethost --net=host -v /:/host alpine:3.22 sh -c "$PROBE"
t priv-pid-net --privileged --pid=host --net=host --cgroupns=host -v /:/host alpine:3.22 sh -c "$PROBE"
t capadd --cap-add ALL --security-opt seccomp=unconfined --security-opt apparmor=unconfined -v /:/host alpine:3.22 sh -c "$PROBE"
t device --device /dev/vda alpine:3.22 sh -c 'ls -ln /dev/vda; head -c1 /dev/vda >/dev/null && echo DISK-READ || echo DISK-DENIED'
t usernshost --userns=host --privileged --pid=host --net=host -v /:/host alpine:3.22 sh -c "$PROBE"
t usernshost-plain --userns=host -v /:/host alpine:3.22 sh -c "$PROBE"
echo "=== volume bind via local driver"; docker volume create --driver local -o type=none -o o=bind -o device=/run spkvol; t volbind -v spkvol:/v alpine:3.22 sh -c 'cat /v/spike-secret'; docker volume rm spkvol
echo "=== exec --privileged"; docker run -d --name spk alpine:3.22 sleep 60; docker exec --privileged spk sh -c 'cat /proc/self/uid_map; grep CapEff /proc/self/status'; docker rm -f spk
echo "=== ls after"; ls -ln "$S/d"
echo "=== restore"
set -x
rm -f /etc/docker/daemon.json; rc-service docker restart; i=0; until docker info >/dev/null 2>&1 || [ $i -ge 60 ]; do sleep 1; i=$((i+1)); done
docker info --format '{{json .SecurityOptions}}'; docker images --format '{{.Repository}}:{{.Tag}}' | head -5
umount /run/spk-ns; rm -f /run/spk-ns; rm -f /run/spike-secret /tmp/spike-alpine.tar
ls -d /var/lib/docker/1*.* && rm -rf /var/lib/docker/100000.100000; rm -rf "$S/d"
sed -i '/^dockremap:/d' /etc/subuid /etc/subgid; deluser dockremap; delgroup dockremap 2>/dev/null
echo SPIKE-DONE
