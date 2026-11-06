---
status: verified
owner: cityhunteur
last_reviewed: 2026-11-06
---

# Tech-debt tracker

This is the single list of known debt: code that breaks the documented principles, missing tests, missing
observability, unenforced invariants, documentation gaps, and any tool or guardrail an agent needed but did not have.
Small items get fixed by recurring [entropy sweeps](../../harness/prompts/entropy-sweep.md). Large items get an
ExecPlan. Add new debt only when it is real and durable; remove an item only when code and evidence justify it.

| ID | Item | Area | Impact | Evidence | Proposed fix | Priority | Status |
|---|---|---|---|---|---|---|---|
| TD-001 | `src/api.rs` is above the 2500-line warning budget; `errors` and `vm_config` route families are still inline | architecture | Agents pattern-match on a hotspot; structure sensor warns on every run | `python3 scripts/harness/check_structure.py --root .`; WOR-105 extracted four families | Extract `src/api/errors.rs` and `src/api/vm_config.rs` with their tests | P1 | open |
| TD-002 | Optimistic issue-promotion workflows (`eval-on-review.yml`, `promote-implemented-issues.yml`) | process | Could promote issue state ahead of evidence | Files no longer exist in `.github/workflows/`; `janitor_scan.py` keeps a guard against their return | Record the retirement as an ADR (candidate in `design-docs/adr/index.md`); keep the guard | P3 | open |
| TD-003 | "A reachable route is not parity" has no mechanical check; behavior-level parity tests are thinner than route reachability | parity | Parity can be over-claimed | `janitor_scan.py` only warns on missing explicit handlers (route sensors scan `src/api.rs` only; fixed on 2026-11-06 to recognise the `route_match` dispatch form); `docs/generated/firecracker-api-surface-stub-matrix.json` still lists `501` stub statuses (unreachable today, all 37 routes have handlers) | Grow `tests/api_contract.rs` per route family; fail CI when a pinned route regresses to a stub | P1 | open |
| TD-004 | vsock runtime proof is a host Unix-socket bridge, not a real vsock transport | runtime | Phase 1 control channel unproven | `src/proxy.rs` `CONNECT` preface; GitHub #9, #13 | Implement and capture end-to-end vsock evidence under `run/<ISSUE>/proxy/` | P2 | open |
| TD-005 | System VM support is manifest and boot-smoke scaffolding only | runtime | No real VM lifecycle evidence | `src/systemvm.rs` spawns an arbitrary command; `assets/systemvm/*` are 47-byte placeholders | Real image inputs and a boot path; see TD-010 | P2 | open |
| TD-006 | No `cargo fmt --check` or `cargo clippy` in the Makefile or CI | tooling | Style and lint drift goes unnoticed | `Makefile`, `.github/workflows/*.yml` contain neither | Add `make lint` and a CI step | P2 | open |
| TD-007 | No structured logging, metrics emission, tracing or request IDs in any binary | observability | Failures are diagnosed from exit codes and artifacts only | [OBSERVABILITY.md](../OBSERVABILITY.md) gaps; no `log`/`tracing` crate in `Cargo.toml` | Add `tracing` with a per-request span in `handle_all` and per-connection logs in the proxy | P2 | open |
| TD-008 | "Parse at the boundary" and "`vesperd` is the only public endpoint" are enforced by review only | architecture | Invariants 1 and 7 in [ARCHITECTURE.md](../../ARCHITECTURE.md#invariants) can erode silently | No lint or structural test | Add a structural test that only `src/bin/vesperd.rs` binds an API listener; a lint for untyped `serde_json::Value` probing outside MMDS | P3 | open |
| TD-009 | Rust toolchain is not pinned | tooling | Local and CI builds can diverge | No `rust-toolchain.toml`; CI uses `dtolnay/rust-toolchain@stable` | Add `rust-toolchain.toml` with the current stable | P3 | open |
| TD-010 | System VM images have no documented provenance; on macOS `make validate-entitlements` checks plist syntax only | packaging, runtime | Image questions reach reviewers late; key regressions are caught only on the Linux CI path | `assets/systemvm/README.md`; `scripts/ci/validate-entitlements.sh` (`rust-tests.yml` runs it on Linux since 2026-11-06) | Document where real kernel and rootfs images come from; make the key check run on macOS too | P2 | open |
| TD-011 | `collect_verification_bundle.sh` expects `run/<ISSUE>/api/requests.log` and `responses.log` that nothing writes, and reads `run/<ISSUE>/proxy/proxy.log` while `make dev-up` writes `run/vesper-proxy.log` | harness | Bundles silently lack API and dev-loop proxy evidence | [scripts/harness/README.md](../../scripts/harness/README.md), `scripts/macos/dev-up.sh` | Add request/response logging to `vesperd` (with TD-007), align the proxy log path, or drop the expectations | P3 | open |
| TD-012 | Two docs checkers coexist: `harness/check_docs.py` (front matter, indexes, reachability) and `scripts/harness/check_docs.py` (required files, links) | docs tooling | Duplicate rules to maintain | `.github/workflows/docs.yml` and `harness-docs.yml` | Fold the required-files and absolute-path checks into `harness/config.toml` and retire the old script, or vice versa | P3 | open |
| TD-013 | Six architecture decisions have no ADR | docs | Reasoning cannot be recovered by agents | [ADR candidates](../design-docs/adr/index.md#candidates) | Owners write ADRs for the candidates they remember | P3 | open |
| TD-014 | `docs/references/*` historical notes still describe the pre-2026-11 layout | docs | Stale paths can mislead | status `deprecated` in their front matter | Rewrite or archive after one review cycle | P3 | open |
| TD-015 | `src/api/tests.rs` (4552 lines) is outside every line budget and the structure allowlist | architecture | The largest file in the crate is unmeasured | `scripts/harness/check_structure.py` `RULES`, `janitor_scan.py` globs `src/*.rs` | Split the handler tests per route family under `src/api/`, then add them to the budgets | P3 | open |
| TD-016 | Compat exception expiry is checked only when the registry or its code changes; no scheduled run | governance | An exception can expire silently | `.github/workflows/compat-exceptions.yml` has no `schedule` trigger | Add a weekly schedule, or run `compatexctl` in the garbage-collection workflow | P3 | open |

**Priority:** P0 = blocks correct agent work or is a correctness or security risk; P1 = repeatedly slows work;
P2 = worth fixing in a sweep; P3 = nice to have.
**Status:** open, planned (link the ExecPlan), done (keep it for one review cycle, then remove it), or won't fix (give the reason).
