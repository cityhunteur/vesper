---
status: deprecated
owner: cityhunteur
last_reviewed: 2026-11-05
---

> Historical note from the 2026-03 harness-engineering bundle. Paths such as `docs/index.md`, `docs/design/` and `docs/plans/` refer to the layout that was replaced on 2026-11-05; see [index.md](index.md).

# Harness engineering adaptation for Vesper

This note explains how the bundle translates OpenAI’s harness-engineering ideas into Vesper-specific files and workflows.

## 1. Humans steer, agents execute

The article frames the engineer’s role as designing environments, specifying intent, and building feedback loops rather than hand-writing code. The bundle implements that by centering `WORKFLOW.md`, repo-local docs, verification templates, and council review instead of issue-label automation. citeturn357410view0turn299543view0

## 2. `AGENTS.md` as a table of contents

OpenAI reports that a single giant `AGENTS.md` file failed because context is scarce, over-guidance becomes noise, stale rules rot quickly, and a blob is hard to verify mechanically. They instead treat `AGENTS.md` as a short table of contents pointing into a structured docs tree. citeturn299543view0

The bundle applies that directly:
- `repo-overlay/AGENTS.md`
- `repo-overlay/docs/index.md`
- `repo-overlay/docs/design/index.md`
- `repo-overlay/docs/PLANS.md`
- `repo-overlay/scripts/harness/check_docs.py`

## 3. Repository knowledge as the system of record

The article says the repo-local docs tree should be the system of record because anything the agent cannot access in-context effectively does not exist. It also emphasizes progressive disclosure over dumping everything into one prompt. citeturn299543view0

The bundle encodes that with:
- `docs/index.md`
- `docs/ARCHITECTURE.md`
- `docs/QUALITY_SCORE.md`
- `docs/RELIABILITY.md`
- `docs/SECURITY.md`
- `docs/plans/*`
- Symphony templates in `.symphony/templates/`

## 4. Make the application legible to the agent

OpenAI made their app bootable per worktree and exposed logs, metrics, and traces so agents could reproduce, validate, and reason about failures directly. citeturn299543view0

Vesper does not have a browser UI, so the bundle translates that principle into runtime legibility for:
- API traces,
- proxy logs,
- System VM launch configs,
- boot logs and reports,
- durable command transcripts.

Files:
- `docs/RELIABILITY.md`
- `scripts/harness/init_issue_run.sh`
- `scripts/harness/collect_verification_bundle.sh`
- the legibility sections of `verification-manifest-template.yaml`

## 5. Enforce architecture and taste mechanically

OpenAI emphasizes strict boundaries, predictable structure, custom linters, structural tests, and remediation-oriented lint messages to keep a fast-moving agent codebase coherent. citeturn299543view0turn384706view0

Vesper-specific implementation:
- `docs/ARCHITECTURE.md`
- `scripts/harness/check_structure.py`
- `docs/QUALITY_SCORE.md`

This bundle does not pretend the current repo is fully decomposed already; it uses warnings for existing hotspots and hard errors for direct boundary violations.

## 6. Feed human taste back into the repo

The article says recurring review comments and user-facing bugs should become docs or tooling, and when docs fall short, the rule should be promoted into code. citeturn384706view0

That idea is built into:
- the `WORKFLOW.md` step that requires harness upgrades when the agent gets stuck,
- the council rubric,
- the tech-debt tracker,
- the janitor scan and quality score reports.

## 7. Garbage collection and quality grades

OpenAI describes a recurring cleanup process: encode golden principles, scan for drift, update quality grades, and open small refactoring PRs continuously rather than doing occasional cleanup marathons. citeturn384706view0

For Vesper the bundle adds:
- `docs/design/core-beliefs.md`
- `scripts/harness/janitor_scan.py`
- `scripts/harness/compute_quality_score.py`
- `.github/workflows/harness-garbage-collection.yml`
- `.github/workflows/harness-quality-score.yml`

## 8. Minimal merge gates: adapt carefully

The article explicitly notes that minimal blocking merge gates were suitable for that team’s high-throughput environment and would be irresponsible in a lower-throughput environment. citeturn384706view0

Vesper is the kind of repo where looser gates can create false parity claims, so this bundle keeps blocking gates for:
- contract correctness,
- baseline and compat policy,
- docs integrity,
- architecture integrity,
- security-sensitive changes,
- runtime evidence where relevant.

## 9. Why this is better than the current Vesper status automation

The current Vesper snapshot already has meaningful contract and governance checks, but it also has workflows that can advance issue state from labels or merged commits rather than from durable evidence. The bundle replaces that with:
- repo-owned guidance,
- explicit run artifacts,
- keep/discard verification,
- council review,
- milestone logging,
- janitor and quality loops.

That better matches both Vesper’s design and the harness-engineering lessons from the article. citeturn299543view0turn384706view0
