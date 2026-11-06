---
status: verified
owner: cityhunteur
last_reviewed: 2026-11-05
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
| End-to-end | **Not implemented** | design-v0 describes Docker-replacement e2e; no code | n/a | n/a | real System VM images (TD-010) |
| Performance | **Not implemented** | design-v0 describes regression and perf gates; no code | n/a | n/a | n/a |
| Security / static analysis | Partial | `scripts/ci/validate-entitlements.sh`, `scripts/harness/check_structure.py` | bash, Python | `make validate-entitlements`, `python3 scripts/harness/check_structure.py --root .` | Python 3 |
| Runtime smoke | Scaffold | `systemvmctl boot-smoke` runs a command N times and writes a report; proves the harness, not a VM boot | Rust | `make systemvm-boot-smoke` | nothing (defaults to `sh -c 'echo boot-ok'`) |
| Documentation | Yes | `harness/tests/`, `harness/check_docs.py`, `scripts/harness/check_docs.py` | unittest | `python3 harness/check_docs.py`, `python3 -m unittest discover -s harness/tests` | Python 3.11+ |

Suite sizes on 2026-11-05: `tests/api_contract.rs` 115 tests, `tests/contract_golden.rs` 1, `src/api/tests.rs` 67,
plus smaller unit modules in `api_matrix`, `api_behavior_matrix`, `proxy`, `baseline`, `compat`, `spec`, `platform`
and `systemvm`.

## Running targeted tests

```sh
cargo test --lib api::                         # every test under src/api (module path filter)
cargo test --test api_contract                 # one integration suite
cargo test --test api_contract guarded_routes  # tests whose name contains a substring
cargo test proxy::                             # proxy unit tests (tokio)
cargo test -- --nocapture                      # show stdout from tests
```

## Running the full suite

```sh
cargo test                     # what baseline-governance.yml runs
make contract-check            # what contract-tests.yml runs first
make contract-test             # contract-check + the two integration suites
```

CI workflows and what each runs: `contract-tests.yml` (golden check, `make contract-test`), `baseline-governance.yml`
(`cargo test`, metadata generate and validate), `compat-exceptions.yml` (`compatexctl`), `harness-docs.yml`,
`harness-structure.yml`, `harness-quality-score.yml`, `harness-garbage-collection.yml` (Python sensors), `docs.yml`
(`harness/check_docs.py` and its unit tests). All of them run on Linux and are practical to reproduce locally.

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
