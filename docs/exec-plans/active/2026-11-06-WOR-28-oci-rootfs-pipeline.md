# OCI image to ext4 rootfs pipeline for microVMs (WOR-28 / GH #19)

This ExecPlan is a living document. The sections Progress, Surprises & Discoveries, Decision Log, and
Outcomes & Retrospective must be kept up to date as work proceeds. It is maintained in accordance with
docs/PLANS.md.

## Purpose / Big Picture

GitHub #19 (Linear WOR-28, mirror #71) asks for a reproducible OCI image to ext4 conversion with provenance
(source image digest, baseline lock revisions, tooling versions), caching, clear errors for invalid images, and a
microVM that boots the generated rootfs. After this plan `make systemvm-up && make systemvm-rootfs-suite` imports a
trivial image into the guest's Docker, builds the rootfs twice to the same digest, reads the embedded provenance,
rejects an unknown image, and boots a jailed Firecracker microVM from the rootfs.

## Progress

- [x] (2026-11-06 20:45Z) Guest `vesper-rootfs` (`import-trivial`, `build IMAGE [--no-cache]`, `provenance PATH`,
  `inspect PATH`): `docker create` + `docker export`, Docker's injected `/etc/hostname`, `/etc/hosts`,
  `/etc/resolv.conf` and `.dockerenv` stripped, `/etc/vesper-provenance.json` embedded (source image id, repo tags,
  layer diff ids, created, architecture, os; baseline Firecracker and containerd revisions from the lock shipped into
  the overlay; mke2fs and Docker versions; build method), all mtimes forced to epoch 0, `E2FSPROGS_FAKE_TIME=0
  mke2fs -d` with UUID and hash seed derived from the image id, sidecar `<path>.provenance.json` with
  `output_sha256` written through `debugfs` (`e2fsprogs-extra` pinned), cache keyed by image id and verified by
  digest; microVM initramfs gains virtio-mmio, virtio-blk, crc32c and ext4 modules from the modloop and a
  `--rootfs` launch mode that mounts `/dev/vda`, prints `VESPER-ROOTFS-OK <image id>` from the embedded provenance
  and executes a command from the rootfs (or `switch_root` when it has `/sbin/init`); agent `ROOTFS`;
  `control::run_rootfs_suite`; `systemvmctl control rootfs|rootfs-suite`; `make systemvm-rootfs-suite`. Suite 11/11.
- [ ] PR merged, CI green, GH #19 checklist and WOR-28 updated, milestone entry; plan moved to completed.

## Surprises & Discoveries

- Observation: a third build of the same image produced a different digest until Docker's per-container files
  (`/etc/hostname` with the container id, `/etc/hosts`, `/etc/resolv.conf`) were stripped from the export; the
  first two builds had matched by chance.
  Evidence: first suite run, `cache_hit` step (`sha256=40a6…` vs `6163…`).
- Observation: `debugfs` is in `e2fsprogs-extra`, not `e2fsprogs`; without it the sidecar was empty and the cache
  never hit.
  Evidence: `provenance_embedded: EOF while parsing` in the first run.
- Observation: ext4 in this kernel needs the `crc32c` crypto driver (`crc32c_generic`, `libcrc32c`) in the
  initramfs; without it the mount fails with "Cannot load crc32c driver".
  Evidence: `VESPER-ROOTFS-MOUNT-FAILED` in the first run.

## Decision Log

- Decision: reproducibility is keyed by the Docker image id (config digest); two builds of one image are
  byte-identical. A re-import of the same tarball gets a new image id because `docker import` stamps `Created`, so
  content-keyed reproducibility across imports is not claimed (TD-031).
  Rationale: the issue asks for "same input → same output hash"; the image id is the input.
  Date/Author: 2026-11-06, agent.
- Decision: the embedded provenance holds only deterministic facts (no build timestamp); the build time is in the
  suite report instead.
  Rationale: a timestamp inside the image would defeat reproducibility.
  Date/Author: 2026-11-06, agent.
- Decision: the pipeline runs inside the System VM against its Docker daemon and `e2fsprogs`, driven from the host
  over the control channel.
  Rationale: the host has neither `mke2fs` nor a container runtime; the guest has both and the rootfs is consumed
  there.
  Date/Author: 2026-11-06, agent.

## Outcomes & Retrospective

See Progress; `run/WOR-28/systemvm/rootfs-report.json` carries the 11 steps. Gaps: TD-031; only the trivial image
has been converted (Docker Hub images need network and are not pinned); the rootfs has no `/sbin/init`, so the
microVM runs the proof command and idles.

## Context and Orientation

`systemvm/overlay/usr/local/bin/vesper-rootfs` and `vesper-microvm`, `src/control/suites.rs`
(`run_rootfs_suite`). Terms: *provenance* is `/etc/vesper-provenance.json` inside the image plus the sidecar;
*image id* is Docker's config digest.

## Plan of Work

Done in one slice; see Progress.

## Concrete Steps

    cd <repo root>
    make systemvm-prepare && make systemvm-up
    make systemvm-rootfs-suite
    cargo run --bin systemvmctl -- control rootfs build vesper/trivial:guest
    make systemvm-down

## Validation and Acceptance

`cargo test control`, `make systemvm-rootfs-suite` with `passed: true`, `make systemvm-microvm-suite` still
passing with the extended initramfs, docs and structure sensors, CI. Issue exit: the four GH #19 checklist items
proven by the report.

## Idempotence and Recovery

Builds are idempotent per image id (`--no-cache` forces a rebuild); the store is `/var/lib/vesper/rootfs/` in the
guest's tmpfs and is gone after a reboot.

## Artifacts and Notes

    ok   build: BUILT /var/lib/vesper/rootfs/153d32a58b18c34c.ext4 sha256=9ca8b36c… bytes=16777216 cached=false
    ok   reproducible_rebuild: sha256=9ca8b36c… twice from image 153d32a5…
    ok   provenance_embedded: image 153d32a5…, 1 layer(s), firecracker "083c4cf69281178acc910f7f857f88e69c51d3b5"
    ok   invalid_image_rejected: ERR image not found: vesper/does-not-exist:nope
    ok   rootfs_boots: VESPER-ROOTFS-OK 153d32a5…; command executed from the rootfs

## Interfaces and Dependencies

`vesper::control::{Session::rootfs, run_rootfs_suite}`; agent `ROOTFS`; guest `vesper-rootfs`; `vesper-microvm
launch ID --rootfs PATH`. Guest packages add `e2fsprogs-extra`. No new crate dependencies.
