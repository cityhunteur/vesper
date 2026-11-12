---
status: verified
owner: cityhunteur
last_reviewed: 2026-11-12
---

# System VM Build and Boot Pipeline (Phase 1)

This project pins a real System VM boot image, verifies it end to end, generates a NAT-default launch
configuration, boots it on Virtualization.framework through `vesper-vmlauncher` (ADR
[0001](../design-docs/adr/0001-systemvm-launcher-swift-helper.md)), and captures repeated boot diagnostics.

## Pinned image

`docs/specs/systemvm-image.lock.yaml` names the upstream release (Alpine Linux 3.22.8 `virt` ISO for `aarch64`,
kernel `6.12.115-0-virt`), its URL and SHA-256, the two ISO members that are used and how each is unpacked, the
boot media, the kernel command line, the console ready pattern, and `expected_image_id`. The lock is parsed only in
Rust (`systemvm::load_image_lock`, `deny_unknown_fields`); the shell never interprets the YAML.

- Kernel: `boot/vmlinuz-virt` is a Linux EFI zboot PE (`MZ..zimg`) wrapping a gzip payload, which
  Virtualization.framework cannot start. `scripts/systemvm/unpack-zboot.py` extracts the raw ARM64 `Image`
  (`build/systemvm/Image-virt`); both the member and the unpacked file are digest-pinned.
- Initramfs: `boot/initramfs-virt` is used as is (`build/systemvm/initramfs-virt`).
- Boot media: the ISO itself is copied to `build/systemvm/` and attached read-only as the first virtio block
  device, so the initramfs finds its modloop and apks offline and boots to a login prompt. No block rootfs is
  written; a writable rootfs for containerd arrives with the Phase 1 containerd slice.
