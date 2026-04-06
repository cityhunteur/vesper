# Bundle test plan

## 1. Repo overlay smoke test

Apply `repo-overlay/` to a Vesper checkout and run:

```bash
python3 scripts/harness/check_docs.py --root .
python3 scripts/harness/check_structure.py --root .
python3 scripts/harness/compute_quality_score.py --root . --markdown-out artifacts/quality-score.md --json-out artifacts/quality-score.json
python3 scripts/harness/janitor_scan.py --root . --fail-on never --markdown-out artifacts/janitor-scan.md --json-out artifacts/janitor-scan.json
```

Expected:
- docs check passes,
- structure check passes with warnings but not errors on current debt,
- quality score emits markdown and JSON,
- janitor scan emits a drift report.

## 2. Symphony workflow parse test

Install `assets/WORKFLOW.md` as `WORKFLOW.md` and confirm Symphony parses it without unsupported fields.

## 3. Verification bundle smoke test

Run:

```bash
scripts/harness/init_issue_run.sh TEST-123
scripts/harness/collect_verification_bundle.sh TEST-123
```

Expected:
- `.symphony/runs/TEST-123/current/verification.yaml` exists,
- `.symphony/runs/TEST-123/current/notes.md` exists,
- git metadata files are created.

## 4. Workflow CI smoke test

Turn on these workflows:
- `harness-docs.yml`
- `harness-structure.yml`
- `harness-quality-score.yml`
- `harness-garbage-collection.yml`

Expected:
- docs and structure workflows hard-fail only on integrity errors,
- quality and janitor workflows emit artifacts reliably.

## 5. Behavioral dry run

Take one small Vesper issue and drive it through:
- `Todo`
- `In Progress`
- `Verification`
- `Council`
- `Human Review`

Expected:
- keep/discard result is recorded,
- milestone log updates,
- GitHub issue or PR is updated with the same evidence,
- tracker state does not outrun the verification bundle.

## 6. Failure path tests

Force:
- a broken docs link,
- a structural boundary violation,
- a council reject,
- a missing runtime artifact for a runtime-sensitive issue.

Expected:
- docs and structure sensors fail appropriately,
- the council blocks promotion,
- the issue stays in `Rework` or `Verification`,
- milestone and verification artifacts explain why.
