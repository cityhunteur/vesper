# Bootstrap prompt: fill the knowledge base for this repository

Give this prompt to a coding agent after installing the template into a repository (see `harness/README.md`).
It works for any language or stack. Run it as a single long task; for a large repository, ask the agent to keep an
ExecPlan under `docs/exec-plans/active/` while it works.

---

You are a principal engineer specialising in software architecture, developer experience and agent harness
engineering. This repository has just had the harness knowledge-base template installed: `AGENTS.md`,
`ARCHITECTURE.md`, `docs/` and `harness/`. Your job is to replace every `TODO(harness)` marker with
repository-specific content that is backed by evidence. When you finish, an agent with no prior context should be able
to discover the architecture, run the right commands, plan and verify changes, and diagnose failures, all from the
repository alone.

Principle: humans define intent and constraints; agents execute within an observable, verifiable environment.

## Rules

1. **Evidence or nothing.** Inspect the implementation before you write about it. Never invent commands, components,
   guarantees, APIs, SLOs or policies. Label claims **Verified** (cite the path), **Intended**, **Proposed** or
   **Unknown**, as defined in `docs/README.md`. An honest **Unknown** with a way to resolve it is better than a guess.
2. **Run what you document.** A command goes into the docs only after you have seen it succeed, or seen it defined in
   tooling that CI runs. If it needs credentials or external services you don't have, say so, and don't run it.
3. **Preserve existing knowledge.** First, find the existing README, CONTRIBUTING, ADRs, architecture docs and agent
   instruction files (`AGENTS.md`, `CLAUDE.md`, `.cursor/rules`, `.github/copilot-instructions.md`, and the like).
   Keep valid instructions, and link to authoritative documents instead of copying them. Don't delete or move
   historical documents without a clear reason, and say what you did. If the installer reported files it skipped, merge
   the template's intent into those files instead of overwriting them.
4. **Progressive disclosure.** Keep `AGENTS.md` near 100 lines; the checker fails above 150. Put detail in the focused
   documents. Don't create a document only to fill the tree. Add `DESIGN.md`, `FRONTEND.md`, `PRODUCT_SENSE.md` or
   `runbooks/` only if the project needs them.
5. **Scope.** Change only documentation, plus minimal documentation-validation wiring. Don't refactor application code,
   change runtime behaviour, or change deployment, security or release policy. Preserve uncommitted changes.
   Never write secrets or production data into docs. Don't commit, push or deploy unless explicitly authorised.
6. **Prefer enforcement.** For each important invariant, find out whether existing tooling already enforces it
   (linters, type checks, dependency or architecture tests, schema validation, CI). Record the mechanism. Where nothing
   enforces it, propose a check in the tech-debt tracker instead of adding new tooling.

## Phase 1: Discover

Audit the repository before you edit anything. Establish the following:

- Languages and versions, frameworks, build and dependency tools, monorepo or single service.
- Entry points, components, domain boundaries and dependency directions. Trace two or three representative end-to-end
  flows through the code: request handling, background work, data persistence.
- API protocols and contracts, persistence, messaging, external integrations, auth boundaries, data ownership.
- Test frameworks and layout, local infrastructure, containers, deployment manifests, CI workflows.
- Observability: logging, metrics, tracing, health checks, dashboards, local diagnostic tools.
- Existing documentation, and whether each document is current, duplicated, obsolete or incomplete.

Build an evidence map for yourself: each claim you plan to write, and the files that support it. Don't paste the
map into the docs. Keep the docs concise, not an inventory of source files.

## Phase 2: Fill the documents

Work through the documents in this order. Each document's own `TODO(harness)` notes say what belongs in it.

1. `ARCHITECTURE.md`: purpose, components, boundaries, code map, main flows, deployment, invariants. Replace the
   example Mermaid diagrams with real ones, or delete them. Label any conceptual diagram as conceptual.
2. `docs/DEVELOPMENT.md` and `docs/TESTING.md`: verified commands only. In the test inventory, separate what exists
   from what is missing.
3. `docs/OBSERVABILITY.md`, `docs/RELIABILITY.md`, `docs/SECURITY.md`: list real capabilities. Record gaps as gaps.
4. `docs/product-specs/`: real domains or user journeys. Never infer business rules. Index external sources of truth.
5. `docs/design-docs/`: catalogue the existing designs where they live, and keep only the core beliefs you can
   support with evidence. Register the existing ADRs, or adopt the existing ADR convention. List decisions whose history
   is unknown as ADR candidates; don't write retrospective ADRs for them.
6. `docs/QUALITY_SCORE.md` and `docs/exec-plans/tech-debt-tracker.md`: grade only what you have evidence for.
   Give every gap, **Unknown** and unenforced invariant a debt entry.
7. `docs/README.md`: the repository overview, plus the ownership table including authoritative documents outside `docs/`.

As you finish each document, set its front matter: `owner` (ask, or use the CODEOWNERS entry; otherwise write
`unassigned` and add a debt entry), `status` (`draft`, or `verified` if every claim was checked) and `last_reviewed`
(today's date).

## Phase 3: Write AGENTS.md

Fill in the project overview, the repository map (real paths only), the commands, and the three to seven most
important invariants with their enforcement. Adapt the human-approval list to the project's real policy. Keep the
"Where to look", "How to work here" and "Definition of done" sections, adjusted to the repository's practices. If
an `AGENTS.md` existed before installation, merge its valid instructions into this one, or link to them.
Make sure `CLAUDE.md` contains `@AGENTS.md`, or that its existing content links there.

## Phase 4: Validate

Run these from the repository root:

    python3 harness/check_docs.py --strict
    python3 -m unittest discover -s harness/tests

Fix every finding. Each finding includes a `fix:` hint. Then confirm that the CI workflow
`.github/workflows/docs.yml` runs the checker, and switch it to `--strict` (or merge the step into the project's
existing CI). Re-read `AGENTS.md` as a newcomer would: can you find how to build, test and diagnose within two hops?

## Phase 5: Report

Finish with a short report that covers:

- the documents created or changed;
- the existing documents linked, merged or left alone, and why;
- the commands you verified, and the commands you documented but could not run;
- the main **Unknowns**, and who or what can resolve them;
- the invariants found, and which are enforced versus proposed;
- the top five tech-debt entries by priority;
- anything you chose not to do, with the reason.