- Boot: `kernel_cmdline` is the ISO's grub line with the console moved to the virtio console (`console=hvc0
  modules=loop,squashfs,sd-mod,usb-storage`); `ready_pattern` is `login:`; `max_boot_ms` is the agreed boot-time
  threshold (5000, 2026-11-06), checked by boot smoke against the launcher's `ready` time.
- Probe (`boot.probe`): after the login prompt the launcher logs in as `root` (the live ISO has no password),
  waits for `/run/vesper/net-ready` (`eth0` configured by DHCP from the Virtualization.framework NAT by the
  detached `vesper-services` unit), pings the default gateway
  (`VESPER-NAT-OK`), waits for `/run/vesper/packages-ready` and checks that `socat`, `containerd` and `dockerd`
  are installed (`VESPER-EXTRAS-OK`), prints `VESPER-PROBE-OK` and runs `poweroff`, so every boot proves NAT and
  the overlay from inside the guest and ends with a clean shutdown. Markers are split in the typed commands
  because the console echoes input.

## Nested virtualization and backend selection (GH #17)

Backend A runs stock Firecracker under KVM inside the System VM, which needs nested virtualization: macOS 15 or
later on an Apple M3 or later. `build/vmlauncher/vesper-vmlauncher --host-capabilities` prints the host facts
(`api_available`, `nested_virtualization_supported`, `virtualization_supported`, `macos`, `chip`, `model`) and
`cargo run --bin systemvmctl -- backend-select [--policy auto|require|off]` turns them into a selection:
`backend-a` with `nested_virtualization: true` when supported; otherwise `backend-b` (`implemented: false`) with
diagnostics naming the missing requirement, or an error under `require`. The lock's `boot.nested_virtualization`
(`auto`) flows into the launch config; the launcher enables `VZGenericPlatformConfiguration.isNestedVirtualizationEnabled`
accordingly, prints a `platform` status line (`"nested_virtualization":"enabled|unsupported|off"`), and under
`require` exits 3 with the same diagnostic. In the guest the probe prints `VESPER-KVM-OK` or `VESPER-KVM-ABSENT`
and `HEALTH` carries `kvm`. CI runs the probe and the selection on GitHub's M-series runner, which lacks nested
virtualization, so the fallback path is exercised on every change.

## Overlay and packages (ADR 0002)

`systemvm/overlay/` is an Alpine `apkovl` tree applied over the tmpfs root at boot. `make systemvm-overlay`
downloads the packages pinned in `docs/specs/systemvm-packages.lock.yaml` (`socat`, `containerd` with `ctr`,
Docker and their closure, 19 files, verified by digest), packs the tree plus the packages into
`build/systemvm/localhost.apkovl.tar.gz` with owner root and mtime 0, appends it to the pinned initramfs as a
second cpio archive at `/vesper/localhost.apkovl.tar.gz` (`build/systemvm/initramfs-vesper`), and re-verifies the
image. The kernel command line names it with `apkovl=`; `etc/local.d/10-vesper-packages.start` installs the
packages and creates `/run/vesper/packages-ready`. The pinned ISO and initramfs stay byte-identical; `verify-image`
checks that the derived file starts with the pinned one and records its digest in the manifest.

To add a package: run `python3 -I scripts/systemvm/resolve-apks.py` with the three APKINDEX files (boot ISO,
`main`, `community`) and the new package list, commit the regenerated packages lock, then `make systemvm-overlay`.

## Commands

0. Everything below in one go: `make systemvm-prepare` (fetch, overlay, launch config, launcher).

1. Fetch and verify the pinned image (first run downloads about 80 MB; later runs reuse `build/systemvm/cache/`):
   `make systemvm-fetch` (wraps `scripts/systemvm/fetch-image.sh [LOCK]`), which runs `systemvmctl lock-env`,
   downloads the ISO with `curl --retry 3`, checks its digest, extracts the two members with `bsdtar` and checks
   their digests, unpacks the kernel, copies the ISO as boot media, and runs `systemvmctl verify-image`, which
   checks the three final digests and the `image_id` and writes `build/systemvm/image-manifest.json`.

2. Build the overlay and derived initramfs: `make systemvm-overlay` (`scripts/systemvm/fetch-packages.sh` then
   `scripts/systemvm/build-overlay.sh`).

3. Generate the NAT-default launch configuration from the lock:
   `make systemvm-launch-config` (runs `systemvmctl launch-config --from-lock --dir build/systemvm`), which writes
   `build/systemvm/launch-config.json` with the kernel, the derived initramfs, boot media, command line and ready
   pattern, and attaches the persistent data disk, creating it on first use
   ([Persistent data disk](#persistent-data-disk-gh-164)).
   Explicit inputs: `cargo run --bin systemvmctl -- launch-config --kernel <path> [--initrd <path>] [--rootfs <path>]
   [--boot-media <path>] [--cmdline <text>] [--ready-pattern <text>] [--data-disk <absolute path>] --console-log run/systemvm/console.log --out
   build/systemvm/launch-config.json`. With no `--cmdline` the line is `console=hvc0`, plus ` root=/dev/vda rw`
   (or `/dev/vdb` behind boot media) when a rootfs is given.

4. Build and sign the launcher (macOS, Xcode command line tools):
   `make vmlauncher-build` (`scripts/macos/build-vmlauncher.sh`), output `build/vmlauncher/vesper-vmlauncher`.

5. Boot repeatedly and record diagnostics:
   `make systemvm-boot-smoke` (`scripts/systemvm/boot-smoke.sh [ITERATIONS] [TIMEOUT] [COMMAND] [ARGS]`, defaults
   5 boots, 60 s each, through the launcher with `--config build/systemvm/launch-config.json`; `MAX_BOOT_MS` in the
   environment overrides the lock's threshold). Each iteration boots the VM to the ready pattern, runs the probe
   and waits for the guest to power off. Outputs: `run/systemvm/console.log` (last boot), `run/systemvm/boot.log`
   (one `result=… duration_ms=… boot_ms=…` line per iteration plus the launcher's `stdout=` JSON lines) and
   `run/systemvm/boot-report.json` (`iterations`, `successful`, `failed`, `failures`, `durations_ms`,
   `max_duration_ms`, `mean_duration_ms`, `boot_ms`, `max_boot_ms`, `max_boot_allowed_ms`). An iteration fails when
   the launcher exits non-zero or `boot_ms` exceeds the threshold. On 2026-11-06 a MacBook Pro (M3, macOS 27.0.1)
   reached the login prompt in about 1.05 s and finished the probe and power-off in about 3.5 s per iteration;
   with the overlay installing 19 packages the login prompt appears at about 2.2 s and the full probe (NAT,
   packages, agent, services) plus power-off takes about 7 s.

6. Re-verify the fetched files or digest explicit ones into a manifest:
   `make systemvm-build` (no arguments re-runs `verify-image`; `scripts/systemvm/build-image.sh KERNEL [INITRD]
   [ROOTFS]` digests explicit files with `systemvmctl build`).

## Make targets

- `make systemvm-prepare` (= `systemvm-fetch`, `systemvm-overlay`, `systemvm-launch-config`, `vmlauncher-build`),
  `make systemvm-boot-smoke`, `make systemvm-build`.

Each wraps a script under `scripts/systemvm/` or `scripts/macos/`. `make` does not forward arguments, so invoke the
script directly to override. The scripts must be run from inside the crate tree; see `scripts/systemvm/README.md`.

## Reproducibility

`image-manifest.json` contains SHA-256 digests for the kernel, the initramfs or rootfs, and the boot media, plus a
stable `image_id` (`sha256("<kernel>:<rootfs or empty>:<initrd or empty>:<boot media or empty>")` over the hex
digests), `schema_version` `"2"` and the raw input paths. Two manifests built from identical bytes at different
paths share `image_id` but are not byte-identical. The `System VM Image` workflow
(`.github/workflows/systemvm-image.yml`) runs `make systemvm-fetch`, `make systemvm-build` and
`make systemvm-launch-config` on Linux (after installing `libarchive-tools` for `bsdtar`), asserts the manifest
`image_id` equals the lock's `expected_image_id`, and uploads both JSON artifacts; its `launcher-build` job builds
and signs the launcher on macOS. GitHub's macOS runners are VMs without nested virtualisation, so boot evidence is
produced on developer hardware and attached to the issue (`run/WOR-27/systemvm/`).

## Bumping the pinned image

1. Read the new release's `latest-releases.yaml` under
   `https://dl-cdn.alpinelinux.org/alpine/<branch>/releases/<arch>/` (flavor `alpine-virt`) and update
   `source.url`, `source.sha256`, `source.size_bytes`, `source.published`, `release`, `branch`, `kernel_version`,
   `boot_media.file` and `boot_media.sha256` (equal to `source.sha256`) in the lock.
2. Extract `boot/vmlinuz-virt` and `boot/initramfs-virt` once (`bsdtar -xf <iso>`), compute their SHA-256 into
   `member_sha256`; run `python3 -I scripts/systemvm/unpack-zboot.py boot/vmlinuz-virt Image-virt` and put the
   result's SHA-256 into `artifacts.kernel.sha256` (`artifacts.initrd.sha256` equals its `member_sha256`).
3. Check `boot/grub/grub.cfg` on the ISO for a changed `modules=` list and update `boot.kernel_cmdline`.
4. Run `make systemvm-fetch`; it fails on `expected_image_id`. Copy the printed `image_id` into the lock and rerun.
5. Check that the probe still matches the guest (root login without password, `udhcpc`, `ip`, `ping`,
   `poweroff` in the live image) and that `max_boot_ms` still holds.
6. Run `cargo test systemvm`, `make systemvm-launch-config`, `make systemvm-boot-smoke`, then record the bump in
   the milestone log.

## Control channel (ADR 0003)

