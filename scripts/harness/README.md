# scripts/harness

## Overview

Harness "sensor" scripts for the Vesper engineering workflow. Four Python
checkers read the repository under `--root` and emit JSON and Markdown
reports plus exit codes: `check_docs.py` (required docs and link integrity),
`check_structure.py` (module dependency allowlists, line budgets, route
coverage), `compute_quality_score.py` (a 100-point score across five
dimensions), and `janitor_scan.py` (drift scan with configurable failure
threshold). Two Bash helpers, `init_issue_run.sh` and
`collect_verification_bundle.sh`, create and populate per-issue run bundles
under `.symphony/runs/<ISSUE-ID>/current` and `run/<ISSUE-ID>`. The checkers
spawn no subprocesses and make no network calls.

## Interface

### Python checkers (shared CLI shape)

- `--root` (default `.`): repository root to scan.
- `--json-out` (default empty): optional JSON report path.
- `--markdown-out` (default empty): optional Markdown report path.
- Each prints `ERROR: ...` and `WARNING: ...` lines (or raw `error:`/`warning:` messages for `janitor_scan.py`) to stdout.

### `check_docs.py`

- `REQUIRED_FILES`: 17 repo-relative files that must exist (`AGENTS.md`, `WORKFLOW.md`, `docs/index.md`, `docs/ARCHITECTURE.md`, `docs/PLANS.md`, `docs/QUALITY_SCORE.md`, `docs/RELIABILITY.md`, `docs/SECURITY.md`, `docs/design/index.md`, `docs/design/core-beliefs.md`, `docs/plans/active/README.md`, `docs/plans/completed/README.md`, `docs/plans/tech-debt-tracker.md`, `docs/generated/README.md`, `docs/process/harness-engineering.md`, `docs/progress/symphony-milestones.md`, `docs/references/README.md`).
- `MARKDOWN_GLOBS`: `AGENTS.md`, `WORKFLOW.md`, `docs/**/*.md`.
- `resolve_link(base, target, root)`: accepts empty, `#` anchor, `://`, and `mailto:` links; resolves `/`-prefixed links as repo-absolute and relative links against the file's parent; rejects links that escape `root` or do not exist.
- `find_markdown_files(root)`: expands `MARKDOWN_GLOBS`, deduplicates, returns sorted paths.
- `main`: exit `1` if any errors, else `0`. JSON keys: `root`, `status` (`pass`|`fail`), `errors`, `warnings`, `checked_files`. Markdown headed `# Harness Docs Report`.

### `check_structure.py`

- `LOCAL_REF_RE`: `(?:crate|vesper)::([a-z_]+)` detects intra-crate module references.
- `METHOD_PATH_RE`: `\("([A-Z]+)",\s*"([^"]+)"\)\s*=>` detects explicit `(METHOD, path)` match arms in `src/api.rs`.
- `RULES`: allowed local references per file. `src/baseline.rs`, `src/buildinfo.rs`, `src/compat.rs`, `src/platform.rs`, `src/spec.rs`, `src/lib.rs`: none. `src/api.rs`: `api_matrix`, `baseline`, `buildinfo`, `compat`, `platform`, `spec`. `src/api_matrix.rs`: `api`, `baseline`, `spec`. `src/proxy.rs` and `src/systemvm.rs`: `buildinfo`, `platform`.
- `WARN_BUDGETS` / `ERROR_BUDGETS`: `src/api.rs` warn at 2500 and error at 3200 lines; every other `RULES` file warn at 800 and error at 1200 (`DEFAULT_WARN_BUDGET`, `DEFAULT_ERROR_BUDGET`).
- `main`: reads `testdata/contracts/firecracker-routes.golden.json` and `src/api.rs` to compute explicit route coverage; treats `PUT /actions` as handled when either literal `if route.method == "PUT" && route.template == "/actions"` or `if route_match.template.method == "PUT" && route_match.template.template == "/actions"` appears. Exit `1` on errors. JSON keys: `root`, `status`, `errors`, `warnings`, `line_sizes`. Markdown headed `# Harness Structure Report`.

