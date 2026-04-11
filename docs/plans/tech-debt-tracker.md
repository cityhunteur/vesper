# Tech Debt Tracker

Track debt that affects agent legibility, parity truth, or architectural coherence.

## Open debt

### TD-001 — Split `src/api.rs`
- Why: current hotspot reduces navigability and raises the chance of agent-local pattern matching.
- Latest evidence: WOR-20 extracted the generated surface-stub matrix into `src/api_matrix.rs`, but `python3 scripts/harness/check_structure.py --root .` still reports `src/api.rs` over the 3200-line error budget.
- Suggested slices:
  - actions
  - machine config
  - boot source
  - logger / metrics / serial / entropy
  - vm config / errors

### TD-002 — Retire optimistic issue-promotion workflows
- Files:
  - `.github/workflows/eval-on-review.yml`
  - `.github/workflows/promote-implemented-issues.yml`
- Why: they can promote issue state ahead of durable verification evidence.

### TD-003 — Add stronger behavior-level parity tests
- Why: route reachability exists, but many endpoints still need behavior and state-machine proofs.

### TD-004 — Implement real vsock runtime proof path
- Why: proxy target parsing exists, but end-to-end runtime proof remains incomplete.

### TD-005 — Expand System VM lifecycle evidence
- Why: current System VM support is still mostly manifest and boot-smoke scaffolding.

## Usage

- Add new debt only when it is real and durable enough to matter.
- Link issues or milestone entries when the debt is acted on.
- Remove an item only when the code and evidence justify removal.
