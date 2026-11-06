# systemvm/overlay

## Overview

The Alpine `apkovl` overlay applied to the System VM at boot. `scripts/systemvm/build-initramfs.py`
packs this tree (plus the pinned extra packages under `var/lib/vesper/apks/`) into
`localhost.apkovl.tar.gz`, appends it to the pinned initramfs as a second cpio archive at
`/vesper/localhost.apkovl.tar.gz`, and the kernel command line (`apkovl=/vesper/localhost.apkovl.tar.gz`
in `docs/specs/systemvm-image.lock.yaml`) makes the Alpine init unpack it over the tmpfs root before
OpenRC starts. See ADR 0002.

## Interface

- `etc/.default_boot_services`: empty marker; keeps the init's default boot services (devfs, mdev, modloop, …) enabled although an overlay is present.
- `etc/hostname`: `vesper`.
- `etc/rc.conf`: `rc_cgroup_mode="unified"` (Docker 28 needs cgroup v2; the live image's hybrid layout has no devices controller).
- `etc/runlevels/sysinit/cgroups`: enables the OpenRC `cgroups` service, which the live image does not run.
- `etc/network/interfaces`: `lo` loopback and `eth0` DHCP from the Virtualization.framework NAT; the `networking` service is started by `vesper-services` (not from a runlevel), so its DHCP wait does not delay the login prompt; it provides `net` for `docker`.
- `etc/init.d/vesper-services` (+ `etc/runlevels/default/vesper-services`): oneshot that `need`s `vesper-packages` and starts `networking` (then `/run/vesper/net-ready`), `containerd` and `docker` in a detached `setsid sh -c` with `env -i PATH=… rc-service <svc> start` (inside the runlevel change a plain `rc-service` only schedules the start; detached so the login prompt waits for neither DHCP nor dockerd); logs `vesper: networking up`, `vesper: service <svc> started` or the failure to the console; creates `/run/vesper/services-ready` when both daemons are up (the probe and `HEALTH` wait for it). Both daemons come from the pinned packages' OpenRC scripts: `supervisor=supervise-daemon` (respawn on crash) with `log_proxy` to `/var/log/containerd.log` and `/var/log/docker.log`.
- `etc/init.d/vesper-packages` (+ `etc/runlevels/default/vesper-packages`): OpenRC oneshot that runs `apk add --quiet --no-progress --force-non-repository /var/lib/vesper/apks/*.apk` (the files are not in a repository, so apk needs the flag; they are verified against the Alpine keys on the boot media); on success creates `/run/vesper/packages-ready`; logs `vesper: …` lines to `/dev/console`; fails the service otherwise. Other Vesper services `need` it, which is how OpenRC orders them after the install.
- `etc/init.d/vesper-agent` (+ `etc/runlevels/default/vesper-agent`): `supervise-daemon`-supervised `socat VSOCK-LISTEN:<port>,reuseaddr,fork EXEC:/usr/local/bin/vesper-agent` (port from `VESPER_AGENT_PORT`, default 1024), `respawn_delay=1`, unlimited respawns; `need localmount vesper-packages`.
- `etc/init.d/vesper-docker-relay` (+ `etc/runlevels/default/vesper-docker-relay`): `supervise-daemon`-supervised `socat VSOCK-LISTEN:<port>,reuseaddr,fork UNIX-CONNECT:/var/run/docker.sock` (port from `VESPER_DOCKER_PORT`, default 2375); `need localmount vesper-packages vesper-services`. The raw Docker and containerd sockets stay guest-local; this relay is the only path to Docker from the host.
- `var/lib/vesper/firecracker/{firecracker,jailer}` (from the packages lock's `archives`, placed by `build-initramfs.py --file`, mode 0755): Firecracker v1.16.0 aarch64 release binaries.
- `usr/local/bin/vesper-microvm`: jailed microVM manager; `prepare` (kernel from `/media/vda/boot/vmlinuz-virt` via `od`/`tail`/`head`/`gunzip`, busybox initramfs via `cpio` with the virtio-mmio, virtio-blk, crc32c and ext4 modules and `modules.dep` from the modloop), `launch ID [--exit] [--rootfs PATH] [--cmd BASE64]` (`--rootfs` copies the ext4 into the jail as `/rootfs.ext4`, attaches it as the root drive, passes `vesper_rootfs=1`; the init then prints `VESPER-ROOTFS-OK <image id>` and `VESPER-ROOTFS-EXEC-OK`, or `VESPER-ROOTFS-MOUNT-FAILED`; `--cmd` needs `--rootfs`, passes `vesper_cmd=BASE64`, and the init chroots into the rootfs, prints `VESPER-WORKLOAD-START`, the output and `VESPER-WORKLOAD-EXIT <code>`, then powers off) (`LAUNCHED ID pid=N boot_ms=N` after the `VESPER-MICROVM-OK` marker, else `FAILED ID` plus the log tail), `status ID` (`RUNNING|EXITED|ABSENT`), `isolation ID` (JSON: `uid`, `root`, `jail_files`, `cgroup`, `seccomp_mode`, `uid_ok`, `chroot_ok`, `cgroup_ok`, `seccomp_ok`), `stop ID` (SIGTERM, SIGKILL after 10 s), `crash ID` (SIGKILL), `cleanup ID` (`CLEAN ID` or `DIRTY ID <process|chroot|cgroup|mount|socket>`, exit 1), `log ID [N]`. Jails live under `/srv/jailer/firecracker/ID/root`, console logs under `/var/log/vesper-microvm-ID.log`, cgroups under `/sys/fs/cgroup/firecracker/ID`.
- `usr/local/bin/vesper-rootfs`: OCI image to ext4 pipeline; `import-trivial` (`IMPORTED <image id>`; busybox with every applet linked, musl, `/tmp`, `/proc`, `/dev`), `build IMAGE [--no-cache]` (`BUILT <path> sha256=<h> bytes=<n> cached=<bool>`, or `ERR image not found: …`, `ERR export failed: …`, `ERR mke2fs failed: …`), `provenance PATH` (the sidecar JSON, else `debugfs`), `inspect PATH` (loop-mount listing). Store `/var/lib/vesper/rootfs/`; baselines from `/var/lib/vesper/baselines.lock.yaml`.
- `var/lib/vesper/baselines.lock.yaml`: copy of `docs/specs/design-v0.baselines.lock.yaml` for embedded provenance.
- `usr/local/bin/vesper-workload`: microVM-per-container manager; `run NAME IMAGE -- CMD…` (`STARTED NAME microvm=wl-NAME pid=N image=ID`, or `ERR workload NAME exists`, `ERR rootfs build failed: …`, `ERR launch failed: …`, `ERR workload did not start: …`), `ps` (`NAME RUNNING|EXITED|STOPPED pid=N|- image=ID`), `logs NAME`, `wait NAME [TIMEOUT]` (`EXITED NAME code=N`), `stop NAME` (`STOPPED NAME teardown=CLEAN`), `rm NAME` (`REMOVED NAME` or `ERR leftovers for NAME: …`), `isolation NAME`. Store `/var/lib/vesper/workloads/` (`NAME.json` record, `NAME.ext4` private rootfs copy).
- `usr/local/bin/vesper-agent`: one process per connection; greets `VESPER-AGENT 1 <hostname>` and answers `PING`, `VERSION`, `ECHO <n>` (reads and returns `n` bytes), `HEALTH` (JSON: `packages_ready`, `services_ready`, OpenRC states of `containerd`, `docker`, `vesper-agent`, `containerd_ok` from `ctr --connect-timeout 2s version`, `docker_ok` and `docker_version` from `docker version`, `kvm` when `/dev/kvm` exists, which it does only when the launcher enabled nested virtualization), `KILL <containerd|docker>` (SIGKILL to the daemon, replies `KILL <name> <pid>`), `MICROVM <args…>` (runs `vesper-microvm <args>`, replies `MICROVM <rc> <count>` then the output lines), `ROOTFS <args…>` (same shape for `vesper-rootfs`), `WORKLOAD <args…>` (same shape for `vesper-workload`), `LOGS <messages|containerd|docker> <n>` (`LOGS <count>` then the lines), `DROP` (closes), `SHUTDOWN` (`BYE` then `poweroff`), `QUIT`; unknown commands get `ERR unknown command`, unknown services or logs `ERR unknown service|log`.

## Invariants

- Every file in this tree lands at the same path under `/` in the guest; the builder sets owner root, mode from the repository (executable bit preserved) and mtime 0, so the archive is deterministic.
- No secrets, no host paths, no package binaries live here: packages come from the cache that `scripts/systemvm/fetch-packages.sh` verifies against the packages lock.
- Scripts must be POSIX `sh` (busybox). A service that depends on an installed package must `need vesper-packages`.

## Side Effects

- At guest boot: `eth0` gets a DHCP lease; cgroup v2 is mounted; packages are installed into the tmpfs root (`/run/vesper/packages-ready`); `containerd` and `dockerd` start under `supervise-daemon` (`/run/vesper/services-ready`, logs under `/var/log/`); console lines prefixed `vesper:` are printed; `socat` listens on vsock port 1024 and spawns the agent per connection, and on vsock port 2375 relaying to the Docker socket.
- `KILL` sends SIGKILL to a daemon; supervise-daemon restarts it within about half a second.
- `MICROVM launch` starts a Firecracker process as uid 10000 in a jail with a cgroup; `cleanup` removes all of it.

## Verification

- `make systemvm-overlay` builds the archive; `make systemvm-boot-smoke` boots with it and the probe in the image lock requires `/run/vesper/packages-ready`, the installed binaries, the agent service and both runtime services; `make systemvm-up && make systemvm-services-suite` proves crash recovery.
