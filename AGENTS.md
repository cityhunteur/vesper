# AGENTS.md

Humans define intent and constraints. Agents execute within an observable, verifiable environment.
This file is a map, not a manual: read it first, then open only the documents your task needs.

## Project overview

Vesper is a Rust crate that exposes a Firecracker-compatible control plane on macOS. `vesperd` serves the pinned
Firecracker OpenAPI routes over a Unix socket with deterministic, baseline-stamped error envelopes; `vesper-proxy`
forwards sockets; `baselinectl`, `compatexctl`, `contractgen` and `systemvmctl` keep parity governance and System VM
artifacts honest. Guest microVM execution (Backend A/B) is designed but not yet implemented here.

## Repository map

| Path | Contents |
|---|---|
| [ARCHITECTURE.md](ARCHITECTURE.md) | Layers, dependency rules, main flows, invariants and how each is enforced |
| [src/](src/README.md) | Library crate: `api`, `api_matrix`, `baseline`, `compat`, `platform`, `proxy`, `spec`, `systemvm` |
| [src/bin/](src/bin/README.md) | The six Rust binaries; [launcher/](launcher/README.md) holds `vesper-vmlauncher`, the Swift System VM launcher |
| [tests/](docs/TESTING.md) | Contract and golden integration suites |
| [scripts/](scripts/README.md) | CI, macOS dev loop, System VM wrappers, harness sensors |
| [docs/](docs/README.md) | Knowledge base and system of record; start at `docs/README.md` |
| [docs/specs/](docs/specs/index.md) | Normative design, parity matrix, baseline lock, compat registry |
| [harness/](harness/README.md) | Knowledge-base checker and recurring agent prompts |
| [WORKFLOW.md](WORKFLOW.md) | Symphony issue-execution workflow and `.symphony/templates/` |
| `third_party/firecracker/` | Vendored Firecracker OpenAPI (pinned revision in the baseline lock) |

## Where to look

| When you are… | Read |
|---|---|
| Orienting in the codebase | [ARCHITECTURE.md](ARCHITECTURE.md), the EGM contract README in each `src/` and `scripts/` directory |
| Changing API or parity behaviour | [design-v0.md](docs/specs/design-v0.md), [parity matrix](docs/specs/firecracker-feature-parity.md), [product specs](docs/product-specs/index.md) |
| Touching baseline, compat or generated matrices | [baseline governance](docs/process/baseline-governance.md), [compat exceptions](docs/process/compat-exceptions.md) |
| Touching the proxy or System VM | [systemvm-bringup.md](docs/process/systemvm-bringup.md), [ADR 0001](docs/design-docs/adr/0001-systemvm-launcher-swift-helper.md), [RELIABILITY.md](docs/RELIABILITY.md), [SECURITY.md](docs/SECURITY.md) |
| Making or questioning a design decision | [Core beliefs](docs/design-docs/core-beliefs.md), [design docs](docs/design-docs/index.md), [ADRs](docs/design-docs/adr/index.md) |
| Starting multi-step or risky work | [PLANS.md](docs/PLANS.md), [active plans](docs/exec-plans/active/index.md) |
| Building and running locally | [DEVELOPMENT.md](docs/DEVELOPMENT.md) |
| Writing or running tests | [TESTING.md](docs/TESTING.md) |
| Diagnosing a failure | [OBSERVABILITY.md](docs/OBSERVABILITY.md), [RELIABILITY.md](docs/RELIABILITY.md) |
| Touching entitlements, sockets or dependencies | [SECURITY.md](docs/SECURITY.md) |
| Looking for cleanup work or known gaps | [Tech debt](docs/exec-plans/tech-debt-tracker.md), [quality score](docs/QUALITY_SCORE.md) |
| Executing a tracked issue end to end | [harness-engineering.md](docs/process/harness-engineering.md), [milestone log](docs/progress/symphony-milestones.md) |

## Commands

All verified on 2026-11-05; detail in [DEVELOPMENT.md](docs/DEVELOPMENT.md) and [TESTING.md](docs/TESTING.md).

```sh
make build                                  # cargo build --bins
cargo test --test api_contract              # one suite; cargo test <name> for one test
cargo test                                  # full suite
make contract-check && make contract-test   # golden route contract + contract suites
make surface-matrix-check && make behavior-matrix-check
cargo run --bin baselinectl -- --mode validate --metadata artifacts/parity-metadata.json   # after --mode generate (DEVELOPMENT.md)
cargo run --bin compatexctl -- --registry docs/specs/compat-exceptions.yaml
make validate-entitlements
python3 harness/check_docs.py               # knowledge-base checks
python3 scripts/harness/check_structure.py --root .
```

No formatter or linter runs in CI; see tech debt TD-006.

## Invariants

These rules are not negotiable. Each one says how it is enforced; detail is in [ARCHITECTURE.md](ARCHITECTURE.md#invariants).

1. `vesperd` is the only public control-plane endpoint; raw backend sockets are never part of the GA surface. *Enforced by: review, SECURITY.md.*
2. Intra-crate dependencies follow the layer table. *Enforced by: `scripts/harness/check_structure.py` in CI.*
3. Every error is a deterministic `ErrorEnvelope` carrying the pinned baseline revision. *Enforced by: `tests/api_contract.rs`.*
4. The golden route list and generated matrices match the pinned swagger. *Enforced by: `make contract-check`, `make surface-matrix-check`, `make behavior-matrix-check`, CI.*
5. A reachable route or a `501` is not parity; parity claims cite the baseline lock revision. *Enforced by: review against the parity matrix; compat drift tracked in `docs/specs/compat-exceptions.yaml`.*
6. Runtime-sensitive changes need runtime artifacts, not just unit tests. *Enforced by: verification bundles under `.symphony/runs/`, council review.*
7. Documentation that contradicts the code is a bug. Fix one or the other in the same change. *Enforced by: `harness/check_docs.py`, doc-gardening.*

## How to work here

- **One smallest verifiable unit per change**, tied to its GitHub issue before any status claim.
- **Plan in proportion.** Multi-slice or runtime-sensitive work gets an ExecPlan per [PLANS.md](docs/PLANS.md).
- **Check targeted first, then broadly**, then collect artifacts with `scripts/harness/collect_verification_bundle.sh`.
- **Commit and push each completed unit** with an issue-linked message, watch CI to completion, then update the issue
  with commit SHA, local and remote verification, and remaining gaps. Local green is not remote green.
- **Code and durable artifacts beat tracker state.** If Linear, GitHub or docs disagree with the code, restore truth first.
- **Keep knowledge in the repo.** Decisions become ADRs, designs, specs or plans. Chat and memory do not count.
- **Don't guess.** Mark gaps **Unknown** with a way to resolve them, and log missing tools or guardrails in the tech-debt tracker.
- **Don't grow `src/api.rs`** when a modular extraction under `src/api/` is practical.

## Ask a human first

- Releases, packaging changes, or anything that widens host-visible sockets or entitlements.
- Destructive or irreversible operations: force-pushes, history rewrites, deleting runtime evidence.
- New runtime dependencies or licence changes.
- Closing or advancing an issue before its acceptance criteria are met in code, tests and green CI.

## Definition of done

1. Behaviour matches the spec, plan or issue, shown with evidence (test output, artifacts, logs).
2. Targeted and relevant broader checks pass locally, CI is green for the pushed SHA, and the issue records both.
3. Owning docs are updated and `python3 harness/check_docs.py` passes.
4. New debt, unknowns and follow-ups are recorded in the tech-debt tracker or the ExecPlan.
