# GH #172: a committed runner for the upstream Firecracker suite on Backend A, with a comparator

This is workstream W3 of `docs/exec-plans/active/2026-11-13-outstanding-work-programme.md` (D-029). It turns the
GH #166 trial, which exists only as untracked scripts in `run/GH-166/`, into a repeatable, committed procedure. With
ADR 0007 complete, this is the last piece of "code complete" for Backend A.

## Purpose / Big Picture

After this change, a developer on an Apple M3 or later runs one command, `make systemvm-upstream-suite`. It runs the
upstream integration tests (`integration_tests/functional` and `integration_tests/security`) unmodified against the
Firecracker and jailer that Vesper ships, inside the System VM. The run uses the upstream build container and the CI
artefacts pinned in the baseline lock, and no test can hang the run. It writes one `{nodeid: outcome}` file.

A comparator then explains every test that did not pass, and fails when it cannot. It compares in one of two ways:

- **Against a Linux pass set (CE-0003, later):** every test that passes on Linux but not on Backend A must match a
  recorded exception class.
- **Without a Linux pass set (today):** every test that does not pass must match a class, and the comparator
  reports the count per class.

Either way, a new, unexplained failure turns the run red.

## Progress

- [x] M1 Pins: the artefact prefix, the build container and the suite selection in the baseline lock, parsed and
  validated by `src/baseline.rs`
- [x] M2 Exception classes: `docs/specs/upstream-suite-classes.yaml`, seeded from the 13 GH #166 classes, each linked to
  a compat exception or a tech-debt row
- [x] M3 Comparator: `baselinectl --mode upstream-compare`, with unit tests (it fails on an unmatched test)
- [x] M4 Runner: `scripts/systemvm/upstream-suite/` and `make systemvm-upstream-suite` (fetch, stage, `tun`, resize,
  run with timeouts, collect, wipe `/srv`, restore)
- [x] M5 Full run on this host, results classified, the comparator green; evidence under `run/GH-172/`
- [ ] M6 Docs (TESTING, systemvm-bringup, the parity matrix note, src/bin README), CI green, #172 updated and closed

## Surprises & Discoveries

- 2026-11-13: the 60 GB disk preflight could never pass on the default 62.4 GB data disk; it is 45 GB, which the
  full run kept (48 GB free before it, `/srv` removed after it).
- 2026-11-13: re-staging onto an existing source volume failed on `gunzip` ("already exists"). Staging now clears the
  volume's `build/` except the cargo cache on every run.
- 2026-11-13: in the full run (`run/GH-172/20261113T081827Z`, 980 tests, 77 min) two `test_attach_maximum_devices`
  cases (6.1 kernel) reached the 600 s timeout. `--timeout-method=thread` ends the whole xdist worker, which reports
  "worker 'gwN' crashed", not "Timeout (". The other two workers ran 577 to 662 s of tests while each crashed test
  ran, which fits the timeout; the 5.10 cases passed in 25 and 46 s. Class `max-devices-worker-timeout`.
- 2026-11-13: `collect.py` read only the `setup`, `call` and `teardown` phases, but pytest-xdist puts a dead
  worker's crash in a phase named `???`, so those two results had no message. It now reads every phase.
- 2026-11-13: one of twelve `test_rng_bw_rate_limiter` cases (6.1, PCI off, 1000 KB/s) overran its 100 s SSH timeout.
  It joins the `unexplained` rate-limiter class; only the Linux run (CE-0003) can say whether it is timing.
- 2026-11-13: pytest's summary says 787 passed and 28 errors; the collector reports 763 passed, because 24 tests
  passed their call phase and then failed teardown (the VMM memory check), and the worst phase counts.
- 2026-11-13: `pytest.log` was 561 MB (tracebacks print every local of the cache-topology failures). The runner
  now also writes `pytest-progress.log` (result lines, worker crashes and the short summary, 328 KB) and
  `junit.xml.gz`; those are committed, and `pytest.log` and `report.json.gz` (23 MB) stay local.
