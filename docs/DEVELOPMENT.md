---
status: verified
owner: cityhunteur
last_reviewed: 2026-11-11
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
| Docker CLI (`docker`) | any recent; verified with 29.8 | only for `make docker-smoke` (client only; the daemon runs in the System VM) |
| `plutil` | macOS built-in; optional | `scripts/ci/validate-entitlements.sh` falls back to Python `plistlib` |
| Go toolchain (`go`) | 1.27 (`shim/go.mod`); CI uses `actions/setup-go@v5` with that file | only for `make shim-build` / `make shim-test`, which `make systemvm-overlay` needs |
| Xcode command line tools (`swift`, `codesign`) | Swift 5.9+ (`launcher/Package.swift`); verified with Swift 6.4 on macOS 27 | only for `make vmlauncher-build` |
| `bsdtar` | macOS built-in `tar`; Linux needs `libarchive-tools` | `scripts/systemvm/fetch-image.sh` (reads the ISO) |
| `gh` CLI | authenticated with write access | only for `scripts/update-issue-checklists.sh` |

Supported host: macOS for the dev loop and entitlements. Linux CI runs `cargo test`, the matrix checks and the
entitlement lint for every `src/**`, `tests/**`, `docs/generated/**` and `docs/specs/*.yaml` change
(`.github/workflows/rust-tests.yml`), plus the Python sensors; [TESTING.md](TESTING.md) lists which workflow runs what.

## Setup

No one-time setup beyond the toolchain. `cargo build` fetches dependencies from `Cargo.lock`.

## Configuration

There is no `.env` file. The binaries take flags, not environment variables; see
[src/bin/README.md](../src/bin/README.md) for every flag and default. Environment variables that do exist:

