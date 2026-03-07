# Vesper Repository Guidance

## Source Of Truth

- Treat `docs/specs/design-v0.md` and `docs/specs/firecracker-feature-parity.md` as the authoritative design and parity references.
- Do not treat closed issues, prior eval comments, or route reachability scaffolding as proof of Firecracker parity.
- If code, specs, and tracker state disagree, update the tracker to match the code reality.

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
3. Implement code and regression tests together.
4. Run the narrowest meaningful local verification first, then broader regression coverage before push.
5. Commit each completed unit separately with an issue-linked commit message where applicable.
6. Push after each unit so GitHub Actions and required checks run against the actual commit.
7. Monitor checks to completion; do not assume local green means remote green.
8. Update the linked GitHub issue with the change summary, commit SHA, local verification, CI status, and any remaining gaps.
9. Close issues only when acceptance criteria and checklist items are satisfied in code and tests.

## Tracker Update Protocol

- Every parity unit must leave a tracker artifact behind: issue comment, issue state change, checklist update, or all three.
- Post the first tracker update after push and after the first CI/check signal is available.
- Include both local verification and remote verification in the tracker update.
- If CI is still pending, mark the tracker update as pending and follow up with the final result.
- If CI fails, record the failing workflow/check, the failing commit SHA, and the remediation plan before moving the issue forward.
- Do not mark checklist items complete from local testing alone if branch checks are still red or pending.

## Verification Commands

Use the commands that match the unit being changed:

```bash
make build
make test
make contract-check
make contract-test
make validate-entitlements
cargo test
cargo run --bin baselinectl -- --mode validate
cargo run --bin baselinectl -- --mode generate
cargo run --bin compatexctl -- --registry docs/specs/compat-exceptions.yaml
```

## Per-Unit Verification Checklist

Before pushing:

- Run the narrowest tests that cover the touched files and behavior.
- Run at least one broader regression command if the unit changes shared library code or API semantics.
- Confirm `git status` only contains the intended files for the current unit.

After pushing:

- Check GitHub Actions or required checks for the pushed SHA.
- Record the initial check state in the linked issue or tracker comment.
- Follow checks through completion and post the final success or failure result.
- If parity behavior changed, update the relevant tracker or checklist status to match the verified outcome.

## Baseline Requirements

- Every parity claim must reference the pinned Firecracker baseline revision in `docs/specs/design-v0.baselines.lock.yaml`.
- Baseline metadata must include backend, host OS, host CPU, timestamp, and git SHA.
- Any behavior deviation before parity GA must be tracked in `docs/specs/compat-exceptions.yaml`.

## Architectural Rules

1. `vesperd` is the single external Firecracker-compatible API endpoint.
2. Raw backend sockets are not part of the GA surface.
3. State-machine guards must distinguish preboot, running, and paused behavior where applicable.
4. Error envelopes must remain deterministic and include the pinned baseline revision.
5. `501 Not Implemented` is not parity for pinned Firecracker routes.

## Rust Crate

- Crate name: `vesper`
- Edition: `2024`
