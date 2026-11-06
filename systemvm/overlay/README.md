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
- `etc/network/interfaces` + `etc/runlevels/boot/networking`: `lo` loopback and `eth0` DHCP from the Virtualization.framework NAT at boot, which also satisfies the `net` dependency of `docker`.
- `etc/init.d/vesper-services` (+ `etc/runlevels/default/vesper-services`): oneshot that `need`s `vesper-packages` and starts `containerd` then `docker` with `env -i PATH=… rc-service <svc> start` (inside the runlevel change a plain `rc-service` only schedules the start); logs `vesper: service <svc> started` or the failure to the console; creates `/run/vesper/services-ready`. Both daemons come from the pinned packages' OpenRC scripts: `supervisor=supervise-daemon` (respawn on crash) with `log_proxy` to `/var/log/containerd.log` and `/var/log/docker.log`.
- `etc/init.d/vesper-packages` (+ `etc/runlevels/default/vesper-packages`): OpenRC oneshot that runs `apk add --quiet --no-progress --force-non-repository /var/lib/vesper/apks/*.apk` (the files are not in a repository, so apk needs the flag; they are verified against the Alpine keys on the boot media); on success creates `/run/vesper/packages-ready`; logs `vesper: …` lines to `/dev/console`; fails the service otherwise. Other Vesper services `need` it, which is how OpenRC orders them after the install.
- `etc/init.d/vesper-agent` (+ `etc/runlevels/default/vesper-agent`): `supervise-daemon`-supervised `socat VSOCK-LISTEN:<port>,reuseaddr,fork EXEC:/usr/local/bin/vesper-agent` (port from `VESPER_AGENT_PORT`, default 1024), `respawn_delay=1`, unlimited respawns; `need localmount vesper-packages`.
- `usr/local/bin/vesper-agent`: one process per connection; greets `VESPER-AGENT 1 <hostname>` and answers `PING`, `VERSION`, `ECHO <n>` (reads and returns `n` bytes), `HEALTH` (JSON: `packages_ready`, `services_ready`, OpenRC states of `containerd`, `docker`, `vesper-agent`, `containerd_ok` from `ctr --connect-timeout 2s version`, `docker_ok` and `docker_version` from `docker version`), `KILL <containerd|docker>` (SIGKILL to the daemon, replies `KILL <name> <pid>`), `LOGS <messages|containerd|docker> <n>` (`LOGS <count>` then the lines), `DROP` (closes), `SHUTDOWN` (`BYE` then `poweroff`), `QUIT`; unknown commands get `ERR unknown command`, unknown services or logs `ERR unknown service|log`.

## Invariants

- Every file in this tree lands at the same path under `/` in the guest; the builder sets owner root, mode from the repository (executable bit preserved) and mtime 0, so the archive is deterministic.
- No secrets, no host paths, no package binaries live here: packages come from the cache that `scripts/systemvm/fetch-packages.sh` verifies against the packages lock.
- Scripts must be POSIX `sh` (busybox). A service that depends on an installed package must `need vesper-packages`.

## Side Effects

- At guest boot: `eth0` gets a DHCP lease; cgroup v2 is mounted; packages are installed into the tmpfs root (`/run/vesper/packages-ready`); `containerd` and `dockerd` start under `supervise-daemon` (`/run/vesper/services-ready`, logs under `/var/log/`); console lines prefixed `vesper:` are printed; `socat` listens on vsock port 1024 and spawns the agent per connection.
- `KILL` sends SIGKILL to a daemon; supervise-daemon restarts it within about half a second.

## Verification

- `make systemvm-overlay` builds the archive; `make systemvm-boot-smoke` boots with it and the probe in the image lock requires `/run/vesper/packages-ready`, the installed binaries, the agent service and both runtime services; `make systemvm-up && make systemvm-services-suite` proves crash recovery.
