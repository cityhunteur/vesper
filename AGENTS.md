# Vesper Agent Workflow

## Source Of Truth

- Treat [docs/specs/design-v0.md](/Volumes/Workspace/github.com/cityhunteur/vesper/docs/specs/design-v0.md) and [docs/specs/firecracker-feature-parity.md](/Volumes/Workspace/github.com/cityhunteur/vesper/docs/specs/firecracker-feature-parity.md) as the authoritative parity and design references.
- Do not mark parity complete based on issue closure, prior eval comments, or scaffolded route reachability alone.
- If local code and GitHub issue state disagree, trust the code and update the issue status/comments to match reality.

## Repository Layout

```text
src/
  api.rs         Firecracker-compatible API surface, state machine, deterministic errors
  baseline.rs    Baseline lock parsing and parity metadata validation
  buildinfo.rs   Build metadata
  compat.rs      Compatibility exception registry and lifecycle policy checks
  platform.rs    Platform capability and entitlement validation
  proxy.rs       Unix/TCP/vsock forwarding
  spec.rs        Firecracker OpenAPI loading
  systemvm.rs    System VM manifests, launch config, boot smoke helpers
  bin/           vesperd, vesper-proxy, baselinectl, compatexctl, contractgen, systemvmctl
tests/           Contract and regression suites
docs/            Specs, process docs, devlog
scripts/         CI, macOS, and System VM helper scripts
third_party/     Vendored Firecracker OpenAPI
```

## Required Workflow

1. Pick the smallest parity unit that can be completed and verified independently.
2. Tie that unit to the relevant GitHub issue before changing status claims.
3. Implement the code and the regression tests together.
4. Run the narrowest meaningful local checks first, then the broader suite before push.
5. Commit each completed unit separately with an issue-linked message.
6. Push after each unit so GitHub Actions and branch checks run against the actual commit.
7. Monitor CI/checks to completion; do not assume success from local green runs.
8. Update the linked GitHub issue with:
   - what changed
   - commit SHA
   - local verification commands
   - CI/check status
   - remaining gaps, if any
9. Close an issue only when its acceptance criteria and checklist items are actually satisfied in code and tests.

## Issue And Status Rules

- Reopen issues if they were closed on scaffolding or partial behavior and the acceptance criteria are still unmet.
- Do not collapse multiple parity units into one issue update if separate commits/issues exist.
- Epic issues stay open until every child issue is complete and the epic-level verification passes.
- Compat exceptions must be explicit; undocumented drift is not an acceptable temporary state.

## Git Rules

- Never commit unrelated user changes already present in the worktree.
- If the worktree is dirty, stage only the files for the current unit.
- Use commit messages that map back to the roadmap issue, for example:
  - `feat(api): implement machine config + readback parity (#2)`
  - `test(api): harden lifecycle parity regressions (#11)`

## Verification Commands

Use the commands that match the unit being changed:

```bash
make contract-check
make contract-test
make validate-entitlements
cargo test
cargo run --bin baselinectl -- --mode validate
cargo run --bin baselinectl -- --mode generate
cargo run --bin compatexctl -- --registry docs/specs/compat-exceptions.yaml
```

## Phase 0 Expectations

- Every route in `third_party/firecracker/firecracker.yaml` must be handled by `vesperd`.
- `501 Not Implemented` is not parity.
- State-dependent routes must enforce Firecracker-style preboot/runtime rules.
- Error responses must stay deterministic and include the pinned baseline revision.
- Contract tests must fail if a route regresses to reachability-only scaffolding.
