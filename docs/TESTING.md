---
status: verified
owner: cityhunteur
last_reviewed: 2026-11-06
---

# Testing

This page explains what kinds of tests exist, where they live, how to run exactly the ones you need, and what evidence
a change needs before it counts as done. While iterating, run the narrowest relevant check. Before declaring done, run
the broader suite. `cargo test` was run successfully on 2026-11-05.

## Test inventory

| Category | Implemented? | Location | Framework | Command | Needs |
|---|---|---|---|---|---|
| Unit | Yes | `#[cfg(test)]` modules in `src/*.rs`; API handler tests in `src/api/tests.rs` | Rust test harness, `tokio::test` for proxy, `tempfile` | `cargo test --lib` | nothing |
| Component | Yes (router level) | `src/api/tests.rs`, `tests/api_contract.rs` drive the axum router with `tower::ServiceExt::oneshot` | Rust, `tower` | `cargo test --test api_contract` | nothing |
| Integration | Partial | `tests/contract_golden.rs` loads the vendored swagger and golden JSON from disk | Rust | `cargo test --test contract_golden` | repo checkout |
| Contract | Yes | `contractgen --mode check`, `tests/api_contract.rs`, generated matrix validation | Rust binaries | `make contract-test`, `make surface-matrix-check`, `make behavior-matrix-check` | nothing |
| End-to-end | **Not implemented** | design-v0 describes Docker-replacement e2e; no code | n/a | n/a | a System VM launcher (TD-005) |
| Performance | **Not implemented** | design-v0 describes regression and perf gates; no code | n/a | n/a | n/a |
| Security / static analysis | Partial | `scripts/ci/validate-entitlements.sh`, `scripts/harness/check_structure.py` | bash, Python | `make validate-entitlements` (CI: `rust-tests.yml`), `python3 scripts/harness/check_structure.py --root .` | Python 3 |
| Runtime smoke | Yes | `make systemvm-boot-smoke` boots the pinned Alpine VM through `vesper-vmlauncher` N times, logs in, proves NAT (DHCP + gateway ping), powers off, and fails any boot over the lock's 5 s threshold; report in `run/systemvm/boot-report.json` | Rust + Swift | `make systemvm-fetch systemvm-launch-config vmlauncher-build systemvm-boot-smoke` | macOS on Apple silicon, Xcode CLT; not runnable in CI |
| Control channel | Yes | `systemvmctl control suite` runs the GH #13 checklist (handshake, keepalive, reconnect after a server-side drop, 2 MiB echo) against the running guest through the launcher's bridge; `src/control.rs` unit tests run the same client against a fake bridge | Rust | `make systemvm-up && make systemvm-control-suite && make systemvm-down`; `cargo test control` | macOS for the real guest; nothing for the unit tests |
| Runtime services | Yes | `systemvmctl control services-suite` runs the GH #14 checklist per daemon (healthy at boot, SIGKILL, supervised recovery, startup and recovery lines in the guest logs) through the bridge | Rust | `make systemvm-up && make systemvm-services-suite && make systemvm-down`; `cargo test control` | macOS for the real guest |
| Docker ingress | Yes | `make docker-smoke` runs the GH #15 checklist with the macOS Docker CLI through `vesper-proxy`: owner-only socket, `docker version`, `docker info`, `docker import` of the offline trivial image, `docker run`, host socket enumeration, refused raw guest port | bash, Docker CLI | `make systemvm-up && make docker-up && make systemvm-trivial-image && make docker-smoke` | macOS, Docker CLI on the host |
| Firecracker lifecycle | Yes | `make systemvm-microvm-suite` launches a stock Firecracker v1.16.0 microVM under the jailer inside the System VM (nested KVM), checks uid/jail/cgroup/seccomp isolation, clean stop, cleanup after SIGKILL and guest power-off; report in `run/systemvm/microvm-report.json` | Rust + guest shell | `make systemvm-up && make systemvm-microvm-suite`; `cargo test control` | macOS on an M3 or later |
| OCI to ext4 rootfs | Yes | `make systemvm-rootfs-suite`: image import, two builds with one digest, cache hit, embedded provenance, invalid-image error, and a jailed microVM booting the rootfs; report in `run/systemvm/rootfs-report.json` | Rust + guest shell | `make systemvm-up && make systemvm-rootfs-suite` | macOS on an M3 or later |
| MicroVM per container | Yes | `make systemvm-workload-suite`: three concurrent workloads in three jailed microVMs, isolation between them, stop of one leaving the others, exit-code and output mapping, full removal; report in `run/systemvm/workload-report.json` | Rust + guest shell | `make systemvm-up && make systemvm-workload-suite` | macOS on an M3 or later |
| Parity baseline | Yes | `make systemvm-parity-baseline`: 19 Firecracker API conformance checks against the stock release binary in a jailed microVM, two performance runs compared within 20 %, merged with the lock, parity metadata and compat registry into `run/systemvm/parity-baseline-report.json`; `make parity-baseline-check` validates the committed capture | Rust + guest shell + Python | `make systemvm-parity-baseline PERF_CYCLES=20 PARITY_FLAGS=--publish` | macOS on an M3 or later; the check runs anywhere |
| Shim Task RPCs | Yes | `make shim-test` (Go unit tests: state machine, command encoding, log follower) and `make systemvm-shim-suite` (21 steps through `ctr` with the Vesper shim: e2e exit code and output, running state, invalid sequences, kill, delete, three concurrent tasks, no leftovers); report in `run/systemvm/shim-report.json` | Go + Rust + guest shell | `make systemvm-up && make systemvm-shim-suite` | macOS on an M3 or later; the Go tests run anywhere |
| Block-first rootfs | Yes | Go unit tests (7 malformed bundles, layer keys, metadata, cache validity) and `make systemvm-bundle-suite` (15 steps: two-layer image boots, cache hit with same key and digest, one drive per chain, base snapshot shared); report in `run/systemvm/bundle-report.json` | Go + Rust + guest shell | `make systemvm-up && make systemvm-bundle-suite` | macOS on an M3 or later; the Go tests run anywhere |
| Reliability | Yes | `make systemvm-reliability` runs N probed boots (default 50) under the threshold, then the control, services and Docker suites on a kept-up VM, checks for leaked processes, sockets and pid files and for boot-time drift, and writes `run/systemvm/reliability-report.json` with actionable `failures[]` | bash, Python | `make systemvm-reliability` (`CYCLES=200 …`); `python3 -I scripts/systemvm/reliability_report.py --self-test` (CI) | macOS on Apple silicon for the run |
| Image reproducibility | Yes | `systemvmctl verify-image` checks the unpacked kernel, initramfs and ISO digests and the `expected_image_id` against `docs/specs/systemvm-image.lock.yaml`; `verify-packages` checks every guest package against `systemvm-packages.lock.yaml`; the derived initramfs must start with the pinned one | Rust, bash, Python | `make systemvm-fetch`, `make systemvm-overlay` (CI: `systemvm-image.yml`, Linux) | network on first run (ISO and packages cached afterwards); `bsdtar` |
| Documentation | Yes | `harness/tests/`, `harness/check_docs.py`, `scripts/harness/check_docs.py` | unittest | `python3 harness/check_docs.py --strict`, `python3 -m unittest discover -s harness/tests`, `python3 scripts/harness/check_docs.py --root .` | Python 3.11+ |

