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
