# OCI bundle validation and block-first rootfs preparation (WOR-37 / GH #24)

This ExecPlan is a living document. The sections Progress, Surprises & Discoveries, Decision Log, and
Outcomes & Retrospective must be kept up to date as work proceeds. It is maintained in accordance with
docs/PLANS.md.

## Purpose / Big Picture

GitHub #24 (Linear WOR-37, mirror #76) asks the shim to validate OCI bundles, prepare the microVM root drive
block-first from shared content-store inputs with reproducible, failure-safe behaviour, persist state for
recovery and audit, and surface actionable diagnostics. After this plan every task from the same image layer
chain shares one ext4 build in `/var/lib/vesper/rootfs-cache`, gets a private copy, and leaves a
`vesper-rootfs.json` record in its bundle; malformed bundles are rejected before anything is mounted.

## Progress

- [x] (2026-11-08) `shim/internal/bundle`: `Validate` (absolute, real directory, `config.json` parses, has
  `ociVersion`, `process.args`, absolute `cwd`, expected `root.path`, rootfs mounts or a populated directory,
  complete mounts) returning `ValidationError{Reason, Hint}`; `Inspect` derives `Layers{Kind, Lower, Upper,
  Key}` (overlay lower chain → sha256 key; other mounts → identity key; no mounts → directory fallback, no
  key); `Meta` record with `WriteMeta`/`ReadMeta` (atomic, one line). `backend.Prepare(task, bundle, mounts)`
  builds `<cache>/<key>.ext4` once per chain with a `.sha256` sidecar (temp file then rename), verifies the
  digest on every hit, copies a private `rootfs.ext4` per task, falls back to a direct per-task build for a
  directory rootfs, and wraps failures as `PrepareError{Step, Hint}`. `Create` validates first and returns
  `InvalidArgument` with the diagnosis; preparation failures return `FailedPrecondition`. Shim runs without the
  framework reaper (its SIGCHLD reaping raced `exec.Cmd.Wait`). Guest `vesper-ctr import-layered|run-image|
  start-image|meta|cache`; `control::run_bundle_suite` (`src/control/suites/bundle.rs`); `systemvmctl control
  bundle-suite`; `make systemvm-bundle-suite`. Suite 15/15 on an M3 Pro: a two-layer image boots and reads its
  second-layer file, two tasks from the chain share key and digest with `cached=true` in 16 ms, one cached drive
  per chain, the layered image's chain reuses the base image's snapshot. Unit tests: 7 malformed bundles,
  keys, metadata round trip, cache validity and copy. Workload, microvm, rootfs, parity and shim suites pass.
- [x] (2026-11-08) PR #134 merged as `d52cff2`, CI green (seven workflows), GH #24 checklist ticked, mirror #76 closed, WOR-37 Done, milestone entry; plan moved to completed.

## Surprises & Discoveries

- Observation: the shim framework reaps every child on SIGCHLD, so `exec.Cmd.Wait` on `vesper-microvm`
  sometimes failed with `waitid: no child processes` (one launch in roughly twenty). The shim now runs with
  `NoReaper` and `NoSubreaper`; microVM processes are `setsid` children of a short-lived shell and reparent
  to PID 1.
- Observation: `mke2fs -d` output is not byte-identical across builds of the same tree (timestamps), so the
  cache is keyed by the layer chain and verified by its own digest, not by a content-derived digest (TD-035).

## Decision Log

- Decision: the cache key is the ordered lower-layer snapshot chain of the overlay mount.
  Rationale: within one containerd instance the chain identifies the image content exactly (containerd
  dedups snapshots by chain id, which the dedup step proves); it is available before any mount and costs
  nothing to compute.
  Date/Author: 2026-11-08, agent.
- Decision: a pre-populated rootfs directory is the documented fallback: built per task, never cached.
  Rationale: it has no stable identity; the record says `fallback: true, strategy: direct`.
  Date/Author: 2026-11-08, agent.
- Decision: a per-task copy rather than a shared read-only drive.
  Rationale: Firecracker opens the drive read-write and tasks must not see each other's writes; CoW or
  overlay drives stay TD-032.

## Outcomes & Retrospective

See Progress; `run/WOR-37/systemvm/bundle-report.json` carries the 15 steps. Gaps: TD-035 (cache never
pruned, not content-addressed, per containerd instance), TD-034 narrowed to exec/TTY/stdin and the shell
requirement.

## Context and Orientation

`shim/internal/bundle`, `shim/internal/backend` (`Prepare`), `shim/internal/task/service.go` (`Create`),
`systemvm/overlay/usr/local/bin/vesper-ctr`, `src/control/suites/bundle.rs`.

## Plan of Work

Done in one slice; see Progress.

## Concrete Steps

    make shim-test && make systemvm-prepare && make systemvm-up
    make systemvm-bundle-suite
    cargo run --bin systemvmctl -- control ctr cache
    make systemvm-down

## Validation and Acceptance

`make shim-test`, `make systemvm-bundle-suite` with `passed: true`, the other guest suites still passing,
docs and structure sensors, CI.

## Idempotence and Recovery

The cache survives across tasks for the System VM's lifetime (tmpfs); a corrupted entry fails its digest
check and is rebuilt; `rm -rf /var/lib/vesper/rootfs-cache` in the guest resets it. `vesper-rootfs.json`
in a bundle tells which drive a task got, from where, and how long it took.

## Artifacts and Notes

    ok   layered_rootfs_boots: second layer's file read from the block-first root drive
    ok   meta_la_recorded: strategy block-first, overlay, 2 lower layers, cached=true, build_ms=16
    ok   meta_lb_cache_hit: cached=true, same key and sha256 as la, build_ms=17
    ok   layer_dedup_shared_base: base layer snapshot …/snapshots/1/fs shared by both images

## Interfaces and Dependencies

No new dependencies. `backend.Backend.Prepare` signature changed to `(ctx, task, bundle, mounts)`.
