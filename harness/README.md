# harness/

This directory holds the tooling that keeps the knowledge base (`AGENTS.md`, `ARCHITECTURE.md`, `docs/`) honest.
Everything in it is Python 3.11+ standard library only.

| File | Purpose |
|---|---|
| `check_docs.py` | Knowledge-base linter, run locally and in CI |
| `config.toml` | Checker configuration: scope, required files, front matter, ExecPlan sections |
| `install.py` | Copies the template into another repository without overwriting anything |
| `prompts/bootstrap.md` | Agent prompt that fills the template for a specific repository |
| `prompts/doc-gardening.md` | Recurring agent prompt: make documents match the code |
| `prompts/entropy-sweep.md` | Recurring agent prompt: make code match the documented principles |
| `tests/` | Tests for the checker and the installer |

## check_docs.py

```sh
python3 harness/check_docs.py            # fails on structural errors
python3 harness/check_docs.py --strict   # also fails on TODO(harness) markers and `status: template`
```

Findings are printed as `path:line: severity: message (fix: …)`. The fix hint lets an agent remediate the problem
without having to look up the rule. The exit code is 1 if there are any errors.

| Check | Severity |
|---|---|
| Required files exist (`[required]`) | error |
| `AGENTS.md` within `max_lines`; above `target_lines` | error / warning |
| Relative links and `#anchors` resolve, including links to code | error |
| Every in-scope document is reachable from `AGENTS.md` | error |
| Every `docs/` directory has an `index.md` or `README.md` that links each document and subdirectory in it | error |
| Front matter has `status`, `owner` and `last_reviewed`; status is known; date is valid and not in the future | error |
| `last_reviewed` older than `stale_after_days` | warning |
| ExecPlans contain the required `##` sections; completed plans have no unchecked Progress items | error |
| Unfilled template content | warning; error with `--strict` |

`TEMPLATE.md` files are exempt from the front-matter and template-content checks. Content inside code fences and
inline code is ignored, so documents can show examples of links and markers.

To change the rules for a project, edit `config.toml`. If you add a required ExecPlan section there, update the
skeleton in `docs/PLANS.md` to match.

## install.py

```sh
python3 harness/install.py /path/to/repo --dry-run   # preview
python3 harness/install.py /path/to/repo             # copy missing files only
```

The installer copies `AGENTS.md`, `CLAUDE.md`, `ARCHITECTURE.md`, `docs/`, `harness/` and
`.github/workflows/docs.yml`. Files that already exist are skipped and listed. Reconcile them with the bootstrap
prompt, which merges the template's intent into the existing files instead of replacing them.

## Tests

```sh
python3 -m unittest discover -s harness/tests
```

The suite also asserts that the shipped template passes the non-strict check, so template edits cannot break the
layout.
