# bin (command-line entry points)

## Overview

Thin `clap`-based binaries over the `vesper` library crate. Cargo discovers
them automatically from `src/bin/*.rs` (there are no `[[bin]]` entries in
`Cargo.toml`). `vesperd` is the single external Firecracker-compatible API
endpoint; `vesper-proxy` forwards a Unix socket to a unix, tcp, or vsock
upstream; the remaining four are repository tooling for baseline, compat
exception, contract golden, and System VM artifacts. Every binary prints
errors to stderr and exits with code 1 on failure.

## Interface

- `vesperd.rs:main` (binary `vesperd`, `#[tokio::main]`). Flags: `--socket`
  (default `/tmp/vesperd.sock`), `--baseline-lock`
  (default `docs/specs/design-v0.baselines.lock.yaml`), `--swagger`
  (default `third_party/firecracker/firecracker.yaml`),
  `--surface-stub-matrix`
  (default `docs/generated/firecracker-api-surface-stub-matrix.json`),
  `--entitlements-file` (optional). Calls
  `vesper::platform::validate_entitlements_file`, `vesper::baseline::load`,
  `vesper::spec::load_routes`, `vesper::spec::load_info_version`,
  `vesper::api_matrix::load_surface_stub_matrix`,
  `vesper::api::AppState::new_with_matrix`, and `AppState::router`.
  `shutdown_signal` resolves on Ctrl-C or SIGTERM.
- `vesper-proxy.rs:main` (binary `vesper-proxy`, `#[tokio::main]`). Flags:
  `--listen` (default `/tmp/vesper/docker.sock`), `--upstream`
  (default `vsock://3:2375`), `--vsock-socket-dir`
  (default `/tmp/vesper/vsock`), `--dial-timeout-seconds` (default 5). Calls
  `vesper::proxy::run_server` with a `ServerConfig`; shutdown is Ctrl-C only
  (`tokio::signal::ctrl_c`), SIGTERM is not handled.
- `baselinectl.rs:main` (binary `baselinectl`). Flags: `--mode` (required),
  `--lock` (default `docs/specs/design-v0.baselines.lock.yaml`), `--metadata`
  (default `artifacts/parity-metadata.json`), `--swagger` (default
  `third_party/firecracker/firecracker.yaml`), `--matrix`, `--captures`,
  `--behavior-matrix`, `--behavior-captures` (defaults: the `docs/generated/`
  and `docs/specs/` paths), `--backend`, `--host-os`, `--host-cpu` (default
  empty). Calls
  `vesper::baseline::{load, build_metadata, validate_metadata}`,
  `vesper::spec::{load_routes, load_info_version}`, and the generate, write,
  load, and validate functions of `vesper::api_matrix` and
  `vesper::api_behavior_matrix`. Metadata JSON shape is
  `{"metadata": [{"key": String, "value": String}]}` (`MetadataPayload`).
- `compatexctl.rs:main` (binary `compatexctl`). Flags: `--registry`
  (default `docs/specs/compat-exceptions.yaml`), `--today` (optional
  `YYYY-MM-DD`), `--require-zero-open`. Calls `vesper::compat::load` and
  `vesper::compat::validate`.
- `contractgen.rs:main` (binary `contractgen`). Flags: `--mode`
  (default `generate`), `--swagger` (default the vendored yaml), `--out`
  (default `testdata/contracts/firecracker-routes.golden.json`). Calls
  `vesper::spec::load_routes`.
