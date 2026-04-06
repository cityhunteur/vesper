# Vesper Architecture Map

This document defines the architectural boundaries that make Vesper legible to both humans and agents.

## Top-level goal

Vesper exposes a Firecracker-compatible control plane on macOS while using staged backends to achieve parity:
- Backend A: stock Firecracker inside a Linux System VM on supported hosts.
- Backend B: eventual native macOS implementation behind the same API and lifecycle contract.

## Layer model

| Layer | Purpose | Current files | Notes |
| --- | --- | --- | --- |
| 0. Specs and baselines | Pinned upstream contract and parity metadata | `third_party/firecracker/*`, `docs/specs/*`, `src/spec.rs`, `src/baseline.rs`, `src/compat.rs` | The design and parity matrix are normative. |
| 1. Platform policy | Host capability and entitlement rules | `src/platform.rs`, `packaging/macos/*` | Must stay small and deterministic. |
| 2. Control plane | Firecracker-compatible state machine and deterministic errors | `src/api.rs` | Current hotspot. Prefer modular extraction over further accumulation. |
| 3. Runtime adapters | Socket proxying and System VM artifacts | `src/proxy.rs`, `src/systemvm.rs`, `scripts/systemvm/*` | Runtime-sensitive changes require runtime proof. |
| 4. Entrypoints | Binaries and command surfaces | `src/bin/*` | May depend on all library layers, but should not own business logic. |
| 5. Tests and harness | Contract tests, scripts, docs, Symphony templates | `tests/*`, `scripts/harness/*`, `.symphony/templates/*`, `docs/*` | These are first-class product assets, not optional extras. |

## Dependency rules

### Allowed directions

- Layer 0 can depend only on itself and external libraries.
- Layer 1 can depend on Layer 0.
- Layer 2 can depend on Layers 0 and 1.
- Layer 3 can depend on Layers 0 and 1.
- Layer 4 can depend on any library layer.
- Layer 5 may observe all layers but should not become a hidden source of product behavior.

### Disallowed directions

- Control-plane code must not import runtime adapter logic directly.
- Proxy and System VM modules must not depend on API state-machine code.
- Baseline/spec/compat modules must not depend on runtime or control-plane behavior.
- Issue-state automation must not become an implicit source of truth.

## Architectural invariants

1. `vesperd` is the only public control-plane endpoint.
2. Raw Firecracker and containerd sockets remain private to the System VM.
3. Boundary parsing and validation happen at the edges:
   - API request bodies,
   - compat exception registries,
   - baseline lock metadata,
   - proxy target parsing,
   - System VM launch config.
4. Deterministic error envelopes and pinned baseline revisions are part of the contract.
5. Runtime-sensitive changes need runtime artifacts, not just unit tests.
6. A reachable route is not parity.

## Hotspots and decomposition queue

The current repository snapshot has one notable hotspot:

- `src/api.rs` is large enough that further accumulation will reduce legibility and make future agent runs less reliable.

Preferred extraction order:
1. `src/api/actions.rs`
2. `src/api/machine_config.rs`
3. `src/api/boot_source.rs`
4. `src/api/logger_metrics.rs`
5. `src/api/errors.rs`
6. `src/api/vm_config.rs`

Each extraction should preserve behavior and bring matching tests with it.

## Guardrails for future work

- Add new parity behavior only with matching tests or runtime evidence.
- Prefer new repo-local abstractions over opaque helper libraries when the abstraction is small, testable, and easier for agents to reason about.
- When a rule is repeated in review, promote it into:
  - a doc,
  - a template,
  - a script,
  - or a workflow.

## Files that encode these boundaries

- `scripts/harness/check_structure.py`
- `scripts/harness/compute_quality_score.py`
- `docs/process/harness-engineering.md`
- `WORKFLOW.md`