### `compute_quality_score.py`

- `CORE_DOCS` (9 files), `HARNESS_WORKFLOWS` (`.github/workflows/harness-docs.yml`, `harness-structure.yml`, `harness-quality-score.yml`, `harness-garbage-collection.yml`), `HARNESS_SCRIPTS` (the six scripts in this directory), `TEMPLATES` (four files under `.symphony/templates/`).
- `score_fraction(present, total, weight)` -> `(round(weight * present / max(total, 1)), "present/total")`.
- `compute_route_coverage(root)` -> `(handled, total, missing)`; recognises only the `route.method` literal for `PUT /actions`, not the `route_match` variant that `check_structure.py` accepts.
- `grade(score)`: `A` >= 90, `B` >= 80, `C` >= 70, `D` >= 60, else `F`.
- `main`: always exits `0`; prints `quality score: N/100 (G)`. JSON keys: `score`, `grade`, `dimensions` (`knowledge_base` weight 20, `harness_sensors` weight 25, `parity_surface` weight 25, `architecture` weight 15 with `api_lines`, `truthful_delivery` weight 15 with `optimistic_workflows`), `missing_route_behaviors`. Markdown headed `# Harness Quality Score`.

### `janitor_scan.py`

- `--fail-on` (default `errors`, choices `never`, `errors`, `warnings`) in addition to the shared flags.
- `broken_doc_links(root)`: scans `docs/**/*.md` and `AGENTS.md`; `warning:` on any `/Volumes/` absolute path, `error:` on unresolved non-external links.
- `missing_route_behaviors(root)`: `warning:` for each golden route without an explicit match arm in `src/api.rs` (same `PUT /actions` literal as `compute_quality_score.py`).
- `oversized_files(root)`: scans `src/*.rs`; `src/api.rs` over 2500 lines is a `warning:`, any other file over 1200 lines is an `error:`.
- `optimistic_automation(root)`: `warning:` if `.github/workflows/eval-on-review.yml` or `.github/workflows/promote-implemented-issues.yml` exists.
- `classify(messages)` -> `(errors, warnings)` by `error:` / `warning:` prefix.
- `main`: exit `0` when `--fail-on never`; exit `1` when `--fail-on warnings` and any message exists; exit `1` when `--fail-on errors` and any error exists; otherwise `0`. JSON keys: `status`, `fail_on`, `errors`, `warnings`. Markdown headed `# Harness Janitor Scan`.

### `init_issue_run.sh`

- `$1` `ISSUE_ID` (required), `$2` `REPO_ROOT` (default `.`). Exit `2` with usage on stderr when `ISSUE_ID` is empty.
- Invokes `mkdir`, `cp`, `date -u`, `git rev-parse --short HEAD`, `pwd`. Prints `initialized issue run for ...`, `local run dir: ...`, `bundle dir: ...`.
- Generated `env.sh` exports `VESPER_ISSUE_ID`, `VESPER_RUN_DIR=$(pwd)/run/<ISSUE_ID>`, `VESPER_BUNDLE_DIR=$(pwd)/.symphony/runs/<ISSUE_ID>/current`. The script itself consumes no environment variables.

### `collect_verification_bundle.sh`

- `$1` `ISSUE_ID` (required), `$2` `REPO_ROOT` (default `.`). Exit `2` with usage on stderr when `ISSUE_ID` is empty.
- `copy_if_exists <src> <dst>`: copies `src` to `dst` (creating the parent directory) only when `src` is a regular file.
- Invokes `mkdir`, `date -u`, `git rev-parse --short HEAD`, `git branch --show-current`, `git status --short`, `cp`. Prints `collected verification bundle for ... at ...`.

## Invariants