- `systemvmctl.rs:main` (binary `systemvmctl`). Subcommands (`Commands`):
  `build` (`--kernel`, optional `--rootfs`, `--initrd`, `--boot-media`,
  `--out` default `build/systemvm/image-manifest.json`), `launch-config`
  (either `--from-lock` with `--lock` and `--dir` default `build/systemvm`, or
  `--kernel` with optional `--rootfs`, `--initrd`, `--boot-media`,
  `--cmdline`, `--ready-pattern`; `--console-log` default
  `run/systemvm/console.log`, `--out` default
  `build/systemvm/launch-config.json`), `lock-env` (`--lock` default
  `docs/specs/systemvm-image.lock.yaml`; prints `KEY='VALUE'` lines),
  `package-list` (`--lock` default `docs/specs/systemvm-packages.lock.yaml`;
  prints `file<TAB>url<TAB>sha256` lines), `verify-packages` (`--lock`,
  `--dir` default `build/systemvm/cache/apks`),
  `verify-image` (`--lock`, `--dir` default `build/systemvm`, `--out` default
  `build/systemvm/image-manifest.json`, `--require-overlay`), `boot-smoke` (`--iterations` 5,
  `--timeout-seconds` 20, `--command` `sh`, `--args` `-c,echo boot-ok`,
  optional `--max-boot-ms`, `--log` default `run/systemvm/boot.log`,
  `--report` default `run/systemvm/boot-report.json`; prints `boot smoke
  passed: successful=… failed=… max_boot_ms=… (threshold …)`; the wrapper
  script substitutes the launcher and the lock's threshold). Calls `vesper::systemvm::{build_manifest, write_manifest,
  default_launch_config, launch_config_from_lock, write_launch_config,
  load_image_lock, image_lock_env, verify_image, load_packages_lock,
  packages_list, verify_packages, run_boot_check}`.

Subdirectories: none.

## Invariants

- `baselinectl.rs:run`: `--mode` must be one of `generate`, `validate`,
  `generate-api-matrix`, `validate-api-matrix`,
  `generate-api-behavior-matrix`, `validate-api-behavior-matrix`. The
  baseline lock is loaded before any mode dispatch.
- `baselinectl.rs:firecracker_revision` and `vesperd.rs:run`:
  `lock.baselines["firecracker"].revision` must exist and be non-blank, else
  `"baseline lock missing firecracker revision"`.
- `baselinectl.rs:validate`: the metadata array must be non-empty.
- `baselinectl.rs:validate_api_matrix` and `validate_api_behavior_matrix`:
  the on-disk matrix must pass library validation and be equal to a freshly
  generated matrix, else it is reported as stale with the regenerate command.
- `compatexctl.rs:run`: `--today` must parse as `%Y-%m-%d`.
- `contractgen.rs:run`: `--mode` must be `generate` or `check`; in `check`
  mode the golden file must equal the serialized routes byte for byte.
- `systemvmctl.rs:Commands`: exactly one subcommand is required; `--kernel`
  is required for `build` and for `launch-config` unless `--from-lock` is
  given (`--from-lock` conflicts with every explicit input flag), and the
  library rejects a call with neither `--rootfs` nor `--initrd`.
  `verify-image` exits non-zero on a missing artifact or boot media, a digest
  mismatch, an `image_id` that differs from the lock's `expected_image_id`,
  a derived initramfs that does not start with the pinned one, or (with
  `--require-overlay`) a missing derived initramfs.
- `vesper-proxy.rs:main`: dial timeout is clamped to at least 1 second.
- `vesperd.rs:run`: when `--entitlements-file` is given it is validated first
  and must contain `com.apple.security.virtualization` (no check without the
  flag); `AppState` is built only via
  `new_with_matrix`, so a matrix load failure prevents binding the socket.

## Side Effects

- `vesperd.rs:run` creates the parent directory of `--socket`, removes any
  existing file at that path, binds a `tokio::net::UnixListener`, chmods the
  socket to `0o660`, prints `vesperd listening on <socket>`, and serves the
  router until shutdown.
- `vesper-proxy.rs:main` listens on the `--listen` Unix socket and forwards
  to `--upstream` through `vesper::proxy::run_server`.
- `baselinectl.rs` reads the lock, swagger, capture, and matrix files; writes
  `--metadata` (generate), `--matrix` (generate-api-matrix), and
  `--behavior-matrix` (generate-api-behavior-matrix), creating parent
  directories as needed.
- `compatexctl.rs:run` reads the registry YAML and the system clock when
  `--today` is absent.
- `contractgen.rs:run` reads the swagger file and writes or reads `--out`.
- `systemvmctl.rs` writes the image manifest and launch config to `--out`
  (`verify-image` also writes the manifest); `lock-env` only prints;
  `boot-smoke` delegates process spawning and log/report writes to
  `vesper::systemvm::run_boot_check`. No subcommand performs network access;
  downloading is done by `scripts/systemvm/fetch-image.sh`.
- All binaries except `vesper-proxy` print a success line to stdout; every
  binary prints the error string to stderr.

## Verification

- Run: `cargo test`
- Build all binaries: `cargo build --bins`
- Smoke: `cargo run --bin baselinectl -- --mode validate`
