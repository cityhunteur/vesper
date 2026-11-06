# systemvm/overlay

## Overview

The Alpine `apkovl` overlay applied to the System VM at boot. `scripts/systemvm/build-initramfs.py`
packs this tree (plus the pinned extra packages under `var/lib/vesper/apks/`) into
`localhost.apkovl.tar.gz`, appends it to the pinned initramfs as a second cpio archive at
`/vesper/localhost.apkovl.tar.gz`, and the kernel command line (`apkovl=/vesper/localhost.apkovl.tar.gz`
in `docs/specs/systemvm-image.lock.yaml`) makes the Alpine init unpack it over the tmpfs root before
OpenRC starts. See ADR 0002.

## Interface

- `etc/.default_boot_services`: empty marker; keeps the init's default boot services (devfs, mdev, modloop, networking, …) enabled although an overlay is present.
- `etc/hostname`: `vesper`.
- `etc/runlevels/default/local`: symlink to `/etc/init.d/local`, so OpenRC runs `etc/local.d/*.start` in the default runlevel.
- `etc/local.d/10-vesper-packages.start`: runs `apk add --quiet --no-progress --force-non-repository /var/lib/vesper/apks/*.apk` (the files are not in a repository, so apk needs the flag; they are verified against the Alpine keys on the boot media); on success creates `/run/vesper/packages-ready`; logs `vesper: …` lines to `/dev/console`; exits 1 on failure.

## Invariants

- Every file in this tree lands at the same path under `/` in the guest; the builder sets owner root, mode from the repository (executable bit preserved) and mtime 0, so the archive is deterministic.
- No secrets, no host paths, no package binaries live here: packages come from the cache that `scripts/systemvm/fetch-packages.sh` verifies against the packages lock.
- Scripts must be POSIX `sh` (busybox) and must not depend on packages installed by `10-vesper-packages.start` unless they run after it (OpenRC runs `local.d` scripts in name order).

## Side Effects

- At guest boot: packages are installed into the tmpfs root; `/run/vesper/packages-ready` is created; console lines prefixed `vesper:` are printed.

## Verification

- `make systemvm-overlay` builds the archive; `make systemvm-boot-smoke` boots with it and the probe in the image lock requires `/run/vesper/packages-ready` and the installed binaries.