- 2026-11-13: the council review of the first run's change (four High findings) found that the classes matched on
  the file alone, so a new failure in a covered file would pass. Every class now names its test functions, a symptom
  message and a `max`; a test missing from the reference must be explained; and pytest's exit code and collection
  errors fail the run. The images are pinned by digest and the artefacts by a SHA-256 manifest.
- 2026-11-13: the four `test_uffd.py` handler tests in the first run failed with FileNotFoundError: the runner had
  not staged the uffd example binaries. Hard-linking them into the test tree then failed with EXDEV, because
  `/binaries` and `/srv` are different volumes. The runner now copies the binaries and examples to `/srv/binaries`
  and passes it as `--binary-dir`; the quick run `20261113T101608Z` shows all eight uffd tests as `no-userfaultfd`.
- 2026-11-13: the VMM memory overshoot ranges from 5.00 to 6.79 MiB, not "up to 5.84" as the GH #166 trial said,
  so the class matches 5.x and 6.x and nothing larger.
- 2026-11-13: the second full run (`20261113T101748Z`, 980 tests, 88 min, 756 passed) was judged first against the
  classes as they stood: 7 results matched none. All were timing under load, each in a test that passed in the
  first run: three sysgenid SSH timeouts, one reboot exit over 10 s, one balloon stats read that had not refreshed,
  one vsock reset that crashed its worker, and one memory overshoot (6.65 MiB) in a test the class did not yet
  name. The classes gained those tests, two new flaky classes with `max: 1` were added, and the comparator is green
  on the second run. The verdicts at the time are kept as `compare-at-run.*`. Against today's classes the first run
  still fails on its four uffd results, which is the harness gap above.

## Decision Log

- 2026-11-13: the comparator is a `baselinectl` mode, not a new binary, because it reads the baseline lock and
  emits parity evidence, which is `baselinectl`'s job. It matches node ids and messages by substring, with `*`
  wildcards in node ids, so that no `regex` crate is added (a new dependency needs the owner).
- 2026-11-13: until CE-0003 exists, "reference" means "every test passes". So every failure, error, hang, or
  skip not allowed by a class must be explained. This is stricter than comparing against Linux, never looser.
- 2026-11-13: the per-test timeout is pytest-timeout with `--timeout-method=thread`, which kills the worker. The
  trial's two 10-hour hangs happened under the default signal method. An outer watchdog also stops the container
  at a wall-clock limit (default 4 h), and a test still running is then recorded as `hung`.

## Outcomes & Retrospective

The full run on an Apple M3 Pro (System VM 4 vCPUs, 8 GiB, 3 workers) took 4,616 s. Of 980 tests, 763 passed and
the comparator explained the other 217 with no unmatched test: 96 platform skips, 49 environment, 8 test
assumption and 64 unexplained (54 VMM memory overshoot, 8 rate-limiter or balloon timing, 2 jail hard-link
EXDEV), the last three classes open under CE-0001 until the Linux reference run. The GH #166 trial's seven harness
gaps are gone, and no test hung past its timeout. With `vmm-memory-overhead` removed the comparator exits 1 and
names its 54 tests. Two more quick runs of `test_api.py` (`20261113T093902Z`, `20261113T094104Z`, 62 and 60 s of pytest)
gave the same 67 outcomes as the first (59 passed, 6 skipped, 2 errors), test for test. The System VM came back on 2 vCPUs and 2048 MiB, and volume `vesper-upstream-srv` was removed.

A second full run after the council fixes (`20261113T101748Z`, 5,305 s) passed 756 and, with the classes widened
for the seven timing results it showed (see Surprises), explained the other 224: 96 skips, 48 environment, 8 test
assumption and 72 unexplained. Between the two runs 23 tests went from passed to a class and 16 the other way, all in
the memory-overshoot, SSH, balloon, vsock, vmclock or reboot classes. The run is therefore reproducible in its classes
but not test for test: timing under nested virtualisation moves a few dozen tests between passed and a class. The `max` caps keep that drift bounded.

## Context and Orientation

