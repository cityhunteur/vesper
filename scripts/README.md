# scripts

## Overview

Operational helper scripts for the `vesper` crate. The directory holds one
direct script, `update-issue-checklists.sh`, which appends a fixed
`## Parity Eval Checklist` section to the bodies of GitHub issues #1 through
#40 via the `gh` CLI, plus four subdirectories wrapped by Makefile targets:
CI entitlement linting (`ci`), harness sensors and issue-run bundles
(`harness`), the macOS local dev loop (`macos`), and System VM image, launch
config, and boot smoke wrappers (`systemvm`). None of these scripts are part
of the GA API surface; they support development, CI, and tracker hygiene.

## Interface

### `update-issue-checklists.sh`

- `append_checklist <issue_num> <checklist>`: shell function that fetches the issue body, checks for the marker, and rewrites the body with the checklist appended when the marker is absent. Called 40 times at top level with hard-coded issue numbers and quoted heredoc checklist text.
- `REPO` (env, default `cityhunteur/vesper`): the only runtime configuration.
- `MARKER="## Parity Eval Checklist"`: shell variable (not overridable) used both as the idempotency `grep -qF` pattern and as the heading inserted into the body.
- No CLI flags, subcommands, or positional arguments are parsed; `$@` is never read. There is no `--dry-run` or help option.
- External commands: `gh issue view "$issue_num" --repo "$REPO" --json body --jq '.body // ""'` to read, `gh issue edit "$issue_num" --repo "$REPO" --body "$new_body"` to write, `grep -qF "$MARKER"` for the marker check.
- Hard-coded issue order: Phase 0: 2, 8, 9, 10, 11, 37, 38, 1. Phase 1: 12, 13, 14, 15, 16, 3. Phase 2: 17, 18, 19, 20, 21, 4. Phase 3: 22, 23, 24, 25, 26, 5. Phase 4: 27, 28, 29, 30, 31, 39, 6. Phase 5: 32, 33, 34, 35, 36, 40, 7.
- stdout: per issue `Processing issue #N...` followed by `  Skipped: checklist already exists` or `  Done: checklist appended`; an empty line, then the final line `All issues processed.`
- Exit code: `0` on completion, otherwise the non-zero status of the first failing `gh` or `grep` command under `set -euo pipefail`. No explicit `exit` statements.

### Subdirectories

- [ci](./ci/README.md): `validate-entitlements.sh` lints the two macOS entitlement plists (`make validate-entitlements`).
- [harness](./harness/README.md): four Python sensors (`check_docs.py`, `check_structure.py`, `compute_quality_score.py`, `janitor_scan.py`) and two Bash bundle helpers (`init_issue_run.sh`, `collect_verification_bundle.sh`).
- [macos](./macos/README.md): `dev-up.sh` / `dev-down.sh` build and run `vesperd` and `vesper-proxy` under `run/` (`make dev-up`, `make dev-down`).
- [systemvm](./systemvm/README.md): `fetch-image.sh`, `build-image.sh`, `generate-launch-config.sh`, `boot-smoke.sh` wrap `cargo run --bin systemvmctl` (`make systemvm-fetch`, `make systemvm-build`, `make systemvm-launch-config`, `make systemvm-boot-smoke`).

## Invariants

- `update-issue-checklists.sh` uses `set -euo pipefail`: a `gh` failure on any issue stops processing of all subsequent issues.
- Idempotency: an issue body is edited only when `grep -qF "## Parity Eval Checklist"` does not match the existing body, so re-running is a no-op for already-updated issues.
- Appended section format is fixed: existing body, blank line, `---`, blank line, `## Parity Eval Checklist`, blank line, a bold `**Phase N — Title (#issue)**` line, then `- [ ]` task-list items.
- `--jq '.body // ""'` coerces a null issue body to the empty string so the append still proceeds.
- Requires an authenticated `gh` CLI with write access to `$REPO`; no explicit tool-presence check is performed.
- Every invocation performs live edits for any issue lacking the marker; there is no dry-run mode.
- Each subdirectory script also uses `set -euo pipefail`. `ci`, `macos`, and `systemvm` scripts resolve `ROOT_DIR` from their own location (the `systemvm` wrappers still run `cargo` in the caller's cwd); the `harness` Bash helpers default `REPO_ROOT` to the caller's cwd. See the child contracts.

## Side Effects

- `update-issue-checklists.sh` mutates the `body` field of up to 40 GitHub issues (#1 to #40) in `$REPO` via `gh issue edit`, replacing the full body with the original plus the appended section. Comments, labels, state, and titles are unchanged.
- `update-issue-checklists.sh` makes 40 network reads via `gh issue view` and up to 40 network writes via `gh issue edit`. It writes no local files and creates no temp files.
- Subdirectory scripts write under `run/`, `build/systemvm/`, `target/`, and `.symphony/runs/`, spawn `cargo`, `plutil`, `python3`, `git`, `vesperd`, and `vesper-proxy`, and open Unix sockets under `run/`; see each child contract for the exact paths.

## Verification

- Run: `cargo test`
- Subdirectory targets: `make validate-entitlements`, `make dev-up` / `make dev-down`, `make systemvm-fetch`, `make systemvm-build`, `make systemvm-launch-config`, `make systemvm-boot-smoke`
- `update-issue-checklists.sh` has no automated test and performs live GitHub edits; verify with `bash -n scripts/update-issue-checklists.sh` for syntax only.
