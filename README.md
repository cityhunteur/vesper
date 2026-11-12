# Vesper

**Firecracker microVMs on macOS.** Vesper gives Apple silicon Macs a Firecracker-compatible control plane: the same
API, the same behaviour and the same tooling as Firecracker on Linux, so tools and tests written for Firecracker run
on a Mac unchanged. Its first workload is a Docker Desktop replacement whose containers run in those microVMs.

> **Status: pre-alpha.** Vesper is under active development and is not ready for production use. Parity is tracked
> test by test against a pinned Firecracker revision, and every gap is recorded. See [What works today](#what-works-today).

[![Licence: BSL 1.1](https://img.shields.io/badge/licence-BSL%201.1-blue)](LICENSE)
![Platform: macOS on Apple silicon](https://img.shields.io/badge/platform-macOS%20%7C%20Apple%20silicon-lightgrey)
![Rust 2024](https://img.shields.io/badge/rust-2024-orange)

## Why Vesper

Firecracker needs Linux and KVM. Mac developers who build on Firecracker today use a remote Linux box or a general
VM with a hand-built setup, and the behaviour they test against is not the behaviour they ship on. Vesper aims for
**parity, measured, not claimed**:

- **One external endpoint.** `vesperd` serves the 38 routes of Firecracker's pinned OpenAPI definition over a Unix
  socket.
- **Stock Firecracker underneath.** On Macs with nested virtualisation (M3 and later), Backend A runs the unmodified
  Firecracker and jailer release in a small Alpine System VM. Its HTTP answers are Firecracker's own.
- **Pinned and audited.** Every parity claim cites the Firecracker revision in
  [the baseline lock](docs/specs/design-v0.baselines.lock.yaml) (v1.16.0). Every deviation is a dated, owned
  [compatibility exception](docs/specs/compat-exceptions.yaml) that CI validates.
- **Evidence over assertions.** Runtime claims come from suites that boot real microVMs, and their results are
  committed as baselines.

## How it fits together

```text
 macOS host                                         Alpine System VM (Virtualization.framework)
 ┌─────────────────────────────────────┐            ┌─────────────────────────────────────────────┐
 │ your tool ── Firecracker API ──▶ vesperd ─┐       │  vesper-agent ──▶ jailer ──▶ firecracker ──▶ microVM
 │                                     │  │ vsock  │                     (KVM, nested)             │
 │ docker CLI ──▶ vesper-proxy ────────┼──┴───────▶│  dockerd / containerd ──▶ containerd-shim-vesper-v2
 │                                     │            │                                               │
 │ vesper-vmlauncher (Swift) ── boots ─┼───────────▶│  data disk: images, snapshot store, jails     │
 └─────────────────────────────────────┘            └─────────────────────────────────────────────┘
```

- `vesperd` is the only public control-plane endpoint. Today it answers from a deterministic in-memory model; with
  `--backend systemvm` (in review, ADR 0007) it relays each API connection to a jailed Firecracker in the System VM.
- `vesper-proxy` exposes the System VM's Docker socket to the macOS Docker CLI.
- `containerd-shim-vesper-v2` (Go) runs containerd tasks as jailed microVMs.
- `vesper-vmlauncher` (Swift) boots the System VM and applies the nested-virtualisation policy.

[ARCHITECTURE.md](ARCHITECTURE.md) has the layers, the main flows and how each invariant is enforced.

## What works today

| Area | State | Proof |
|---|---|---|
| Firecracker API surface: 38 routes, deterministic errors | Done for the in-memory model | `cargo test`, `make contract-check`, `make behavior-matrix-check` (173 cases) |
| System VM boot on Apple silicon | Done: 200 of 200 boots under 5 s | `make systemvm-boot-smoke`, `make systemvm-reliability` |
| Docker from macOS (`docker run`, ports, binds, DNS, cross-arch) | Works, with containers on runc for now | `make docker-conformance` and the port, bind, DNS and workflow suites |
| Jailed stock Firecracker microVMs in the System VM | Works | `make systemvm-microvm-suite`, `make systemvm-parity-baseline` |
| `vesperd` driving real Firecracker (relay, snapshots, staging) | In review ([ADR 0007](docs/design-docs/adr/0007-vesperd-to-backend-a.md), GH #156) | `make systemvm-launchapi-suite`; the relay and snapshot suites land with it |
| containerd tasks as microVMs | Works with `ctr`, not yet with dockerd | `make ctr-conformance` (74 steps) |
| Upstream Firecracker integration suite | Runs unmodified in the System VM: 755 of 980 pass; the rest are classified | [GH #166 report](docs/exec-plans/completed/2026-11-12-GH166-upstream-suite-trial.md) |
| Backend B (no nested virtualisation, for M1 and M2) | Not started | GH #56 |

The [design](docs/specs/design-v0.md) has the full status by phase, and the [parity matrix](docs/specs/firecracker-feature-parity.md)
has it by feature.

## Quick start

You need an Apple silicon Mac (M3 or later for microVMs), macOS with Xcode command line tools, Rust stable, Go 1.27
and Python 3.11+. [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) lists every tool and version.

```sh
git clone https://github.com/cityhunteur/vesper.git && cd vesper

make build                     # the Rust binaries
cargo test                     # unit, contract and integration suites

make dev-up                    # vesperd (model backend) on run/vesperd.sock
curl --unix-socket run/vesperd.sock http://localhost/
make dev-down

make systemvm-prepare          # fetch and verify the pinned image and packages; build the overlay, shim and launcher
make systemvm-up               # boot the System VM
make docker-up                 # expose its Docker on run/vesper/docker.sock
DOCKER_HOST=unix://$PWD/run/vesper/docker.sock docker run --rm alpine echo hello
make docker-down systemvm-down
```

## Documentation

| If you want to… | Read |
|---|---|
| Understand the design and its status | [design-v0.md](docs/specs/design-v0.md), [ARCHITECTURE.md](ARCHITECTURE.md) |
| Build, run and configure | [DEVELOPMENT.md](docs/DEVELOPMENT.md), [System VM bring-up](docs/process/systemvm-bringup.md) |
| Run or write tests | [TESTING.md](docs/TESTING.md) |
| Check what matches Firecracker | [Parity matrix](docs/specs/firecracker-feature-parity.md), [compatibility exceptions](docs/specs/compat-exceptions.yaml) |
| See why a decision was made | [Decision log](docs/design-docs/decision-log.md), [ADRs](docs/design-docs/adr/index.md) |
| Review the security model | [SECURITY.md](docs/SECURITY.md); to report a vulnerability, [security policy](.github/SECURITY.md) |
| Find known gaps | [Tech-debt tracker](docs/exec-plans/tech-debt-tracker.md), [quality score](docs/QUALITY_SCORE.md) |
| Start anywhere | [docs/README.md](docs/README.md) |

## Built for humans and agents

Vesper is developed with coding agents working inside an engineering harness that keeps the repository honest:

- [AGENTS.md](AGENTS.md) is the map: invariants, where to look and what needs a human first.
- [harness/](harness/README.md) checks the knowledge base: links, front matter, reachability and ExecPlan shape
  (`python3 harness/check_docs.py`).
- [scripts/harness/](scripts/harness/README.md) checks the layer rules (`check_structure.py`), computes the
  [quality score](docs/QUALITY_SCORE.md) and collects verification bundles.
- [docs/PLANS.md](docs/PLANS.md) defines ExecPlans: living plans for multi-step work, kept in
  [docs/exec-plans/](docs/exec-plans/index.md).
- Baseline, compatibility and contract governance run in CI: `baselinectl`, `compatexctl`, `contractgen`.

The same rules apply to human contributors; see [CONTRIBUTING.md](CONTRIBUTING.md).

## Contributing

Issues and pull requests are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) first. Each change is one small,
verifiable unit with its tests and docs, and every commit is signed off under the Developer Certificate of Origin.
Please follow the [code of conduct](CODE_OF_CONDUCT.md).

## Licence

Vesper is source-available under the [Business Source License 1.1](LICENSE). You may use, modify and redistribute
it, and use it in production for your own and your organisation's internal purposes. You may not offer it, or a
product that includes it, to third parties without a commercial licence. Each version converts to the
**Apache License 2.0** four years after it is published.

Third-party components keep their own licences; see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) and
[NOTICE](NOTICE). Firecracker is a trademark of Amazon.com, Inc. or its affiliates. Vesper is an independent project
and is not affiliated with or endorsed by Amazon, Apple or Docker.
