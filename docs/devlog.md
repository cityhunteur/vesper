---
status: verified
owner: cityhunteur
last_reviewed: 2026-11-05
---

# Vesper Development Log

## 2026-03-01

### Rust Rewrite
- Rewrote the implementation layer from Go to Rust (`Cargo` project with `src/` modules and `src/bin/` commands).
- Replaced binaries with Rust equivalents:
  - `vesperd`
  - `vesper-proxy`
  - `baselinectl`
  - `compatexctl`
  - `contractgen`
  - `systemvmctl`
- Ported core functionality:
  - Firecracker route loading/parity contract handling
  - API state machine + deterministic error semantics
  - Baseline lock governance and metadata generation/validation
  - Compatibility exception registry lifecycle checks
  - Proxy forwarding skeleton and connection handling
  - System VM manifest/launch/boot-smoke scaffolding
- Migrated CI workflows to Rust/Cargo commands.
- Migrated local scripts and `Makefile` targets to Cargo-based tooling.
- Validation: `cargo test` passes.

### Roadmap Eval Orchestration
- Progressed all roadmap issues through project status workflow to trigger eval execution (`Backlog -> Ready -> In progress -> In review`).
- Ran eval batch via `eval:ready` labels; reconciled project board state with passing items moved to `Done`.
- Captured eval failures for Phase 0 platform issues (#9, #10) and opened follow-up bug #41.
- Updated eval workflow automation to:
  - use project token fallback for project operations;
  - create/link bug issues automatically on eval failure;
  - move created bug items to `Ready`.
- Added cross-platform entitlement validation script so Linux CI can validate entitlements without `plutil`.
