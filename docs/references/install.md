---
status: deprecated
owner: cityhunteur
last_reviewed: 2026-11-05
---

> Historical note from the 2026-03 harness-engineering bundle. Paths such as `docs/index.md`, `docs/design/` and `docs/plans/` refer to the layout that was replaced on 2026-11-05; see [index.md](index.md).

# Installation notes

1. Copy `repo-overlay/` into the Vesper repository.
2. Replace the root `AGENTS.md` with the overlay version.
3. Add the `.symphony/templates/` directory and `WORKFLOW.md`.
4. Enable the new harness workflows in GitHub Actions.
5. Keep the existing baseline, compat, and contract workflows.
6. Once the new flow is stable, remove or disable:
   - `.github/workflows/eval-on-review.yml`
   - `.github/workflows/promote-implemented-issues.yml`

This order keeps the useful existing sensors while replacing the misleading status automation.