`make systemvm-up` (`scripts/systemvm/up.sh [TIMEOUT]`) starts the launcher detached with `--keep-running`,
writes `run/systemvm/vmlauncher.pid` and `run/systemvm/vmlauncher.log`, and returns once the probe succeeded and
the bridge socket exists (about 3 s). The launcher serves the lock's `control` bridge at
`run/systemvm/vsock/3.sock` (mode 0600): a client writes `CONNECT <port>\n` and is relayed to that guest vsock
port. In the guest, OpenRC supervises `socat -t 86400 VSOCK-LISTEN:1024,fork,max-children=64 EXEC:/usr/local/bin/vesper-agent`
(`systemvm/overlay/README.md` documents protocol version 1).

`cargo run --bin systemvmctl -- control [--config build/systemvm/launch-config.json] <handshake|ping|version|
health|echo --bytes N|keepalive --interval S --seconds S|shutdown|suite>` talks to the agent; `make
systemvm-control-suite` runs the GH #13 checklist (handshake, keepalive under idle, reconnect after a server-side
drop, 2 MiB echo) and writes `run/systemvm/control-report.json`. `make systemvm-down` (`down.sh [WAIT]`) asks the
agent to power off, waits for the launcher to exit, and falls back to SIGTERM. On 2026-11-06 (M3): up in 3.1 s,
suite passed (11 keepalive pings over 20 s, reconnect in 1 attempt, 2 MiB echoed in 11 ms), down in 3.3 s.

## Runtime services (GH #14)