Suite sizes on 2026-11-05: `tests/api_contract.rs` 115 tests, `tests/contract_golden.rs` 1, `src/api/tests.rs` 67,
plus smaller unit modules in `api_matrix`, `api_behavior_matrix`, `proxy`, `baseline`, `compat`, `spec`, `platform`
and `systemvm`.

## Running targeted tests

```sh
cargo test --lib api::                         # every test under src/api (module path filter)
cargo test --test api_contract                 # one integration suite
cargo test --test api_contract guards          # tests whose name contains a substring (the state-guard suites)
cargo test proxy::                             # proxy unit tests (tokio)
cargo test -- --nocapture                      # show stdout from tests
```

## Running the full suite

```sh
cargo test                     # what baseline-governance.yml runs
make contract-check            # what contract-tests.yml runs first
make contract-test             # contract-check + the two integration suites
```

CI workflows and what each runs, all on Linux and all reproducible locally:

- `rust-tests.yml` (any `src/**`, `tests/**`, `Cargo.*`, `Makefile`, `third_party/**`, `docs/generated/**`,
  `docs/specs/*.yaml`, `packaging/**`, `scripts/ci/**` change): `make build`, `cargo test`,
  `make surface-matrix-check`, `make behavior-matrix-check`, `make validate-entitlements`.
