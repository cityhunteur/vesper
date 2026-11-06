---
status: verified
owner: cityhunteur
last_reviewed: 2026-11-06
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
  brings `eth0` up and runs `udhcpc` against the Virtualization.framework NAT, pings the default gateway
  (`VESPER-NAT-OK`), waits for `/run/vesper/packages-ready` and checks that `socat`, `containerd` and `dockerd`
  are installed (`VESPER-EXTRAS-OK`), prints `VESPER-PROBE-OK` and runs `poweroff`, so every boot proves NAT and
  the overlay from inside the guest and ends with a clean shutdown. Markers are split in the typed commands
  because the console echoes input.

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
   pattern.
   Explicit inputs: `cargo run --bin systemvmctl -- launch-config --kernel <path> [--initrd <path>] [--rootfs <path>]
   [--boot-media <path>] [--cmdline <text>] [--ready-pattern <text>] --console-log run/systemvm/console.log --out
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
   with the overlay installing 18 packages the login prompt appears at about 2.0 s.

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
port. In the guest, OpenRC supervises `socat VSOCK-LISTEN:1024,fork EXEC:/usr/local/bin/vesper-agent`
(`systemvm/overlay/README.md` documents protocol version 1).

`cargo run --bin systemvmctl -- control [--config build/systemvm/launch-config.json] <handshake|ping|version|
health|echo --bytes N|keepalive --interval S --seconds S|shutdown|suite>` talks to the agent; `make
systemvm-control-suite` runs the GH #13 checklist (handshake, keepalive under idle, reconnect after a server-side
drop, 2 MiB echo) and writes `run/systemvm/control-report.json`. `make systemvm-down` (`down.sh [WAIT]`) asks the
agent to power off, waits for the launcher to exit, and falls back to SIGTERM. On 2026-11-06 (M3): up in 3.1 s,
suite passed (11 keepalive pings over 20 s, reconnect in 1 attempt, 2 MiB echoed in 11 ms), down in 3.3 s.

## Runtime services (GH #14)

The overlay starts `containerd` and `dockerd` at boot (`vesper-services`, after the package install) under OpenRC's
`supervise-daemon`, with cgroup v2 mounted (`cgroups` in sysinit, `rc_cgroup_mode="unified"`) and `eth0` configured
by the `networking` service. The agent's `HEALTH` reports `containerd_ok` (`ctr version`) and `docker_ok` with the
server version. `make systemvm-services-suite` (`systemvmctl control services-suite`) runs the GH #14 checklist
for each service: healthy at boot, `KILL` (SIGKILL), recovery judged by `HEALTH` within 60 s, and
`/var/log/<svc>.log` plus `supervise-daemon` lines in `/var/log/messages` as evidence; it writes
`run/systemvm/services-report.json`. On 2026-11-06 (M3): both recovered in under 600 ms; the login prompt now
appears at 3.4 to 4.5 s because the daemons start before it.

## Docker ingress (GH #15)

`make docker-up` (`scripts/systemvm/docker-up.sh`) starts `vesper-proxy --listen run/vesper/docker.sock --upstream
vsock://3:2375 --vsock-socket-dir run/systemvm/vsock --validate-upstream` detached (pid and log under
`run/vesper/`), after checking that the bridge socket exists. In the guest, `vesper-docker-relay` (supervised
`socat VSOCK-LISTEN:2375,fork UNIX-CONNECT:/var/run/docker.sock`) forwards that port to the Docker daemon, so
`DOCKER_HOST=unix://$PWD/run/vesper/docker.sock docker version` reaches Docker 28 inside the VM. The listen socket
is created with mode 0600 (`--listen-mode`). `make systemvm-trivial-image` builds `build/systemvm/trivial-rootfs.tar`
(busybox and musl from the pinned ISO, deterministic) and `make docker-smoke` runs the GH #15 checklist: socket
exists and is owner-only, `docker version`, `docker info`, `docker import` of that tarball, `docker run … echo`,
no other host sockets under `run/`, and a refused `CONNECT` to a guest port without a listener; it writes
`run/systemvm/docker-smoke-report.json`. `make docker-down` sends SIGTERM to the proxy and removes the socket.
On 2026-11-06 (M3): 7/7, the container printed `vesper-container-ok`.

The packaged canonical path from the issue, `/var/run/vesper/docker.sock`, is Intended: it needs root and no
packaging exists yet.

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
