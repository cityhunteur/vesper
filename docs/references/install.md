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
