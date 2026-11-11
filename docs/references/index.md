---
status: verified
owner: cityhunteur
last_reviewed: 2026-11-06
---

# References

This directory holds material an agent needs and cannot reliably get elsewhere: notes from the harness-engineering
bundle that shaped this repository, and pointers to the pinned third-party sources. Normative Vesper behaviour lives
in [docs/specs/](../specs/index.md), not here.

## Third-party material

| Reference | Covers | Source and version | Retrieved |
|---|---|---|---|
| `third_party/firecracker/firecracker.yaml` | Firecracker OpenAPI loaded by `vesperd` to build its route table and version string | `firecracker-microvm/firecracker` at the revision pinned in [design-v0.baselines.lock.yaml](../specs/design-v0.baselines.lock.yaml) | lock `updated_at` 2026-03-01 |
| [design-v0.md § Normative baselines](../specs/design-v0.md#normative-baselines) | Which upstream documents parity claims cite (Firecracker, containerd, Apple Containerization) | lock file | 2026-03-01 |

No condensed library docs are kept yet; add one here when an agent repeatedly gets a dependency wrong.

## Historical bundle notes

These describe the 2026-03 harness-engineering overlay and use the pre-2026-11 docs layout (`docs/index.md`,
`docs/design/`, `docs/plans/`). They are kept for migration history and are marked `deprecated`.

| Note | Covers |
|---|---|
| [harness-engineering-adaptation.md](harness-engineering-adaptation.md) | How the bundle mapped OpenAI's harness-engineering ideas onto Vesper |
| [vesper-review.md](vesper-review.md) | Repository review framed around agent-first delivery and parity truth |
| [testing.md](testing.md) | Smoke-test plan for the installed harness overlay |
| [milestones.md](milestones.md) | Suggested milestone sequence after adoption |
| [install.md](install.md) | Original overlay installation notes |

To add a reference, save it as `docs/references/<name>.md` (or `<name>-llms.txt`), add a row above, and link it from
any design doc or spec that relies on it.