- `init_issue_run.sh` and `collect_verification_bundle.sh` use `set -euo pipefail` and exit `2` without an `ISSUE-ID`.
- `check_docs.py`: all 17 `REQUIRED_FILES` must exist; `AGENTS.md` must contain the string `docs/index.md` (error) and is warned above 160 lines; `docs/index.md` must mention `ARCHITECTURE.md`, `QUALITY_SCORE.md`, `RELIABILITY.md`, `SECURITY.md`, and `PLANS.md`; every Markdown link in the scanned globs must resolve inside the repo root; any `/Volumes/` string is a warning.
- `check_structure.py`: each `RULES` file may reference only its allowlisted `crate::` / `vesper::` modules (error otherwise; `src/lib.rs` is exempt); a missing listed file is a warning; line budgets as above; explicit route coverage below 100% of the golden file is a warning, never an error. Files not in `RULES` (`src/api_behavior_matrix.rs`, `src/api/*.rs`, `src/bin/*.rs`) are checked for neither references nor line budgets.
- `compute_quality_score.py`: dimension weights sum to 100; `architecture` scores 15 when `src/api.rs` is at most 1200 lines, 11 at most 2500, 8 at most 3200, 4 above that, 0 if absent; `truthful_delivery` loses 5 when any optimistic workflow exists; the script never fails.
- `janitor_scan.py`: `src/*.rs` files other than `api.rs` above 1200 lines are errors, `api.rs` above 2500 lines is a warning; `status` is `fail` only when errors exist; pass or fail is controlled by `--fail-on`.
- Both Bash helpers create `verification.yaml` and `notes.md` only when absent (idempotent); `init_issue_run.sh` writes the `results.tsv` header only when the file is absent.

## Side Effects

- All four Python checkers write a report file at `--json-out` and/or `--markdown-out` when given, creating parent directories; otherwise they only read files under `--root`.
- `init_issue_run.sh` creates `.symphony/runs/<ISSUE_ID>/current/{api,proxy,systemvm,council}` and `run/<ISSUE_ID>/{api,proxy,systemvm}` under `REPO_ROOT`; creates `.symphony/runs/<ISSUE_ID>/results.tsv` with header `attempt\tstarted_at\tresult\tnotes` if absent; copies `.symphony/templates/verification-manifest-template.yaml` to `.symphony/runs/<ISSUE_ID>/current/verification.yaml` if absent and the template exists (a missing template is silently skipped); creates `notes.md` (issue, started_at, git_head) if absent; always overwrites `env.sh`.
- `collect_verification_bundle.sh` creates `.symphony/runs/<ISSUE_ID>/current/{api,proxy,systemvm}`; always overwrites `git-info.txt` (issue, collected_at, git_head, git_branch) and `git-status.txt` (empty when `git status` fails, since it runs with `|| true`); creates `verification.yaml` from the template (when it exists) and `notes.md` (issue, collected_at) when absent; copies when present `build/systemvm/image-manifest.json`, `build/systemvm/launch-config.json`, `run/systemvm/boot.log`, `run/systemvm/boot-report.json`, `run/<ISSUE_ID>/systemvm/boot.log` (to `systemvm/boot.issue.log`), `run/<ISSUE_ID>/systemvm/boot-report.json` (to `systemvm/boot-report.issue.json`), `run/<ISSUE_ID>/proxy/proxy.log` then `run/proxy/proxy.log` (both to `proxy/proxy.log`, so the generic log overwrites the issue log when both exist), `run/<ISSUE_ID>/api/requests.log`, `run/<ISSUE_ID>/api/responses.log`.
- Both Bash helpers spawn `git` and `date -u`. No script in this directory calls `gh`, `cargo`, `make`, or the network.

## Verification

- Run: `python3 scripts/harness/check_docs.py --root .`
- Run: `python3 scripts/harness/check_structure.py --root .`
- Run: `python3 scripts/harness/compute_quality_score.py --root .`
- Run: `python3 scripts/harness/janitor_scan.py --root . --fail-on errors`
- Bundle helpers: `bash scripts/harness/init_issue_run.sh <ISSUE-ID>` then `bash scripts/harness/collect_verification_bundle.sh <ISSUE-ID>`
