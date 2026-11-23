---
status: verified
owner: cityhunteur
last_reviewed: 2026-11-23
---

# Process documents

How work is executed and governed in this repository. Each document is authoritative for its subject; the
[issue workflow](../../WORKFLOW.md) and [CLAUDE.md](../../CLAUDE.md) point here.

| Document | Covers |
|---|---|
| [harness-engineering.md](harness-engineering.md) | Guides, sensors, verification bundles, tracker and GitHub update protocol, merge philosophy |
| [baseline-governance.md](baseline-governance.md) | Required steps when the baseline lock, capture registries or generated matrices change; CI enforcement |
| [compat-exceptions.md](compat-exceptions.md) | Fields and lifecycle of `docs/specs/compat-exceptions.yaml`; `compatexctl` checks |
| [macos-packaging.md](macos-packaging.md) | Entitlement plists, `make validate-entitlements`, the local dev loop |
| [systemvm-bringup.md](systemvm-bringup.md) | `systemvmctl` commands, Make targets, manifest reproducibility, the Phase 0 vsock bridge convention |
| [nightly-evidence.md](nightly-evidence.md) | `make nightly`: the System VM suites on owner hardware as one evidence bundle; the CI check; a launchd schedule (not installed) |
| [shim-operations.md](shim-operations.md) | Runbook for the Vesper containerd runtime: deploy, verify (`make ctr-conformance`), troubleshoot by symptom, recover; known limits |
