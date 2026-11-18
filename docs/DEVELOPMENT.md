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
| `VESPER_SYSTEMVM_CPUS` | The System VM's CPUs, read by `make systemvm-launch-config` and `make systemvm-up-shares` (`systemvmctl launch-config --cpus`). A shim microVM can have at most this many vCPUs (TD-048) | no | `2` |
| `VESPER_SYSTEMVM_MEMORY_MIB` | The System VM's memory in MiB, at least 1024 (`systemvmctl launch-config --memory-mib`). The microVM memory budget is this less a 256 MiB reserve, so a task with a large memory limit needs a larger System VM (TD-048, TD-056) | no | `2048` |
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

### Disk space on the Mac

The System VM's data disk `run/systemvm/data.img` is a sparse 64 GiB file. Files deleted in the System VM keep their
space on the Mac until the disk is trimmed:

```sh
cargo run -q --bin systemvmctl -- control data trim   # TRIM discard=… trimmed=<bytes>
du -sh run/systemvm/data.img
```

On 2026-11-16 this took the image from 61 GiB to 12 GiB with 11 GiB in use. Nothing in the System VM is deleted.

### Containers are not root on the System VM (ADR 0010)

`make docker-up` prints `containers mapped`. dockerd runs every container in a remapped user namespace, behind an
authorisation plugin, so some `docker` commands that work on other hosts fail here:

| Command | Result |
|---|---|
| `docker run --privileged`, `--pid=host`, `--network=host`, `--ipc=host`, `--userns=host` | refused |
| `docker run --runtime <name>` other than `vesper-runc` (the default) or `runc`, volumes with driver options (`docker volume create -o …`, Compose `driver_opts`) | refused |
| `docker build` with BuildKit | refused; use `DOCKER_BUILDKIT=0` |
| `docker swarm`, `docker service`, `docker plugin` | refused |
| a container create whose request is 1 MiB or more | refused |
| `-v /var/run/docker.sock:…` | mounts, but the container cannot open the socket |
| `-v /some/new/path:…` where the path does not exist and is not inside a shared macOS directory | refused; Docker would create it on the System VM |
| `docker run --runtime runc` with a bind mount from a share | for its first second the bind can appear owned by `nobody`; `chmod` and `git` can fail (TD-062: the default runtime refreshes the owner, runc does not) |
| `chmod` or `touch` of a share's root directory, bound as a whole | fails for every uid (TD-038); bind a directory inside the share |

Images, volumes and containers made before this change are still on the data disk, unused; `make docker-up` says
so. Pull or build images again. Volumes hold data, so they can be copied into the remapped store:

```sh
cargo run -q --bin systemvmctl -- control data legacy status               # VOLUME <name> kib=<n> imported|not-imported|has-options
cargo run -q --bin systemvmctl -- control data legacy import-volume NAME   # IMPORTED <name> entries=N kib=N unmapped=N
```

`import-volume` copies one volume into a new volume of the same name; the old one stays as it was. Each owner is
shifted into the container range, so a file owned by root or by uid 1000 in the old volume has the same owner in a
container. An owner outside 0 to 65535 becomes `nobody` (counted as `unmapped`). Hard links, symbolic links,
modes and set-id bits are kept; extended attributes, ACLs and file capabilities are not. It refuses a name that
already exists in the remapped store, a volume with driver options (bind, tmpfs or NFS: create it again) and a
copy that would not fit. Stop the containers that use the volume first. Nothing deletes the old store. The new
volume carries the label `vesper.import` (a one-time token), so the import never uses or removes a volume of the
same name that another client created while it was copying.

The upstream Firecracker suite (`make systemvm-upstream-suite`) is the one exception: it restarts the System VM
with unmapped containers on a data disk of its own (`run/systemvm/upstream-data.img`) and puts the normal
configuration back afterwards. Do not run other workloads during it.

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
