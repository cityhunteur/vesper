---
status: accepted
owner: cityhunteur
last_reviewed: 2026-11-06
---

# 0002. Customise the System VM with an Alpine overlay delivered through the initramfs

Date: 2026-11-06. Supersedes: none. Superseded by: none.

## Context and problem

Phase 1 needs guest software that the pinned Alpine virt ISO does not ship: `socat` for the vsock control
channel (GH #13), `containerd` and `dockerd` with their OpenRC services (GH #14), plus boot-time configuration.
The pinned ISO must stay byte-identical to keep the image reproducibility proof from WOR-27, macOS developer
hosts have no `mke2fs`, `xorriso` or Linux `apk`, and two experiments failed: an overlay on a second ISO device is
not found by the Alpine init, and an ISO built with macOS `hdiutil` is mounted by the guest as plain ISO 9660
(no Rock Ridge), which mangles file names.

## Decision

We will deliver guest customisation as an Alpine `apkovl` overlay packed into a second cpio archive appended to
the pinned initramfs (`build/systemvm/initramfs-vesper`), selected with `apkovl=/vesper/localhost.apkovl.tar.gz`
on the kernel command line. The overlay tree lives in `systemvm/overlay/`; the extra packages are pinned by URL
and SHA-256 in `docs/specs/systemvm-packages.lock.yaml`, downloaded into the cache, carried inside the overlay
under `var/lib/vesper/apks/`, and installed at boot by `etc/local.d/10-vesper-packages.start`.

## Alternatives considered

- **Rebuild the boot ISO** with the overlay at its root: the documented Alpine path, but needs `xorriso` on every
  host and makes the attached medium a build output instead of the pinned upstream file.
- **Second block device** holding the overlay: no tooling needed, but the init only searches the boot medium and
  `apkovl=/dev/vdb:iso9660:…` did not apply it on Alpine 3.22.
- **A custom rootfs or initramfs built on Linux**: full control, but a Linux-only build and a second image
  pipeline before any guest workload exists.
- **Installing packages from the network at boot**: simplest, but every boot would depend on the mirror and the
  reliability suite would measure the network.

## Rationale and consequences

The kernel unpacks concatenated cpio archives in order, so the pinned initramfs stays untouched and the derived
file is verified to start with it. Everything is produced by standard-library Python with fixed owners and
mtimes, so the overlay is deterministic for identical inputs on macOS and Linux CI. Easier: adding a guest file or
service is a commit under `systemvm/overlay/`; adding a package is a lock bump. Harder: packages are installed into
the tmpfs root on every boot (about half a second for the current 18), the overlay adds about 56 MB to the
initramfs and to guest RAM, and nothing persists across boots; a persistent data disk is a later decision.

## Implementation and migration impact

`src/systemvm/packages.rs` (typed packages lock), `overlay` section in the image lock (schema 4),
`scripts/systemvm/resolve-apks.py` (pins a closure), `fetch-packages.sh`, `build-initramfs.py`,
`build-overlay.sh`, `make systemvm-overlay` and `make systemvm-prepare`; `systemvmctl package-list` and
`verify-packages`; `verify-image` records the derived initramfs digest; the launch config's `initrd_path` becomes
the derived file. ExecPlan: [2026-11-06-WOR-25-systemvm-control-channel.md](../../exec-plans/completed/2026-11-06-WOR-25-systemvm-control-channel.md).

## References

- [ADR 0001](0001-systemvm-launcher-swift-helper.md), [systemvm-bringup.md](../../process/systemvm-bringup.md),
  [systemvm/overlay/README.md](../../../systemvm/overlay/README.md).
- Alpine initramfs `init` (`prepare_apkovl`, `unpack_apkovl`) inside the pinned `initramfs-virt`.
- GitHub #13, #14; Linear WOR-25, WOR-33.
