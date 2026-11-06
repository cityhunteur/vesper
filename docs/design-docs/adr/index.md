---
status: verified
owner: cityhunteur
last_reviewed: 2026-11-06
---

# Architecture decision records

An ADR records one significant decision: its context, the options considered, what was chosen and the consequences.
ADRs are never edited after acceptance except to change their status. A new decision supersedes an old one through a
new ADR, and the old ADR then links forward to it. The repository had no ADR series before 2026-11-05.

## Records

| ADR | Title | Status | Date | Supersedes |
|---|---|---|---|---|
| [0001](0001-systemvm-launcher-swift-helper.md) | Boot the System VM through a thin Swift Virtualization.framework helper | accepted | 2026-11-06 | none |
| [0002](0002-systemvm-overlay-via-initramfs.md) | Customise the System VM with an Alpine overlay delivered through the initramfs | accepted | 2026-11-06 | none |

## Candidates

Decisions that are visible in the code but whose history and reasoning are not recorded. No retrospective ADRs are
written for them; the people who made them should.

- **Rewrite from Go to Rust** (2026-03-01). Visible in [devlog.md](../../devlog.md) and `Cargo.toml`. Who: the
  repository owner.
- **Two-backend parity strategy** (Backend A stock Firecracker in a Linux System VM, Backend B native macOS). Visible in
  [design-v0.md](../../specs/design-v0.md). Rationale is in the spec but no alternatives are recorded.
- **Single fallback handler dispatch** instead of per-route axum routes: `AppState::router` registers one
  `fallback(any(handle_all))` and matches templates by hand ([src/api.rs](../../../src/api.rs)). Why: unknown.
- **Data-driven route gating** through `RouteSemantics` in the generated surface stub matrix rather than per-handler
  guards, with `PUT /actions` and `PATCH /vm` as the two per-handler exceptions ([src/api_matrix.rs](../../../src/api_matrix.rs),
  [src/api.rs](../../../src/api.rs)). Why: unknown.
- **Phase 0 vsock bridge as host Unix socket plus `CONNECT <port>` preface** (GitHub #9, WOR-71). Visible in
  [src/proxy.rs](../../../src/proxy.rs) and [systemvm-bringup.md](../../process/systemvm-bringup.md).
- **Tracker truth split between Linear (planning) and GitHub (evidence)**. Visible in
  [harness-engineering.md](../../process/harness-engineering.md). Why: unknown.
- **Retiring optimistic issue-promotion workflows** (`eval-on-review.yml`, `promote-implemented-issues.yml`). The
  files are gone; the decision is recorded only in the tech-debt tracker (TD-002), while
  `docs/specs/firecracker-feature-parity.md` and `docs/references/install.md` still describe `eval-on-review.yml` as present.

## Writing an ADR

1. Copy [TEMPLATE.md](TEMPLATE.md) to `NNNN-short-title.md`, using the next free four-digit number.
2. Set status `proposed`, and add a row above.
3. On agreement, set status `accepted` and the date. If rejected, keep the file with status `rejected`.
4. To change a decision, write a new ADR. Set the old one to `superseded`, and link each ADR to the other.