**Baseline revision:** upstream Firecracker v1.16.0 is pinned at `d83d72b710361a10294480131377b1b00b163af8` in
`docs/specs/design-v0.baselines.lock.yaml`. Decision D-023 makes the upstream suite the Backend A half of the pass-set
procedure, and D-024 keeps the Linux reference run (CE-0003) to one run, at code complete.

**The trial (GH #166, `run/GH-166/summary.md`) and what it showed the runner needs:**
- **Container:** the upstream build image `public.ecr.aws/firecracker/fcuvm:v90`, run `--privileged` on the System
  VM's Docker with `/dev` mounted.
- **Tree:** the upstream tree on a Docker volume on the data disk. It cannot be on the virtio-fs share, because
  `unsquashfs` fails there.
- **Binaries:** `--binary-dir` holds the v1.16.0 release binaries, which are digest-identical to
  `/var/lib/vesper/firecracker`, plus the upstream examples built at the pinned revision.
- **Artefacts:** the CI artefacts come from the public bucket `spec.ccfc.min`, prefix
  `firecracker-ci/20260603-e656faf58d6c-0/aarch64`.
- **Kernel modules:** `tun` must be loaded first.
- **Tool and debug files:** some tests ignore `--binary-dir` and expect the tool binaries and `*.debug` files under
  `build/cargo_target/aarch64-unknown-linux-musl/release`.
- **Disk:** `/srv` reached 42 GB, so it is wiped after every run.
- **Size and duration:** the System VM ran with 4 vCPUs and 8 GiB at `-n 3`, for about 70 to 80 minutes.

**Trial scripts:** `run-suite.sh`, `fetch-artifacts.sh`, `stage-volume.sh`, `build-examples.sh`, `tabulate.py`,
`classify.py`. The classes in `classify.py` are the seed for M2.

**Run plumbing:**
- `scripts/systemvm/generate-launch-config.sh` writes `build/systemvm/launch-config.json`, and
  `scripts/systemvm/up.sh` and `down.sh` start and stop the System VM.
- The System VM's Docker is reached by `DOCKER_HOST=unix://$PWD/run/vesper/docker.sock` after `make docker-up`.
- Attached `docker run` output was lost in the trial (GH #167, fixed since). The runner still runs detached and
  reads `docker logs`, which is more robust for a run of several hours.

## Plan of Work

**M1.** Add an optional `upstream_suite` block under `baselines.firecracker` in the lock:

    upstream_suite:
      container: "public.ecr.aws/firecracker/fcuvm:v90"
      artifacts_bucket: "spec.ccfc.min"
      artifacts_prefix: "firecracker-ci/20260603-e656faf58d6c-0/aarch64"
      release_asset: "firecracker-v1.16.0-aarch64.tgz"
      paths: ["integration_tests/functional", "integration_tests/security"]
      workers: 3
      test_timeout_s: 600

Extend `src/baseline.rs:BaselineRef` with `upstream_suite: Option<UpstreamSuite>`, validate it (non-blank, prefix
ends in the host architecture, `workers` from 1 to 8, timeout from 60 to 3600 s), and add tests.

**M2.** Write `docs/specs/upstream-suite-classes.yaml`. Each class has:
- an `id` and a `kind` (`environment`, `test-assumption`, `harness`, `unexplained`);
- `nodeid` patterns and optional `message` substrings;
- `outcomes` (the outcomes it may explain);
- a `tracked_by` reference (CE-nnnn or TD-nnn) and a one-line `reason`.

Seed the classes from the GH #166 table. Harness gaps are not classes: M4 fixes them. "Unexplained" classes must
cite CE-0001 (the open upstream-suite exception) until a Linux run explains them. Skips are a class kind too
(x86-only, cgroup v1, guest-kernel), so a new skip is noticed.

**M3.** `baselinectl --mode upstream-compare --results R.json [--reference L.json] --classes C.yaml --report OUT.json`:
- it reads the outcomes, applies the reference rule from the Decision Log, and matches each test that needs
  explaining against the classes, first match wins;
- it writes `{summary, per_class, unmatched, stale_classes}`, where `stale_classes` are classes that matched no test
  (reported, not fatal);
- it exits 1 on any unmatched test.

Unit tests cover the cases below. A small Python step in the runner (`collect.py`) converts pytest-json-report into
`{nodeid: {outcome, message}}`.

| Unit test case | Expected |
|---|---|
| A matched failure | passes |
| An unmatched failure | exits 1 |
| A hang | must be explained like a failure |
| A test passing on the reference and skipped here | must be explained |
| A test failing on both | needs no class |
| A class whose `outcomes` exclude the observed outcome | does not match it |

**M4.** `scripts/systemvm/upstream-suite/run.sh` runs these steps, each idempotent and logged to
`run/GH-172/<stamp>/`:
1. **Preflight:** check free space on the data disk (45 GB by default; the default disk is 64 GiB, 62.4 GB usable) and that Docker is up.
2. **Fetch:** shallow-fetch the upstream tree at the lock revision into `build/upstream-suite/firecracker`. Download
   the artefacts at the pinned prefix with retries, then check the object count and sizes against the bucket
   listing. Download the release tarball and check its sha256 against `systemvm/archives.yaml`.
3. **Stage:** copy the tree into Docker volume `vesper-upstream-src`. Run `tools/setup-ci-artifacts.sh` there. Copy
   the release tools and `.debug` files into the `build/cargo_target/...` path that some tests expect. Build the
   examples in the container, with the cargo cache in a volume.
4. **Resize:** `VESPER_CPUS=4 VESPER_MEMORY_MIB=8192` added to `generate-launch-config.sh` (the defaults are
   unchanged), then a System VM restart. A `trap` restores the default config and restarts on every exit path.
5. **`tun`:** `modprobe tun` in the System VM.
6. **Run:** detached `docker run` of `tools/test.sh --binary-dir /binaries -n <workers> --dist worksteal --timeout
   <s> --timeout-method=thread --json-report …` over the pinned paths. A watchdog loop stops the container at the
   wall-clock limit.
7. **Collect:** `docker logs`, the JSON report, the junit file, and `collect.py` into `results.json`.
8. **Wipe:** remove the `/srv` volume and the test artefacts. The source volume is kept for the next run, so
   `--clean` removes it.
9. **Compare:** run the comparator and print its summary.

The Make target is `systemvm-upstream-suite`. `UPSTREAM_ARGS` passes a node-id selection for a quick partial run
(the comparator then judges only the selected tests).

**M5.** Run in full on this host and classify. Every unmatched test either gets a class (with the reasoning in
Surprises) or is a real bug, which gets a fix or a CE entry. Run `test_api.py` twice more as a quick determinism check.

**M6.** Update the owning docs, then push, wait for CI, and comment on #172 with the evidence. Close #172 only when
its four acceptance criteria hold.

## Concrete Steps

    cargo test --lib baseline && cargo test --bin baselinectl
    make systemvm-upstream-suite UPSTREAM_ARGS="integration_tests/functional/test_api.py"   # quick
    make systemvm-upstream-suite                                                          # full, about 80 min
    cargo run --bin baselinectl -- --mode upstream-compare --results run/GH-172/<stamp>/results.json \
        --classes docs/specs/upstream-suite-classes.yaml --report run/GH-172/<stamp>/compare.json

## Validation and Acceptance

| Check | Expected |
|---|---|
| Full run | completes with no hang longer than the per-test timeout |
| System VM after the run | back on the default launch config |
| `/srv` volume after the run | gone |
| Comparator on the full results | exits 0 |
| Comparator with one class removed | exits 1 and names the tests that class explained |
| Quick run | finishes in under 15 minutes on a warm cache |

These four results satisfy #172's acceptance criteria.

## Idempotence and Recovery

Every step can be rerun. Fetch skips any object that is present with the listed size. Stage rebuilds the volume only
with `--clean`. Resize is undone by the trap. If the process is killed with `-9`, `make systemvm-launch-config &&
make systemvm-up` restores the defaults. The runner never touches containers or volumes outside its `vesper-upstream-*`
names, and it removes its own container before starting.