| Variable | Purpose | Required | Local default |
|---|---|---|---|
| `VESPER_GIT_COMMIT`, `VESPER_BUILD_DATE` | Baked into `buildinfo` at compile time | no | `unknown` when unset ([src/buildinfo.rs](../src/buildinfo.rs)) |
| `VESPER_SHARES` | Host directories to share into the System VM over virtio-fs, space-separated `PATH`, `PATH:ro` or `PATH:nowatch` (no inotify replay of host edits, GH #150); read by `make systemvm-launch-config` | no | empty |
| `VESPER_DATA_DISK` | Persistent System VM data disk image on macOS (GH #164); a relative path is taken from the repository root; `none` attaches no disk, so the System VM keeps its state in tmpfs. Created sparse on first use; delete it with the System VM stopped to reset all container state ([systemvm-bringup.md](process/systemvm-bringup.md#persistent-data-disk-gh-164)); read by `make systemvm-launch-config` | no | `run/systemvm/data.img` |
| `VESPER_DATA_DISK_GIB` | Size in GiB of a data disk image that `make systemvm-launch-config` creates; an existing image is never resized | no | `64` |
| `VALIDATE_ENTITLEMENTS_FORCE_PYTHON` | Force the Python plist validator | no | `0` |
| `REPO` | Target repo for `scripts/update-issue-checklists.sh` | no | `cityhunteur/vesper` |
| `VESPER_ISSUE_ID`, `VESPER_RUN_DIR`, `VESPER_BUNDLE_DIR` | Exported by the `env.sh` that `scripts/harness/init_issue_run.sh` writes | no | per issue |

None of these are secrets.

## Local infrastructure

None. There is no database, queue or container dependency. The System VM image is not checked in:
`make systemvm-fetch` downloads the Alpine virt ISO pinned in `docs/specs/systemvm-image.lock.yaml` (about 80 MB,
cached under `build/systemvm/cache/`), verifies every digest and unpacks the kernel into `build/systemvm/`.
`make systemvm-overlay` downloads the pinned guest packages (about 55 MB) and packs them with
`systemvm/overlay/` into a derived initramfs; `make vmlauncher-build` then `make systemvm-boot-smoke` boots it on
Virtualization.framework (macOS, Apple silicon). `make systemvm-prepare` runs all four steps; `make systemvm-up`, `make systemvm-control-suite` and
`make systemvm-down` keep the VM running and exercise the guest agent over the vsock bridge; `make docker-up`
then exposes the guest's Docker on `run/vesper/docker.sock` for the host Docker CLI
(see [process/systemvm-bringup.md](process/systemvm-bringup.md)).

## Build and run

```sh
make build            # cargo build --bins
make dev-up           # cargo build vesperd and vesper-proxy only, then start both in the background
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
  --surface-stub-matrix docs/generated/firecracker-api-surface-stub-matrix.json \
  --entitlements-file packaging/macos/vesperd.entitlements
```

## Format, lint and type-check

**Unknown / gap:** no `cargo fmt --check` or `cargo clippy` step exists in the Makefile or CI (tech debt TD-006).
Type checking happens through `cargo build` and `cargo test`. Repository-level structure and docs lints:

```sh
python3 scripts/harness/check_structure.py --root .   # dependency allowlist and line budgets (CI: harness-structure.yml)
python3 scripts/harness/janitor_scan.py --root .      # broken links, absolute paths, missing route handlers, oversized files, retired workflows; exit 1 on errors
make validate-entitlements                            # plist lint (CI: rust-tests.yml). With plutil present only syntax is checked; VALIDATE_ENTITLEMENTS_FORCE_PYTHON=1 also checks the required keys
```

## Code generation

All generated files are committed and checked in CI.

| Generator | Input | Output | Generate | Check |
|---|---|---|---|---|
| `contractgen` | `third_party/firecracker/firecracker.yaml` | `testdata/contracts/firecracker-routes.golden.json` | `make contract-generate` | `make contract-check` (CI: `contract-tests.yml`) |
| `baselinectl --mode generate-api-matrix` | swagger + `docs/specs/firecracker-api-baseline-captures.yaml` | `docs/generated/firecracker-api-surface-stub-matrix.json` | `make surface-matrix-generate` | `make surface-matrix-check` (CI: `rust-tests.yml`) |
| `baselinectl --mode generate-api-behavior-matrix` | swagger + `docs/specs/firecracker-api-behavior-captures.yaml` | `docs/generated/firecracker-api-behavior-matrix.json` | `make behavior-matrix-generate` | `make behavior-matrix-check` (CI: `rust-tests.yml`) |
| `baselinectl --mode generate` | baseline lock + `--backend/--host-os/--host-cpu` | `artifacts/parity-metadata.json` (not committed) | `cargo run --bin baselinectl -- --mode generate --backend backend-a --host-os macos --host-cpu arm64 --metadata artifacts/parity-metadata.json` | `cargo run --bin baselinectl -- --mode validate --metadata artifacts/parity-metadata.json` (CI: `baseline-governance.yml`) |
| `systemvmctl` | `docs/specs/systemvm-image.lock.yaml`, fetched ISO | `build/systemvm/{Image-virt,initramfs-virt,<iso>,image-manifest.json,launch-config.json}` (not committed) | `make systemvm-fetch`, `make systemvm-launch-config` | `make systemvm-build` (CI: `systemvm-image.yml`) |
| `build-initramfs.py` | `systemvm/overlay/`, `docs/specs/systemvm-packages.lock.yaml`, pinned initramfs | `build/systemvm/{localhost.apkovl.tar.gz,initramfs-vesper}` (not committed) | `make systemvm-overlay` | `systemvmctl verify-image` (CI: `systemvm-image.yml`) |
| `swift build` + `codesign` | `launcher/` | `build/vmlauncher/vesper-vmlauncher` (not committed) | `make vmlauncher-build` | `launcher-build` job of `systemvm-image.yml` |

## Documentation checks

```sh
python3 harness/check_docs.py            # structural checks
python3 harness/check_docs.py --strict   # also fails on unfilled template content; this is what docs.yml runs in CI
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
