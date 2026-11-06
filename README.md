# Vesper Blueprint

## System Overview

Vesper is a Rust crate (`vesper`, edition 2024) that exposes a
Firecracker-compatible control plane on macOS. The single external endpoint is
the `vesperd` binary, which serves the pinned Firecracker OpenAPI route set
over a Unix socket from an in-memory VM state machine (`src/api.rs`). Every
error response is a deterministic `ErrorEnvelope` that carries the pinned
Firecracker baseline revision from `docs/specs/design-v0.baselines.lock.yaml`.

The library is layered. Spec, baseline, and compat modules load and validate
the vendored OpenAPI, the baseline lock, and the compat exception registry.
The platform module validates macOS entitlements. The control plane
(`src/api.rs` and its `src/api/` route-family modules) owns the state machine
and route dispatch. Runtime adapters (`src/proxy.rs`, `src/systemvm.rs`)
forward sockets and build System VM artifacts. Six `clap` binaries under
`src/bin/` wrap these layers. Shell and Python helpers under `scripts/`
support CI, the local macOS dev loop, System VM bring-up, and the
engineering harness.

Normative design references are `docs/specs/design-v0.md` and
`docs/specs/firecracker-feature-parity.md`.

## Subdirectories & System Boundaries

- src: [./src/README.md](./src/README.md) - The `vesper` library crate.
  Public modules `api`, `api_matrix`, `api_behavior_matrix`, `baseline`,
  `buildinfo`, `compat`, `platform`, `proxy`, `spec`, `systemvm`.
  `AppState::router` registers a single fallback handler; `handle_all`
  dispatches on exact route templates and applies `validate_state_guard`
  using data-driven `RouteSemantics` from the surface stub matrix.
  - src/api: [./src/api/README.md](./src/api/README.md) - `pub(super)`
    handlers for `PUT /actions`, `PUT /boot-source`, `PUT /logger`,
    `PUT /metrics`, `PUT /cpu-config`, and `PUT|PATCH /machine-config`.
    They validate bodies, mutate `RwLock`-guarded `AppState`, and return
    `204` or an `ErrorEnvelope`.
  - src/bin: [./src/bin/README.md](./src/bin/README.md) - Binaries
    `vesperd`, `vesper-proxy`, `baselinectl`, `compatexctl`, `contractgen`,
    `systemvmctl`. Each exits `1` on failure after printing the error to
    stderr.
- scripts: [./scripts/README.md](./scripts/README.md) - Operational scripts.
  `update-issue-checklists.sh` appends a parity checklist to GitHub issues
  #1 to #40 via `gh`.
  - scripts/ci: [./scripts/ci/README.md](./scripts/ci/README.md) -
    `validate-entitlements.sh` lints the two macOS entitlement plists
    (`make validate-entitlements`).
  - scripts/harness: [./scripts/harness/README.md](./scripts/harness/README.md) -
    Python sensors `check_docs.py`, `check_structure.py`,
    `compute_quality_score.py`, `janitor_scan.py` and Bash bundle helpers
    `init_issue_run.sh`, `collect_verification_bundle.sh`.
  - scripts/macos: [./scripts/macos/README.md](./scripts/macos/README.md) -
    `dev-up.sh` / `dev-down.sh` build and run `vesperd` and `vesper-proxy`
    under `run/` (`make dev-up`, `make dev-down`).
  - scripts/systemvm: [./scripts/systemvm/README.md](./scripts/systemvm/README.md) -
    Wrappers around `cargo run --bin systemvmctl` for image build, launch
    config, and boot smoke (`make systemvm-*`).
- tests: Integration suites `api_contract.rs` and `contract_golden.rs`
  (test-only, no contract file).
- docs: Specs, process docs, generated matrices, and devlog. Authoritative
  design in `docs/specs/`; generated route matrices in `docs/generated/`.
- third_party: Vendored Firecracker OpenAPI (`firecracker.yaml`), read by
  `src/spec.rs`.
- packaging/macos: `vesperd.entitlements` and `vesper-proxy.entitlements`,
  validated by `src/platform.rs` and `scripts/ci/validate-entitlements.sh`.
- testdata/contracts: `firecracker-routes.golden.json`, written and checked
  by `contractgen`.
- docs/specs/systemvm-image.lock.yaml: pinned System VM kernel and initramfs,
  fetched into `build/systemvm/` by `make systemvm-fetch`.

## Global Invariants

- `vesperd` is the only public control-plane endpoint. Raw backend sockets are
  not part of the GA surface (`src/bin/vesperd.rs`, `src/proxy.rs`).
- `AppState::new_with_matrix` rejects empty routes, a blank Firecracker
  revision or version, and any surface stub matrix that fails
  `validate_surface_stub_matrix`. A matrix load failure in `vesperd` prevents
  binding the socket.
