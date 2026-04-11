# Vesper Agent Map

This repository is the system of record for Vesper work. Use this file as a map, not as the full manual.

## Read in this order

1. [`docs/index.md`](docs/index.md)
2. [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
3. [`docs/process/harness-engineering.md`](docs/process/harness-engineering.md)
4. Task-specific sources:
   - API / parity: [`docs/specs/design-v0.md`](docs/specs/design-v0.md), [`docs/specs/firecracker-feature-parity.md`](docs/specs/firecracker-feature-parity.md)
   - baseline / compat: [`docs/process/baseline-governance.md`](docs/process/baseline-governance.md), [`docs/process/compat-exceptions.md`](docs/process/compat-exceptions.md)
   - system VM / proxy: [`docs/process/systemvm-bringup.md`](docs/process/systemvm-bringup.md), [`docs/RELIABILITY.md`](docs/RELIABILITY.md), [`docs/SECURITY.md`](docs/SECURITY.md)
   - planning / execution: [`docs/PLANS.md`](docs/PLANS.md), [`docs/progress/symphony-milestones.md`](docs/progress/symphony-milestones.md)

## Golden rules

- Code and durable artifacts beat tracker status.
- If Linear or GitHub disagree with the code, restore truth before advancing state.
- A reachable route is not parity.
- `501 Not Implemented` is not parity.
- One issue should map to one smallest verifiable parity or harness unit.
- Runtime-sensitive claims require runtime evidence.
- If the harness cannot prove the work, improve the harness before claiming completion.
- A completed slice is not complete while it exists only as a local dirty diff;
  accepted work must be captured in an issue-linked commit before review-state
  promotion.
- Prefer boring, inspectable, repo-legible solutions over opaque abstractions.
- Encode repeated review feedback into docs, scripts, or templates.
- Keep `AGENTS.md` short; put deep guidance in `docs/`.

## Working files by concern

- Control plane and state machine:
  - `src/api.rs`
  - `tests/api_contract.rs`
  - `tests/contract_golden.rs`
- Governance and parity metadata:
  - `src/baseline.rs`
  - `src/compat.rs`
  - `src/spec.rs`
  - `docs/specs/design-v0.baselines.lock.yaml`
  - `docs/specs/compat-exceptions.yaml`
- Runtime and transport:
  - `src/proxy.rs`
  - `src/systemvm.rs`
  - `scripts/systemvm/*`
- Repo harness:
  - `WORKFLOW.md`
  - `docs/ARCHITECTURE.md`
  - `docs/QUALITY_SCORE.md`
  - `scripts/harness/*`
  - `.symphony/templates/*`

## Standard commands

```bash
make contract-check
make contract-test
make validate-entitlements
cargo test
cargo run --bin baselinectl -- --mode validate
cargo run --bin compatexctl -- --registry docs/specs/compat-exceptions.yaml
python3 scripts/harness/check_docs.py --root .
python3 scripts/harness/check_structure.py --root .
python3 scripts/harness/compute_quality_score.py --root .
python3 scripts/harness/janitor_scan.py --root .
scripts/harness/init_issue_run.sh ISSUE-ID
scripts/harness/collect_verification_bundle.sh ISSUE-ID
```

## Issue execution

1. Map the tracker ticket to the GitHub issue or PR that carries the evidence.
2. Restore issue truth before coding and initialize `.symphony/runs/<ticket>/`.
3. Implement one small unit plus its proof.
4. Run narrow sensors, then a broader proof, then collect artifacts.
5. Record keep / discard / crash in the verification bundle.
6. Run the LLM council.
7. Create an issue-linked commit for every accepted slice before any non-active
   tracker transition.
8. Update GitHub issue or PR, the milestone log, and then the tracker state.

## What not to do

- Do not advance tracker state from labels or merged code alone.
- Do not rely on Slack, memory, or chat as hidden requirements.
- Do not ship new parity claims without updating tests or runtime artifacts.
- Do not stage unrelated worktree changes.
- Do not dump every new behavior into `src/api.rs` if a modular extraction is practical.
