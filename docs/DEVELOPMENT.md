---
status: verified
owner: cityhunteur
last_reviewed: 2026-11-05
---

# Development

This page covers how to get from a clean checkout to a running, tested build. Every command here comes from the
repository's own tooling (`Makefile`, Cargo, CI workflows) and was run successfully on 2026-11-05 unless marked
otherwise.

## Prerequisites

| Tool | Version | Pinned in |
|---|---|---|
| Rust toolchain (`cargo`, `rustc`) | stable; CI uses `dtolnay/rust-toolchain@stable`, edition 2024 in `Cargo.toml` | **Not pinned**: no `rust-toolchain.toml` (tech debt TD-009) |
| Python 3 | 3.11+ for `harness/`; CI uses 3.12 (`docs.yml`) and `3.x` (`harness-*.yml`) | `.github/workflows/docs.yml` |
| GNU make, bash | any recent | `Makefile`, `scripts/**/*.sh` |
| `plutil` | macOS built-in; optional | `scripts/ci/validate-entitlements.sh` falls back to Python `plistlib` |
| `gh` CLI | authenticated with write access | only for `scripts/update-issue-checklists.sh` |

Supported host: macOS for the dev loop and entitlements; Linux CI runs every Cargo and Python check.

## Setup

No one-time setup beyond the toolchain. `cargo build` fetches dependencies from `Cargo.lock`.

## Configuration

There is no `.env` file. The binaries take flags, not environment variables; see
[src/bin/README.md](../src/bin/README.md) for every flag and default. Environment variables that do exist:

| Variable | Purpose | Required | Local default |
|---|---|---|---|
| `VESPER_GIT_COMMIT`, `VESPER_BUILD_DATE` | Baked into `buildinfo` at compile time | no | `unknown` when unset ([src/buildinfo.rs](../src/buildinfo.rs)) |
| `VALIDATE_ENTITLEMENTS_FORCE_PYTHON` | Force the Python plist validator | no | `0` |
| `REPO` | Target repo for `scripts/update-issue-checklists.sh` | no | `cityhunteur/vesper` |
| `VESPER_ISSUE_ID`, `VESPER_RUN_DIR`, `VESPER_BUNDLE_DIR` | Exported by the `env.sh` that `scripts/harness/init_issue_run.sh` writes | no | per issue |

None of these are secrets.

## Local infrastructure

None. There is no database, queue or container dependency. `assets/systemvm/vmlinux` and `assets/systemvm/rootfs.ext4`
are 47-byte placeholders; a real System VM boot needs real images supplied by path (**Unknown** where they come from;
see [process/systemvm-bringup.md](process/systemvm-bringup.md)).

## Build and run

```sh
make build            # cargo build --bins
make dev-up           # build, then start vesperd and vesper-proxy in the background
make dev-down         # stop them
```

`dev-up` listens on `run/vesperd.sock` (API) and `run/docker.sock` (proxy), writes `run/*.pid` and `run/*.log`, and
prints `vesperd listening on <socket>` into `run/vesperd.log` when up. Paths derive from the repository root, so one
instance per git worktree is isolated automatically. `dev-up` has no idempotency guard: run `dev-down` first.
Details: [scripts/macos/README.md](../scripts/macos/README.md).

To run `vesperd` by hand with explicit inputs:

```sh
cargo run --bin vesperd -- --socket run/vesperd.sock \
  --baseline-lock docs/specs/design-v0.baselines.lock.yaml \
  --swagger third_party/firecracker/firecracker.yaml \
  --entitlements-file packaging/macos/vesperd.entitlements
```

## Format, lint and type-check

**Unknown / gap:** no `cargo fmt --check` or `cargo clippy` step exists in the Makefile or CI (tech debt TD-006).
Type checking happens through `cargo build` and `cargo test`. Repository-level structure and docs lints:

```sh
python3 scripts/harness/check_structure.py --root .   # dependency allowlist and line budgets (CI: harness-structure.yml)
python3 scripts/harness/janitor_scan.py --root .      # broken links, missing route handlers, oversized files
make validate-entitlements                            # plist lint (CI: not wired; see TD-010)
```

## Code generation

All generated files are committed and checked in CI.

| Generator | Input | Output | Generate | Check |
|---|---|---|---|---|
| `contractgen` | `third_party/firecracker/firecracker.yaml` | `testdata/contracts/firecracker-routes.golden.json` | `make contract-generate` | `make contract-check` (CI: `contract-tests.yml`) |
| `baselinectl --mode generate-api-matrix` | swagger + `docs/specs/firecracker-api-baseline-captures.yaml` | `docs/generated/firecracker-api-surface-stub-matrix.json` | `make surface-matrix-generate` | `make surface-matrix-check` |
| `baselinectl --mode generate-api-behavior-matrix` | swagger + `docs/specs/firecracker-api-behavior-captures.yaml` | `docs/generated/firecracker-api-behavior-matrix.json` | `make behavior-matrix-generate` | `make behavior-matrix-check` |
| `baselinectl --mode generate` | baseline lock + `--backend/--host-os/--host-cpu` | `artifacts/parity-metadata.json` (not committed) | `cargo run --bin baselinectl -- --mode generate --backend backend-a --host-os macos --host-cpu arm64 --metadata artifacts/parity-metadata.json` | `cargo run --bin baselinectl -- --mode validate --metadata artifacts/parity-metadata.json` (CI: `baseline-governance.yml`) |
| `systemvmctl` | kernel + rootfs paths | `build/systemvm/*.json` (not committed) | `make systemvm-build`, `make systemvm-launch-config` | none |

## Documentation checks

```sh
python3 harness/check_docs.py            # structural checks; run in CI (docs.yml)
python3 harness/check_docs.py --strict   # also fails on unfilled template content
python3 -m unittest discover -s harness/tests
python3 scripts/harness/check_docs.py --root .   # older required-files and link checker (CI: harness-docs.yml)
```

These need Python 3.11 or later, and nothing outside the standard library. See [harness/README.md](../harness/README.md).

## Troubleshooting

- **`check_docs` reports "document is not reachable from AGENTS.md"**: link the file from its directory `index.md`
  (or `README.md`); every `docs/` directory needs one.
- **`make validate-entitlements` fails on Linux**: `plutil` is absent, so the Python path runs and also checks the
  required keys. Reproduce on macOS with `VALIDATE_ENTITLEMENTS_FORCE_PYTHON=1`.
- **`vesperd` exits at startup with an entitlements error**: the plist passed to `--entitlements-file` must contain
  the `com.apple.security.virtualization` key literally.
- **Duplicate `vesperd` processes after `make dev-up`**: the pid files were overwritten. Kill the strays by hand, then
  `make dev-down` and `make dev-up`.
- **`baselinectl --mode validate-api-matrix` says the matrix is stale**: regenerate with the printed command and
  commit the JSON.
