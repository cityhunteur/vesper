# Stock Firecracker and jailer lifecycle inside the System VM (WOR-31 / GH #18)

This ExecPlan is a living document. The sections Progress, Surprises & Discoveries, Decision Log, and
Outcomes & Retrospective must be kept up to date as work proceeds. It is maintained in accordance with
docs/PLANS.md.

## Purpose / Big Picture

GitHub #18 (Linear WOR-31, mirror #68) asks for Firecracker binary provisioning, jailer configuration and lifecycle
controls inside the System VM, proven by a microVM that launches, a teardown that removes sockets, processes and
mounts, jailer isolation (chroot, cgroup, seccomp) and cleanup after abnormal termination. After this plan
`make systemvm-up && make systemvm-microvm-suite` runs a stock Firecracker v1.16.0 microVM under the jailer on
nested KVM and proves all of that from the host.

## Progress

- [x] (2026-11-06 20:30Z) Packages lock schema 2 with a digest-pinned `archives` section (Firecracker v1.16.0
  aarch64 release: `firecracker` and `jailer` members into `/var/lib/vesper/firecracker/`), `e2fsprogs` pinned,
  resolver `--include-base` and `--extra-yaml`, `systemvmctl archive-list`, `verify-packages --archives-dir`,
  `fetch-packages.sh` archive handling, `build-initramfs.py --file`; guest `vesper-microvm`
  (`prepare|launch|status|isolation|stop|crash|cleanup|log`): kernel unpacked from the ISO's zboot image in the
  guest, busybox initramfs with a marker-printing init, jailer with uid/gid 10000, `--cgroup-version 2`,
  `pids.max` and `memory.max` limits, Firecracker's default seccomp; agent `MICROVM`; `control::run_microvm_suite`;
  `systemvmctl control microvm|microvm-suite`; `make systemvm-microvm-suite`. Suite 13/13 on an M3 Pro.
- [ ] PR merged, CI green, GH #18 checklist and WOR-31 updated, milestone entry; plan moved to completed.

## Surprises & Discoveries

- Observation: the single-file Firecracker config requires a `drives` array even when empty.
  Evidence: `missing field drives` in the first launch log.
- Observation: the jailer executes the binary by its chroot-relative name and uses `pivot_root` in a new mount
  namespace, so `/proc/<pid>/root` reads `/` from the host; the jail is judged by its contents (`vm.json`, no
  `/etc/alpine-release`) and by a different `mnt` namespace. Without a `--cgroup` option the jailer creates no
  cgroup at all.
  Evidence: first isolation output (`"root":"/"`, `"cgroup":"/openrc.vesper-agent"`), fixed in the second run.
- Observation: the Alpine `virt` kernel boots under Firecracker as a raw Image with an initramfs in about 0.9 s of
  guest time (1.5 s wall from launch to marker); its virtio and ext4 drivers are modules, so a block rootfs needs
  modules in the initramfs (WOR-28).
  Evidence: `run/WOR-31/systemvm/microvm-report.json`, console log lines.

## Decision Log

- Decision: provision the Firecracker v1.16.0 release binaries (first release containing the pinned baseline
  revision `083c4cf6…`) through the packages lock's `archives` section, digest-pinned, into the overlay.
  Rationale: same supply-chain rule as apks; the pinned revision is a `main` commit without release artifacts, and
  building Firecracker in CI is a later concern (TD-029).
  Date/Author: 2026-11-06, agent.
- Decision: the first microVM is kernel plus a self-built busybox initramfs with no drive; `--exit` makes the guest
  power off after the marker.
  Rationale: #18 is about lifecycle and isolation; the rootfs pipeline is #19.
  Date/Author: 2026-11-06, agent.
- Decision: isolation evidence is read from `/proc/<pid>` by the guest manager (uid, jail contents and mount
  namespace, cgroup path, seccomp mode 2) and judged by the host suite.
  Rationale: observable without instrumenting Firecracker.
  Date/Author: 2026-11-06, agent.

## Outcomes & Retrospective

See Progress; `run/WOR-31/systemvm/microvm-report.json` carries the 13 steps. Gaps: no rootfs drive yet
(WOR-28), the microVM has no network, and the binaries are a release rather than the exact pinned commit (TD-029).

## Context and Orientation

`systemvm/overlay/usr/local/bin/vesper-microvm` (guest manager), `systemvm/archives.yaml` and
`docs/specs/systemvm-packages.lock.yaml` (binaries), `src/control.rs` (`microvm`, `run_microvm_suite`). Terms:
*jail* is the jailer's chroot under `/srv/jailer/firecracker/<id>/root`; *marker* is `VESPER-MICROVM-OK` printed by
the microVM's init.

## Plan of Work

Done in one slice; see Progress.

## Concrete Steps

    cd <repo root>
    make systemvm-prepare && make systemvm-up
    make systemvm-microvm-suite
    cargo run --bin systemvmctl -- control microvm launch demo && cargo run --bin systemvmctl -- control microvm isolation demo
    cargo run --bin systemvmctl -- control microvm cleanup demo
    make systemvm-down

## Validation and Acceptance

`cargo test control packages`, `make systemvm-microvm-suite` with `passed: true`, overlay determinism, docs and
structure sensors, CI. Issue exit: the four GH #18 checklist items proven by the report.

## Idempotence and Recovery

`cleanup ID` always leaves the guest clean (it kills, removes the jail and the cgroup, and reports `DIRTY` with the
leftover kind otherwise); `prepare` is cached per boot.

## Artifacts and Notes

    ok   launch: LAUNCHED vesper-test pid=2762 boot_ms=1520
    ok   jailer_isolation: uid 10000, jail files "Image dev firecracker firecracker.pid initrd.gz run sys vm.json ", cgroup "/firecracker/vesper-test", seccomp mode 2
    ok   cleanup_after_crash: CLEAN vesper-test
    ok   guest_poweroff_exits: firecracker exited after the guest powered off

## Interfaces and Dependencies

`vesper::systemvm::{Archive, ArchiveMember, archive_list, verify_archives}`; `vesper::control::{Session::microvm,
run_microvm_suite}`; agent `MICROVM`; guest `vesper-microvm`. Guest binaries: Firecracker and jailer v1.16.0
(Apache-2.0). No new crate dependencies.