The overlay starts `networking` (`eth0` by DHCP), `containerd` and `dockerd` after the package install from a
detached `vesper-services` unit, under OpenRC's `supervise-daemon`, with cgroup v2 mounted (`cgroups` in sysinit,
`rc_cgroup_mode="unified"`); because the unit is detached, the login prompt (the boot-time measure) waits for
neither DHCP nor the daemons, while the probe and `HEALTH` wait for `/run/vesper/services-ready`. The agent's `HEALTH` reports `containerd_ok` (`ctr version`) and `docker_ok` with the
server version. `make systemvm-services-suite` (`systemvmctl control services-suite`) checks that `vesper-services` loaded `tun` at boot (`/dev/net/tun`, reported as `tun` in `HEALTH`; GH #173), then runs the GH #14 checklist
for each service: healthy at boot, `KILL` (SIGKILL), recovery judged by `HEALTH` within 60 s, and
`/var/log/<svc>.log` plus `supervise-daemon` lines in `/var/log/messages` as evidence; it writes
`run/systemvm/services-report.json`. On 2026-11-06 (M3): both recovered in under 600 ms; the login prompt stays
at about 2.2 s.

## Persistent data disk (GH #164)

The System VM's root is a tmpfs, but its state lives on a data disk: a raw image on macOS that the guest formats
as ext4. What persists across `make systemvm-down` and `make systemvm-up`: `/var/lib/docker` (images, containers,
volumes), `/var/lib/containerd`, and the Vesper state directories `/var/lib/vesper/rootfs`,
`/var/lib/vesper/rootfs-cache` (the microVM root-drive cache), `/var/lib/vesper/workloads` and
`/var/lib/vesper/state`. What does not: the packages, which are still reinstalled into the tmpfs root on every
boot, and anything else outside those directories, such as task bundles under `/run/containerd` and the jails
under `/srv/jailer` and `/srv/vesperd` (TD-025).

- Creation: `make systemvm-launch-config` (`scripts/systemvm/generate-launch-config.sh`, from-lock mode) uses
  `VESPER_DATA_DISK`, default `run/systemvm/data.img`; a relative path is taken from the repository root. A
  missing image is created sparse with `VESPER_DATA_DISK_GIB` GiB (default 64) and mode 0600; an existing image is
  never changed. The script passes `--data-disk <absolute path>` to `systemvmctl launch-config`, which sets
  `data_disk_path` in the launch config and appends `vesper_data=/dev/vdX` to the kernel command line: the next
  virtio block device after the boot media and the rootfs, `/dev/vdb` with the pinned image. The launcher attaches
  the image writable, with cached access and full synchronisation, and exits 3 when the file is missing.
- First boot and later boots: `vesper-services` runs `vesper-data mount` before it starts containerd and dockerd.
  An empty disk is formatted ext4 with label `vesper-data` (lazy init, about a second); a disk labelled
  `vesper-data` is mounted without formatting. The disk is mounted at `/var/lib/vesper-data` and each persistent
  directory is bind-mounted onto it. The console shows `vesper: DATA formatted /dev/vdb` or
  `vesper: DATA mounted /dev/vdb`. When the services stop, `vesper-data stop` syncs and unmounts the disk.
- Refusal: a disk with any other filesystem or label is never formatted. The console shows
  `vesper: DATA refused <dev> <reason>` (for example `foreign:ext4:other-label`) and the System VM keeps its state in
  tmpfs, as before GH #164; Docker still works but nothing survives the next reboot.
- Status: `cargo run --bin systemvmctl -- control data status` prints the outcome of the last mount
  (`DATA formatted|mounted <dev>`, `DATA absent` or `DATA refused <dev> <reason>`, kept in `/run/vesper/data-state`)
  and, while the disk is mounted, `used=` and `avail=` in KiB.
- Reset: stop the System VM (`make systemvm-down`), delete the image
  (`rm run/systemvm/data.img`), then `make systemvm-launch-config` and `make systemvm-up`. This deletes every image,
  container, volume and cached root drive; the next boot formats a new disk.
- Turn it off: `VESPER_DATA_DISK=none make systemvm-launch-config` writes a config without a data disk; the guest
  prints `vesper: DATA absent` and keeps everything in tmpfs (the behaviour before GH #164). The tmpfs has about
  600 MiB free, too little for large images such as the upstream Firecracker test container.
- Space: the image is sparse, so it uses only what the guest has written (about 18 MB on macOS after formatting a
  64 GiB image). It is not trimmed when the guest deletes data, and the root-drive cache is never pruned (TD-035),
  so the file grows over time. A larger size applies only to a new image.

`make data-disk-suite` (`scripts/systemvm/data-disk-suite.sh`, 9 steps; needs `make docker-up`) restarts the System
VM itself four times and leaves it running on the default disk. It checks that the disk is in use and holds the
state directories, that the second boot mounts without formatting, that an image, a stopped container and a named
volume survive a reboot, that the launcher is ready within 5000 ms with the disk mounted, that a disk relabelled
by the suite is refused and keeps its label, and that Docker works with the disk refused. It writes
`run/systemvm/data-disk-report.json`. On 2026-11-11 (M3 Pro): 9/9, ready in 2.7 s with the disk mounted.

With the disk the upstream Firecracker test container (`public.ecr.aws/firecracker/fcuvm`, 1.31 GiB compressed for
arm64) has room to be pulled, which the CE-0001 feasibility run needs (decision D-017).

## Docker ingress (GH #15)

`make docker-up` (`scripts/systemvm/docker-up.sh`) starts `vesper-proxy --listen run/vesper/docker.sock --upstream
vsock://3:2375 --vsock-socket-dir run/systemvm/vsock --validate-upstream` detached (pid and log under
`run/vesper/`), after checking that the bridge socket exists. In the guest, `vesper-docker-relay` (supervised `socat -t 86400 VSOCK-LISTEN:2375,fork UNIX-CONNECT:/var/run/docker.sock`) forwards that port to the Docker daemon. The long `-t` keeps relaying after one side half-closes; socat's default ends the connection 0.5 s later, which cut attached output (GH #167). The port relay, the agent and the share sink use the same setting, so
`DOCKER_HOST=unix://$PWD/run/vesper/docker.sock docker version` reaches Docker 28 inside the VM. The listen socket
is created with mode 0600 (`--listen-mode`). `make systemvm-trivial-image` builds `build/systemvm/trivial-rootfs.tar`
(busybox and musl from the pinned ISO, deterministic) and `make docker-smoke` runs the GH #15 checklist: socket
exists and is owner-only, `docker version`, `docker info`, `docker import` of that tarball, `docker run … echo`,
no other host sockets under `run/`, and a refused `CONNECT` to a guest port without a listener; it writes
`run/systemvm/docker-smoke-report.json`. `make docker-down` sends SIGTERM to the proxy and removes the socket.
On 2026-11-06 (M3): 7/7, the container printed `vesper-container-ok`.

The packaged canonical path from the issue, `/var/run/vesper/docker.sock`, is Intended: it needs root and no
packaging exists yet.

### Published ports (GH #28, ADR 0005)

`make docker-up` starts `vesper-proxy` with `--publish-ports`: the publisher follows container events on the
Docker API, reconciles running containers at start, listens on `127.0.0.1:<host port>` for every TCP `-p`
binding and forwards each connection through the bridge (`CONNECT 2400`, then `PORT <n>`) to the guest's
`vesper-port-relay`, a supervised `socat VSOCK-LISTEN:2400,fork EXEC:vesper-port-relay-conn` that connects to
the guest loopback port dockerd published. `run/vesper/ports.json` lists the path, the gateway and every
mapping (`listening`, `conflict` when the macOS port is taken, `unsupported` for UDP or a non-loopback host
ip) and the proxy log names binds and releases. `make ports-suite` runs the GH #28 checklist (11 steps) into
`run/systemvm/ports-report.json`. The launcher ignores SIGPIPE since this slice: a proxy dying with a live
bridge stream used to take the VM down.

### Bind mounts over virtio-fs (GH #29)

`VESPER_SHARES="/abs/path /other:ro /third:nowatch" make systemvm-launch-config` (or `systemvmctl launch-config --share`) adds
`shared_directories` to the launch config; the launcher attaches one virtio-fs device per entry (tags
`share0`, `share1`, …) and `up.sh` asks the guest (`vesper-share mount TAG PATH ro|rw` through the agent) to mount
each tag at the same absolute path, so `docker run -v /abs/path:/x` from macOS binds the host directory. No
share is configured by default. Semantics, recorded by `make binds-suite` (20 steps, `run/systemvm/binds-report.json`; developer hardware only, never in CI):
host files appear owned by uid 0 in the guest, guest writes appear on the host owned by the sharing user, `chmod`
round-trips, `chown` is ignored, read-only is enforced by the share flag and by `:ro`, host edits are visible on
the next access, inotify fires for guest writes and, on watched read-write shares, for host edits (below), and a
`-v` path outside the configured shares is a guest-local directory. `make systemvm-up-shares` boots with the three suite shares (read-write watched, read-only, read-write `:nowatch`).

Host edits and inotify (GH #150). virtio-fs tells the guest kernel nothing about host-side edits, so the launcher
watches every read-write share with FSEvents (`"watch": true` in the launch config; `PATH:nowatch` opts a share
out) and writes each changed path to the guest's sink, `vesper-share notify` on guest vsock port 2401. The sink
replays each path in three steps: an atime-only set (`touch -c -a -r P P`), whose reply refreshes the guest's
cached attributes; an mtime-only set (`touch -c -m -r P P`), which makes the guest kernel emit `IN_MODIFY` for
containers watching the bind mount; and, only if the file's size changed between the two, `touch -c -m P`
(reason `raced`). The replay never changes contents. It leaves the host mtime equal to the last host write,
except that a host write landing between the two sets gets the time of the replay (never older than the write),
and a same-size overwrite inside that window (about a millisecond) is not detected and can keep the previous
write's mtime. The replay sets the host atime to the guest's cached atime, which can move it backwards, and
changes the ctime ([SECURITY.md](../SECURITY.md)).

The watcher never waits for the guest. Writes to the sink are non-blocking, in batches of 1,000 lines; when the
sink is not reading, the watcher tries again every 50 ms. It keeps up to 100,000 unsent lines; beyond that it
drops the oldest lines not yet on the wire and forgets them, so a later edit of the same path is sent again. A
failed connect or write is retried every 2 s. When FSEvents reports dropped events, the watcher walks the
reported directory (at most 50,000 entries) and sends what was modified in the last 60 s. Files stay current on
access throughout, and SIGTERM and guest shutdown do not wait for the sink.

Launcher output for the watcher:

- Status line `"status":"share-watch"` (`"watched":N,"port":2401`) once the FSEvents stream runs, or
  `"status":"share-watch-failed"` when the stream cannot start; the VM runs either way.
- stderr `share-watch: connected to guest port 2401 (connection N)` each time the sink answers,
  `share-watch: write to guest port 2401 failed (…); reconnecting` when it goes away, and
  `share-watch: FSEvents dropped events under <path>; rescan found <n> recently changed path(s)` for each rescan.
- Status line `"status":"share-watch-summary"` before `terminating` or `stopped`, with the counters
  `forwarded= sent= echoes= ignored= dropped= connects= stalls= rescans= queued=`
  ([RELIABILITY.md](../RELIABILITY.md#share-watcher-gh-150) explains each).

What a container sees differs from a native Linux file system. TD-038 holds the complete list; the main points:
a host create is `IN_MODIFY`, not `IN_CREATE`; a host delete or rename is `IN_MODIFY` on the parent
directory; there is no `IN_CLOSE_WRITE` and no `IN_ATTRIB` for a host `chmod`; deletes, renames away and
directory deletes directly in a share's root directory, and all edits on read-only shares, produce nothing; a
container's own write to a watched share gives one extra `IN_MODIFY`; a watch on a single file does not follow
a host atomic save; and a watcher that adds watches only on `IN_CREATE` (`inotifywait -r`) does
not follow a directory created on the host after it started. Watchers that rescan a directory when it changes
(chokidar) see `change`, `add` and `unlink`; Go `fsnotify` reports `WRITE` for the file on create, append and
save and `WRITE` for the directory on rename and delete, never `CREATE`, `REMOVE` or `RENAME`. Bind mounts, and
so these events, do not reach tasks that the containerd shim runs in microVMs.

Checks without a VM: `make share-notify-selftest` (the sink's path policy, 20 checks, Linux CI) and
`make share-watch-selftest` (the watcher's decisions, delivery, reconnect, the queue limit and exit under a
stalled sink, 20 checks, macOS CI). `vesper-vmlauncher --watch-dry-run DIR` prints the paths the watcher would
send; with `--watch-sink-socket PATH` it sends them to a Unix stream socket as it would to the guest (self-test
only), and `VESPER_SHARE_WATCH_PENDING_LIMIT` lowers the queue limit (minimum 2,000). In both modes SIGTERM
prints `SUMMARY <counters>`. `systemvmctl control share touch PATH` replays one path by hand.

### DNS and network defaults (GH #30)

Containers on the default bridge inherit the System VM's resolver, which DHCP sets to the NAT gateway
(`nameserver 192.168.64.1`); user-defined and compose networks use dockerd's embedded DNS (`127.0.0.11`), which
answers service names and aliases and forwards everything else to that resolver. The System VM's network mode is
`nat` from the image lock and dockerd runs with its default bridge and address pools, so the defaults are the
same on every boot. Caveats: the host resolver may return IPv6 addresses although containers have no IPv6
route (clients fall back to IPv4); `host.docker.internal` is not provided (TD-039). `make dns-suite`
(`scripts/systemvm/dns-suite.sh` with `fixtures/dns-stack.yml`) runs the GH #30 checklist (12 steps) from macOS
into `run/systemvm/dns-report.json`.

### Cross-architecture containers (GH #39)

The packages lock pins `qemu-x86_64`; `vesper-binfmt` (OpenRC, default runlevel) mounts `binfmt_misc` and
registers it for x86-64 ELF with flags `OCF` (fix-binary: the interpreter is opened at registration, so
containers need no qemu in their rootfs). `docker run --platform linux/amd64 …` from macOS then runs emulated;
`vesper-arch status` lists the native arch and the registered interpreters, `vesper-arch mode linux/<arch>`
answers `native`, `emulated:qemu-x86_64` or `unsupported (…; supported: aarch64 native, x86_64 emulated)`
(`systemvmctl control arch …` from the host). Caveat: `docker run --platform` on an existing tag re-tags that
reference with the foreign variant; use a separate reference for foreign platforms. `make crossarch-suite`
runs the GH #39 checklist (9 steps, incl. a native-vs-emulated timing of a shell loop) into
`run/systemvm/crossarch-report.json`; the workflow suite's amd64 step is required since this landed.

### Developer workflow suite (GH #31)

`make workflow-suite` (`scripts/systemvm/workflow-suite.sh`) runs the everyday Docker workflow from macOS through
the ingress: the compose stack from `fixtures/dns-stack.yml`, `docker build` of `fixtures/wf-build/` (BuildKit,
RUN and COPY layers) and a run of the result, `docker logs -f` streaming, an interactive `docker exec -it`
driven by `expect` through a pty, HUP/TERM/KILL signal delivery, and a `--platform linux/amd64` probe that
records the cross-architecture gap until GH #39 (`WF_REQUIRE_AMD64=1` makes it required). The report
`run/systemvm/workflow-report.json` embeds the `baselinectl` metadata so a regression is tied to the baseline
revisions. The suite first makes sure the test image is the native arm64 variant, because an amd64 pull
re-tags the same reference.

### Docker API conformance (GH #27)

`make docker-conformance` (`scripts/systemvm/docker-conformance.sh`) runs 22 checks from macOS through the
ingress: protocol (`/_ping`, `/version`), a named Docker context, images (import, list, inspect), `run` with
exit code and separated streams, `ps`, `exec`, `stop` (SIGTERM reaches the process), `kill`, `rm`, `pull` from
Docker Hub through the System VM's NAT, dockerd's own errors for `login` to an unknown registry, `checkpoint`
and an unknown runtime passing through intact, and the boundary (socket mode 600, no TCP listener, no other
host sockets, guest ports without a listener refused). The proxy stays a byte-level forwarder; Vesper-specific
unsupported behaviours (published ports from macOS, bind mounts) are GH #28 and #29 (TD-036).

## Reliability suite (GH #16)

`make systemvm-reliability` (`scripts/systemvm/reliability-suite.sh [CYCLES]`, default 50, `CYCLES=200 make
systemvm-reliability`) stops anything running, snapshots host resources (launcher and proxy processes, sockets and
pid files under `run/`), runs `CYCLES` probed boots through `boot-smoke.sh` under the lock's threshold, then brings
the VM up once and runs the control suite, the services suite, `docker-up`, the Docker smoke and the tear-down,
snapshots again, and assembles `run/systemvm/reliability-report.json` with
`scripts/systemvm/reliability_report.py`. The report records per-section status, boot statistics with a drift check
(last five cycles may not exceed the first five by more than 1.5×), the leak snapshots, and `failures[]` entries of
`{step, reason, evidence}`; `passed` is true only when every section is ok and nothing leaked. The transcript is in
`run/systemvm/reliability.log`. The assembler's `--self-test` runs in CI; the suite itself needs Apple silicon.

## Backend A: Firecracker microVMs inside the System VM (GH #18)

The packages lock's `archives` section pins the Firecracker v1.16.0 aarch64 release (its tag commit `d83d72b7…` is
the baseline revision of `docs/specs/design-v0.baselines.lock.yaml`, GH #162); `make systemvm-overlay` places
`firecracker` and `jailer` under `/var/lib/vesper/firecracker/` in the guest. `vesper-microvm` (overlay,
`usr/local/bin`) manages jailed microVMs: `prepare` unpacks the ISO's zboot kernel into a raw Image and builds a
busybox initramfs whose init prints `VESPER-MICROVM-OK` (and powers off when the command line has `vesper_exit=1`);
`launch ID [--exit]` copies kernel and initramfs into `/srv/jailer/firecracker/ID/root`, writes a single-file
Firecracker config (1 vCPU, 256 MiB, no drives, `console=ttyS0 reboot=k panic=1 pci=off`) and starts
`jailer --id ID --exec-file … --uid 10000 --gid 10000 --chroot-base-dir /srv/jailer --cgroup-version 2 --cgroup
pids.max=256 --cgroup memory.max=671088640 -- --config-file /vm.json --api-sock /run/firecracker.socket` (the
640 MiB cap leaves room for a full snapshot's memory file in the tmpfs jail, which is charged to the same cgroup), waiting
for the marker; `isolation ID` reports uid, jail contents, mount namespace, cgroup path and seccomp mode from
`/proc/<pid>`; the manager finds its Firecracker with `pgrep -u 10000 -f "firecracker --id ID "`, so it never acts
on a `vesperd` microVM with the same id (uid 20000, ADR 0007); `stop`, `crash` (SIGKILL), `cleanup` (kill, remove jail and cgroup, report `CLEAN` or `DIRTY <kind>`),
`status`, `log`. From the host, `cargo run --bin systemvmctl -- control microvm <args>` runs any of these and
`make systemvm-microvm-suite` runs the GH #18 checklist (13 steps) and writes `run/systemvm/microvm-report.json`.
On 2026-11-06 (M3 Pro): launch to marker in about 1.5 s, all isolation checks true, cleanup clean after stop,
SIGKILL and guest power-off.

## OCI image to ext4 rootfs (GH #19)

`vesper-rootfs` (overlay, `usr/local/bin`) converts an image from the guest's Docker daemon into a reproducible ext4
rootfs: `build IMAGE [--no-cache]` runs `docker create` and `docker export`, strips Docker's per-container files,
embeds `/etc/vesper-provenance.json` (source image id, repo tags, layer diff ids, created, architecture, os; the
Firecracker and containerd revisions from `docs/specs/design-v0.baselines.lock.yaml`, which `make systemvm-overlay`
ships to `/var/lib/vesper/baselines.lock.yaml`; mke2fs and Docker versions), forces every mtime to epoch 0 and runs
`E2FSPROGS_FAKE_TIME=0 mke2fs -t ext4 -d` with a UUID and hash seed derived from the image id, so one image id always
yields the same bytes. Output: `/var/lib/vesper/rootfs/<id16>.ext4` with a sidecar `.provenance.json` carrying
`output_sha256` (read back with `debugfs`); a second `build` is a cache hit verified by digest. `provenance PATH`
prints the document, `inspect PATH` lists the files of a loop-mounted image, `import-trivial` makes a busybox image
from the guest itself. `vesper-microvm launch ID --rootfs PATH` attaches the image as `/dev/vda`; the microVM
initramfs (now carrying virtio-mmio, virtio-blk, crc32c and ext4 modules from the modloop) mounts it, prints
`VESPER-ROOTFS-OK <image id>` from the embedded provenance, `switch_root`s into `/sbin/init` when present or
executes a command from the rootfs. `make systemvm-rootfs-suite` runs the GH #19 checklist (11 steps) and writes
`run/systemvm/rootfs-report.json`. Reproducibility is keyed by the image id (TD-031).

## MicroVM per container (GH #20)

`vesper-workload` (overlay, `usr/local/bin`) maps one workload to one jailed microVM. `run NAME IMAGE -- CMD…`
builds (or reuses) the image's rootfs with `vesper-rootfs`, copies it to `/var/lib/vesper/workloads/NAME.ext4`,
and launches `vesper-microvm launch wl-NAME --rootfs … --cmd <base64 CMD>`; the microVM init mounts the rootfs,
chroots into it, prints `VESPER-WORKLOAD-START`, the command's output and `VESPER-WORKLOAD-EXIT <code>`, then
powers off. `ps` lists `NAME STATE pid=N image=ID` (RUNNING from the microVM manager, EXITED from the console
log's exit marker, STOPPED otherwise), `logs NAME` returns the output between the markers, `wait NAME [TIMEOUT]`
blocks until the exit marker and tears the microVM down (`EXITED NAME code=N`), `stop NAME` sends SIGTERM and
cleans the jail and cgroup (`STOPPED NAME teardown=CLEAN`), `rm NAME` removes the jail, cgroup, rootfs copy,
record and log (`REMOVED NAME` or `ERR leftovers`), `isolation NAME` returns the microVM manager's isolation
JSON. `make systemvm-workload-suite` runs the GH #20 checklist (16 steps: three concurrent microVMs with distinct
pids and cgroups, a file written in one rootfs invisible to another, stop of one leaving the others running,
exit-code and output mapping, full removal) and writes `run/systemvm/workload-report.json`. Limits: TD-032.

## Parity and performance baseline (GH #21)

`vesper-fcparity` (overlay, `usr/local/bin`) talks to a jailed microVM's Firecracker API socket with `socat`
(`api ID METHOD PATH [BODY|-] [TIMEOUT]`; the request side is a FIFO kept open until the response is complete,
because Firecracker resets a half-closed connection). `conformance ID` launches a microVM and runs 19 checks
(version equals the binary's, instance state and id, machine and VM config, deterministic 400 faults, pause, full
snapshot while paused, resume, SIGTERM stop, clean jail), printing `CHECK <name> ok|fail <detail>` lines and
`CONFORMANCE passed=N failed=M firecracker_version=V`. `perf N` runs N launch/exit/cleanup cycles and prints
`PERF n= boot_p50_ms= boot_p95_ms= boot_max_ms= cycle_p50_ms= create_rate_per_min= wall_ms= failures=`. From the
host, `systemvmctl control parity-suite --perf-cycles N` turns these into a suite report (one step per check,
`perf_run_1`, `perf_run_2`, `perf_comparable`: no failures and boot p50 within 20 %). `make systemvm-parity-baseline`
(`scripts/systemvm/parity-baseline.sh [PERF_CYCLES] [--publish]`) brings the VM up if needed, runs the suite,
`baselinectl --mode generate` and `compatexctl`, and assembles `run/systemvm/parity-baseline-report.json` with
`parity_baseline_report.py`: the exact lock revisions, the Firecracker release, archive digest and API version, the
parity metadata, the compat exception summary (open ids and entries), the conformance checks, both performance
runs and `passed`. `--publish` copies a passing report to `docs/specs/backend-a-parity-baseline.json`; the `Parity
Baseline` workflow re-validates that capture against the lock, `systemvm/archives.yaml` and the registry
(`make parity-baseline-check`) and uploads it as a CI artifact, since runners cannot boot the System VM (TD-005).
The gaps to the design bar are compat exceptions CE-0001 (upstream pytest suite not run), CE-0002 (release
binary vs pinned commit, TD-029) and CE-0003 (no Linux baseline on the same host class).

## containerd shim v2 (GH #22, ADR 0004)

`containerd-shim-vesper-v2` (Go module `shim/`) is built static for Linux arm64 and installed by the overlay.
It registers `io.containerd.vesper.v2`. `Create` validates the OCI bundle and prepares a private ext4 drive.
`Start` boots a jailed microVM with `--guest-exec`, sends a generated OCI config over vsock and connects
independent stdin, stdout and stderr streams. runc executes argv inside the mounted rootfs.
The console remains available for boot and runtime diagnostics.

The shim supports init and exec lifecycle, terminal resize, stdin EOF, signals and exit status (ADR 0006).
`make systemvm-shim-suite` checks the original 21 lifecycle cases; `make systemvm-exec-suite` adds 19 exec cases.
See [shim/README.md](../../shim/README.md) for interfaces, protocol and limits.

## Block-first rootfs preparation (GH #24)

The shim validates every bundle before mounting (`shim/internal/bundle`: config, process, cwd, root, mounts;
errors carry a reason and a hint) and prepares the root drive block-first: the overlay mount's lower-layer
chain is the cache key, `/var/lib/vesper/rootfs-cache/<key>.ext4` is built once per chain with a `.sha256`
sidecar and verified on every reuse, and each task gets a private copy as `<bundle>/rootfs.ext4`. A bundle whose
rootfs is a plain directory (no snapshot mounts) is the documented fallback: built per task, never cached. The
record `<bundle>/vesper-rootfs.json` (`strategy`, `cached`, `key`, `lower`, `sha256`, `bytes`, `build_ms`,
`fallback`) is what `vesper-ctr meta NAME` prints; `vesper-ctr cache` lists the cache. `make systemvm-bundle-suite`
runs the GH #24 checklist (15 steps: a two-layer image built from the trivial one boots and reads its second
layer, two tasks from the chain share key and digest with `cached=true`, one drive per chain, the layered
chain reuses the base snapshot) into `run/systemvm/bundle-report.json`. Limits: TD-035.

## Shim restart resilience (GH #25)

containerd 2.1 reconnects to a live shim through the bootstrap record it keeps in the bundle, so a task survives
a containerd restart with its microVM untouched. For the shim's own death the shim writes
`<bundle>/vesper-task.json` on every transition (created, running, stopped, deleted, with the VMM pid and exit),
and containerd's `delete` of the dead shim reconciles from it: real exit status for a finished task, 137 for a
running one, and `vesper-microvm cleanup` in every case. `vesper-ctr shim-pid|kill-shim|wait-gone|state-file|
inflight-start|inflight-result` drive the scenarios and `make systemvm-restart-suite` runs the GH #25 checklist
(18 steps: SIGKILL containerd with a running task and find it RUNNING with the same pid after respawn, kill and
delete it; SIGKILL a shim and see the task gone, the microVM EXITED and the jail CLEAN; an in-flight
`ctr run --rm` across a restart fails with EOF and leaves nothing) into `run/systemvm/restart-report.json`.

## ctr conformance (GH #26)

`make ctr-conformance` runs the shim, bundle, restart and exec suites on a booted System VM and assembles
`run/systemvm/ctr-conformance-report.json` (`scripts/systemvm/ctr_conformance_report.py`: baseline lock revisions,
`baselinectl` metadata, every step per suite, the GH #26 checklist mapping and the open gaps). `CTR_FLAGS=--publish`
copies a passing report to `docs/specs/ctr-conformance-baseline.json`, which the `ctr Conformance` workflow
validates against the lock (`make ctr-conformance-check`) and uploads. The shim's `Stats` reads the microVM's
cgroup v2 files so `ctr task metrics` answers. Operations are in [shim-operations.md](shim-operations.md).

## MicroVM vsock transport (GH #23, in progress)

`vesper-microvm launch ID --vsock` attaches a Firecracker vsock device (guest CID 3) whose host side is the
Unix socket `/srv/jailer/firecracker/ID/root/v.sock`; a connection sends `CONNECT <port>\n` and gets `OK <port>`,
the same preface as the launcher bridge. The initramfs carries the guest transport modules and the init loads
them (`VESPER-VSOCK-OK`); `status ID` shows `vsock=present`. `vesper-microvm --guest-exec` now uses this transport for runc sessions (ADR 0006).
The shim sends OCI configs separately from the ext4 drive and connects three stream ports.
`make systemvm-exec-suite` checks stdin, separated output, terminal resize, exec and signals.
The [shim contract](../../shim/README.md) describes lifecycle and protocol details.

## `vesperd` microVMs (ADR 0007, slice 3)

`vesperd`'s backend in the System VM is `vesper-vesperd` (overlay, `usr/local/bin`), reached through three agent
verbs. `LAUNCHAPI <id> [ceiling]` runs `vesper-microvm prepare`, copies the kernel and initramfs into
`/srv/vesperd/firecracker/<id>/root` as `/Image` and `/initrd.gz`, and starts `jailer --id <id> --uid 20000 --gid
20000 --chroot-base-dir /srv/vesperd --parent-cgroup vesperd --cgroup-version 2 --cgroup pids.max=256 --cgroup
memory.max=<ceiling> -- --api-sock /run/firecracker.socket`, with no configuration file, so the microVM is built from
API requests. It replies `READY <id> pid=<n> firecracker_version=<v>` once the API socket exists, or `ERR <reason>`
and closes. The connection is then the lease: when the client closes it, the agent stops Firecracker (SIGTERM, 3 s,
SIGKILL) and removes the jail, the cgroup `/sys/fs/cgroup/vesperd/<id>` and the record
`/run/vesper/vesperd/leases/<id>`; when Firecracker exits, the agent does the same and closes the connection.
`FCAPI <id>` replies `OK` and relays bytes to the API socket. `VESPERD <args…>` runs the helper's inspection
commands (`status`, `isolation`, `list`, `log`, `reap`) and test hooks with the buffered reply of the other guest
tools. The `vesper-vesperd-reaper` service runs a reaper pass every 10 s. Values and their reasons are in the
[ExecPlan](../exec-plans/active/2026-11-12-GH156-vesperd-backend-a.md#values).

`make systemvm-launchapi-suite` (`systemvmctl control launchapi-suite`) boots a microVM through the API (`PUT
/machine-config`, `PUT /boot-source`, `PUT /actions` `InstanceStart`) and checks `GET /` before and after, the
three teardown paths, the reaper and the id checks, 20 steps, into `run/systemvm/launchapi-report.json`. On
2026-11-12 (M3 Pro): 20/20; `READY` 0.2 s after the jailer started (1.4 s on the first launch after boot), teardown
about 0.5 s after the lease closed, a SIGKILLed lease shell's microVM reaped within one interval.

`vesperd` itself does not use this yet (slice 4). The jails are in tmpfs until slice 5 moves them onto the data disk.

## Proxy bridge convention

Phase 0 keeps the proxy-side vsock bridge inspectable by resolving
`vsock://<cid>:<port>` upstreams through a host Unix socket at
`<vsock_socket_dir>/<cid>.sock` and sending a Firecracker-style
`CONNECT <port>\n` preface before relaying traffic. Since 2026-11-06 `vesper-vmlauncher` serves that socket
(see "Control channel"), so `vesper-proxy --vsock-socket-dir run/systemvm/vsock --upstream vsock://3:<port>`
reaches a guest port while the VM is up.

Configuration lives on `vesper-proxy`: `--upstream` (default `vsock://3:2375`), `--vsock-socket-dir` (default
`/tmp/vesper/vsock`) and `--dial-timeout-seconds` (default 5); the preface is written after the dial succeeds within
that timeout. `make dev-up` does not use the bridge: it points the proxy at a Unix upstream.

## runc sessions inside microVMs (GH #23, ADR 0006)

The current shim replaces the kernel-command-line and console-log execution path with private runc sessions.
Legacy `vesper-workload --cmd` probes still use their original console path.
The new shim supports terminal tasks, stdin EOF, separated stdout/stderr, exec, signal delivery and resize.
`vesper-microvm launch ID --rootfs PATH --runc-smoke` provides the prerequisite proof.
`make systemvm-exec-suite` writes `run/systemvm/exec-report.json`.
Pause, snapshot and checkpoint compatibility with runc remain unresolved and unsupported by the shim.