- `error_response` always emits an `ErrorEnvelope` with `path` set to the
  route template (the cleaned request path for a `404` on an unmatched route)
  and `baseline_firecracker_revision` set from the loaded baseline lock. `lock.baselines["firecracker"].revision` must be non-blank.
- Route gating is data-driven: `validate_state_guard` reads `RouteSemantics`
  (`Any`, `PreBoot`, `PostBoot`, `PreBootRuntime`) for the operation key.
  `PUT /actions` and `PATCH /vm` dispatch before the guard; all other routes
  run it first. The guard does not distinguish `Running` from `Paused`.
- Every one of the 37 pinned routes has an explicit arm in `handle_all`. The
  trailing `matrix_response` fallback (stub status from the surface stub
  matrix, always `{501, "NotImplemented", "operation not implemented"}`) is a
  safety net that is unreachable with the current swagger and generated matrix.
- `vm_state` changes only through `InstanceStart` (`PreBoot` to `Running`),
  `PATCH /vm` (`Running` to `Paused` and back), and a successful
  `PUT /snapshot/load` (to `Paused`, or `Running` when `resume_vm` is true).
- `validate_surface_stub_matrix` and `validate_api_behavior_matrix` require
  version 1, matching revision and version, no duplicate keys, semantics equal
  to `documented_route_semantics`, and total route coverage. `baselinectl`
  validate modes additionally require the on-disk matrix to equal a freshly
  generated one.
- `compat::validate` requires `status` in `{open, closed}`, `issue > 0`,
  `expiry_date` as `YYYY-MM-DD`, no open exception past expiry, and no open
  exceptions at all when `--require-zero-open` is set.
- When `--entitlements-file` is given, `vesperd` validates it for
  `com.apple.security.virtualization` before loading anything else; without
  the flag no entitlement check runs.
- Control-plane code does not depend on runtime adapters, and proxy and
  System VM modules do not depend on the API state machine
  (`scripts/harness/check_structure.py` `RULES` allowlist).
- All Bash scripts use `set -euo pipefail`. `scripts/macos/*` resolve paths
  from their own location; `scripts/systemvm/*` build absolute paths but run
  `cargo` in the caller's cwd; the harness bundle helpers default to the
  caller's cwd (`REPO_ROOT="${2:-.}"`).

## Global Side Effects

- `vesperd` creates the parent directory of `--socket`
  (default `/tmp/vesperd.sock`), removes any existing file there, binds a
  `UnixListener`, and chmods it to `0o660`.
- `vesper-proxy` and `proxy::run_server` bind the `--listen` Unix socket
  (default `/tmp/vesper/docker.sock`), remove a pre-existing socket file, and
  dial `unix://`, `tcp://`, or `vsock://<cid>:<port>` upstreams; the vsock
  bridge writes a `CONNECT {port}\n` preface to `<vsock_socket_dir>/<cid>.sock`.
- `AppState` mutations happen under `std::sync::RwLock`; lock acquisitions
  panic on poison. Drive, pmem, boot-source, logger, and metrics handlers open
  host files for validation (logger and metrics paths are created if absent).
- `baselinectl` and `contractgen` write `artifacts/parity-metadata.json`,
  `docs/generated/*.json`, and `testdata/contracts/firecracker-routes.golden.json`.
- `systemvmctl` and the `scripts/systemvm/` wrappers write
  `build/systemvm/image-manifest.json`, `build/systemvm/launch-config.json`,
  `run/systemvm/boot.log`, and `run/systemvm/boot-report.json`, and
  `run_boot_check` spawns the configured command per iteration.
- `scripts/macos/dev-up.sh` spawns detached `vesperd` and `vesper-proxy`
  processes and writes pid and log files under `run/`.
- `scripts/harness/init_issue_run.sh` writes under `.symphony/runs/<ISSUE-ID>/`
  (`results.tsv` and `current/`) and creates `run/<ISSUE-ID>/`;
  `collect_verification_bundle.sh` writes only under
  `.symphony/runs/<ISSUE-ID>/current/` and reads from `run/`.
- `scripts/update-issue-checklists.sh` edits the body of up to 40 GitHub
  issues in `$REPO` via `gh issue edit`.

## System Verification

- Run: `cargo test`
- Build: `make build`
- Contract and matrix checks: `make contract-check`, `make contract-test`,
  `make surface-matrix-check`, `make behavior-matrix-check`
- Baseline and compat policy: `cargo run --bin baselinectl -- --mode validate`,
  `cargo run --bin compatexctl -- --registry docs/specs/compat-exceptions.yaml`
- Packaging: `make validate-entitlements`