- `contract-tests.yml` (swagger, `src/spec.rs`, `src/api.rs`, `contractgen`, golden file): golden check, `make contract-test`.
- `shim-tests.yml` (`shim/**`, `build-shim.sh`): `go vet`, `go test`, cross-build, artifact upload.
- `parity-baseline.yml` (the committed capture, lock, registry, archives, assembler): assembler self-test, `--validate` of `docs/specs/backend-a-parity-baseline.json`, artifact upload.
- `baseline-governance.yml` (lock file, `src/baseline.rs`, `baselinectl`): `cargo test`, metadata generate and validate.
- `compat-exceptions.yml` (registry, `src/compat.rs`, `compatexctl`): `cargo test`, `compatexctl`; a `workflow_dispatch`
  input enables the zero-open gate. There is no schedule, so a calendar expiry is caught only on the next run.
- `harness-docs.yml`, `harness-structure.yml`, `harness-quality-score.yml`: the Python sensors on docs and script changes.
- `harness-garbage-collection.yml`: weekly janitor scan with `--fail-on never` (non-blocking).
- `docs.yml` (every PR and push): `harness/check_docs.py --strict` and its unit tests.

## Fixtures and environments

- `third_party/firecracker/firecracker.yaml`: the pinned OpenAPI every router test loads.
- `testdata/contracts/firecracker-routes.golden.json`: golden route list, regenerated only by `contractgen`.
- `docs/specs/*.yaml` and `docs/generated/*.json`: lock, registries and matrices read by `baselinectl` tests.
- Handler tests that need files (kernel, drive, log and metrics paths) create them with the `tempfile` crate.
- Each test builds its own `AppState`, so suites run in parallel under the default Cargo runner; `cargo test` passed
  with parallel execution on 2026-11-05.

## Reproducing failures

- Re-run the exact CI command from the workflow file listed above; all are plain `make` or `cargo` invocations.
- `RUST_BACKTRACE=1 cargo test <name> -- --nocapture` for a panic in one test.
- There is no randomness to seed and no known flaky-test list (**Unknown** whether any test is flaky; none was
  observed).
- Runtime-sensitive work (proxy, System VM) must also leave artifacts under `run/<ISSUE-ID>/` and
  `.symphony/runs/<ISSUE-ID>/current/`; see [RELIABILITY.md](RELIABILITY.md).

## Definition of done

A change is complete when:

1. A test that fails without the change passes with it, or the issue or PR explains why no test is possible.
2. The targeted tests and every suite CI will run for the touched area pass locally, and CI is green for the pushed SHA.
3. For API or parity changes, `make contract-test`, `make surface-matrix-check` and `make behavior-matrix-check` pass;
   for governance changes, `baselinectl --mode generate` then `--mode validate` and `compatexctl` pass.
4. Behaviour you cannot see in tests is shown with evidence: a command transcript, proxy log, boot report or
   verification bundle.
5. `python3 harness/check_docs.py` and `python3 scripts/harness/check_docs.py --root .` pass, and the owning docs are
   updated.
6. The linked GitHub issue records the commit SHA, local verification, CI result and remaining gaps
   ([CLAUDE.md](../CLAUDE.md), [harness-engineering.md](process/harness-engineering.md)).
