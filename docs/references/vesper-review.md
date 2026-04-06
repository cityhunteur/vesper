# Vesper review with harness-engineering framing

## Executive summary

Vesper has a good design center: pinned baselines, a two-backend parity strategy, explicit compatibility governance, and a strong emphasis on deterministic Firecracker contract behavior. The current repository snapshot also shows useful groundwork in contract, baseline, compat, proxy, and System VM scaffolding.

The main weakness is not lack of ideas. It is the mismatch between:
- what the code can presently prove,
- what issue automation has historically suggested is complete,
- and what a future agent can reliably infer from the repository.

This is exactly the gap harness engineering is meant to close. citeturn299543view0turn384706view0

## What the current Vesper repo already does well

- Strong normative design in `docs/specs/design-v0.md`
- Parity audit in `docs/specs/firecracker-feature-parity.md`
- Baseline lock governance and compat exception validation
- Contract generation and route reachability checks
- Proxy support for unix and tcp forwarding
- System VM manifest and boot-smoke scaffolding
- Existing `AGENTS.md` that already leans toward evidence over issue state

## Main gaps relative to harness engineering

### 1. The repo lacks a structured docs system of record

The design docs are good, but there is no short top-level map plus structured docs index that makes progressive disclosure easy for future agents. The bundle fixes this with:
- `AGENTS.md` as a map,
- `docs/index.md`,
- `docs/ARCHITECTURE.md`,
- `docs/PLANS.md`,
- quality, reliability, and security docs. citeturn299543view0

### 2. There are not enough repo-local sensors

Vesper already has useful computational sensors, but it lacks repo-owned checks for:
- docs integrity,
- structural drift,
- quality score tracking,
- janitor-style drift scanning.

The bundle adds all four.

### 3. Runtime work is not yet sufficiently legible

OpenAI’s article stresses that agents need direct access to runtime evidence to reason reliably about bugs and fixes. Vesper’s equivalent is:
- API trace artifacts,
- proxy connection logs,
- System VM boot and launch artifacts,
- durable command transcripts.

The bundle adds a standard artifact layout and collector scripts. citeturn299543view0

### 4. Issue automation still overstates progress

The current repo contains optimistic issue-promotion workflows. Those are counterproductive for parity-sensitive runtime work because they can shift status based on labels or merged commits without a durable verification bundle.

The bundle replaces that philosophy with:
- keep/discard verification,
- milestone logging,
- council review,
- GitHub evidence as the durable sink.

## Current hotspot signals from the repo snapshot

Static scan of the current snapshot shows:
- 37 Firecracker routes in the generated route contract,
- 15 routes with explicit control-plane behavior in `src/api.rs`,
- a large `src/api.rs` hotspot,
- remaining gaps concentrated in runtime-heavy features such as drives, ballooning, MMDS, snapshots, hotplug memory, and network interface management.

These are not reasons for pessimism. They are exactly the kind of truths the harness should surface continuously.

## Recommended adoption order

1. Install the repo overlay.
2. Switch issue execution to the new `WORKFLOW.md`.
3. Start logging milestone and verification bundles on every accepted issue.
4. Enable docs, structure, quality, and janitor workflows.
5. Retire the optimistic status workflows.
6. Use the quality score and janitor report to drive small refactors and parity slice planning.

## Review limitation

This review is grounded in repository contents plus the harness-engineering article. It does not claim that the bundle was run inside a full Rust or Elixir toolchain environment.
