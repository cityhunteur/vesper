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
  (default `vesperd.sock` in `$XDG_RUNTIME_DIR/vesper`, `$TMPDIR/vesper` or `$HOME/.vesper/run`, mode 0700; socket 0600;
  `default_socket`, `vesper::proxy::claim_socket`, `bind_owner_only`), `--backend model|systemvm` (default `model`),
  `--baseline-lock`
  (default `docs/specs/design-v0.baselines.lock.yaml`), `--swagger`
  (default `third_party/firecracker/firecracker.yaml`),
  `--surface-stub-matrix`
  (default `docs/generated/firecracker-api-surface-stub-matrix.json`),
  `--entitlements-file` (optional). Calls
  `vesper::platform::validate_entitlements_file`, `vesper::baseline::load`,
  `vesper::spec::load_routes`, `vesper::spec::load_info_version`,
  `vesper::api_matrix::load_surface_stub_matrix`,
  `vesper::api::AppState::new_with_matrix`, and `AppState::router`.
  `shutdown_signal` resolves on Ctrl-C or SIGTERM. `--id`, `--memory-max`, `--stage`, `--bridge` and `--agent-port`
  are refused without `--backend systemvm`.
- `vesperd/stage.rs` (module `stage` of the `vesperd` binary, ADR 0007 slice 5): the subcommands `vesperd stage [--id
  ID] SRC DEST` and `vesperd store list|rm NAME`, with the global `--launch-config`, `--bridge` and `--agent-port`.
  `run` checks every form with `vesper::control` (`StageSource::parse`, `StageDest::parse`, `check_pair`,
  `needs_id`, `valid_store_name`), then sends one `VESPERD stage|store-list|store-rm` line over
  `vesper::control::connect` (600 s read timeout, since the reply follows the copy) and prints the guest's lines.
  Exit 0 done; 1 refused or failed in the System VM, or unreachable; 2 a bad argument, refused before connecting.
- `vesperd/systemvm.rs` (module `systemvm` of the `vesperd` binary, ADR 0007 slice 4): `--backend systemvm --id ID`
  (1 to 64 letters, digits and hyphens) with `--memory-max BYTES` (optional `LAUNCHAPI` ceiling), `--launch-config`
  (default `build/systemvm/launch-config.json`, read for `vsock_bridge_path` and `agent_port`), `--bridge PATH`
  (overrides it; must be named `<cid>.sock`), `--agent-port N` (default 1024) and `--stage SRC:DEST` (slice 5,
  repeatable, at most 16, parsed by `vesper::control::StageSpec`). `Options::from_args`, `bridge_and_port`, `run`.
  `run` claims the socket, asks `DATA status` (`check_data_disk`; refuses unless `formatted` or `mounted`), sends
  `LAUNCHAPI <id> [ceiling] [stage=SRC:DEST]... [tap=NAME]...` (one `tap=` per `--tap`, at most 8, checked by
  `vesper::control::valid_tap_name`; CE-0010) over `vesper::proxy::open_upstream` and checks `READY` with
  `vesper::control::parse_ready` against the swagger's `info.version`, binds the socket, then runs
  `vesper::proxy::serve` with the `FCAPI <id>` preface and the bounds until the lease closes or SIGTERM/SIGINT.
  Values (each reasoned in the GH #156 ExecPlan): greeting, `OK`, `ERR` and preface writes 5 s; `READY` 30 s, plus
  120 s with `--stage`; at most 4 `DATA status` lines; lines
  1 KiB; relay reads 8 KiB; idle timeout 120 s; per-request ceiling 120 s; 16 connections; `ERR busy` retried for
  15 s; teardown wait after closing the lease 20 s.
- `systemvmctl launch-config --share PATH[:ro][:nowatch]` (repeatable; read-write shares get `watch: true` unless `:nowatch`, GH #150) adds `shared_directories` to the written config;
  `systemvmctl launch-config --data-disk PATH` (absolute path to a disk image on macOS, GH #164) sets
  `data_disk_path` and appends `vesper_data=/dev/vdX` to the kernel command line
  (`vesper::systemvm::attach_data_disk`);
  `systemvmctl control share <args…>` runs `vesper-share` in the guest; `control data mount|status|stop` runs
  `vesper-data` (the persistent data disk); `control arch status|mode ARCH` runs
  `vesper-arch` (execution mode per architecture).
- `vesper-proxy.rs:main` (binary `vesper-proxy`, `#[tokio::main]`). `--publish-ports` adds the GH #28
  publisher (`--relay-port` default 2400, `--ports-status` default `run/vesper/ports.json`); it needs a vsock
  upstream, whose bridge socket is the gateway path, and exits 2 otherwise. Flags:
  `--listen` (default `/tmp/vesper/docker.sock`), `--upstream`
  (default `vsock://3:2375`), `--vsock-socket-dir`
  (default `/tmp/vesper/vsock`), `--dial-timeout-seconds` (default 5),
  `--listen-mode` (octal, default `600`), `--validate-upstream` (flag). Calls
  `vesper::proxy::run_server` with a `ServerConfig`; shutdown on Ctrl-C or
  SIGTERM.
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
  `--cmdline`, `--ready-pattern`, `--nested` (auto|require|off); `--console-log` default
  `run/systemvm/console.log`, `--out` default
  `build/systemvm/launch-config.json`), `lock-env` (`--lock` default
  `docs/specs/systemvm-image.lock.yaml`; prints `KEY='VALUE'` lines),
  `package-list` (`--lock` default `docs/specs/systemvm-packages.lock.yaml`;
  prints `file<TAB>url<TAB>sha256` lines), `archive-list` (`--lock`; one
  tab-separated line per archive member), `verify-packages` (`--lock`,
  `--dir` default `build/systemvm/cache/apks`, `--archives-dir` default
  `build/systemvm/cache/archives`),
  `verify-image` (`--lock`, `--dir` default `build/systemvm`, `--out` default
  `build/systemvm/image-manifest.json`, `--require-overlay`),
  `backend-select` (`--launcher` default `build/vmlauncher/vesper-vmlauncher`,
  `--policy` default the lock's `boot.nested_virtualization`, `--lock`,
  `--capabilities` to print the raw probe; prints the `BackendSelection`
  JSON or exits 1 with diagnostics under `require`), `control`
  (`--config` default `build/systemvm/launch-config.json`, optional `--bridge`
  and `--port` overriding the config's `vsock_bridge_path` and `agent_port`;
  actions `handshake`, `ping`, `version`, `health`, `echo --bytes` (2 MiB),
  `keepalive --interval 5 --seconds 30`, `shutdown`, `kill <service>`, `logs
  <messages|containerd|docker> --lines 50`, `services-suite --report
  run/systemvm/services-report.json --services containerd,docker
  --recovery-seconds 60`, `microvm <args…>` (runs `vesper-microvm` in the
  guest, prints its output, exits non-zero on its failure), `microvm-suite
  --report run/systemvm/microvm-report.json --id vesper-test`, `rootfs
  <args…>` (runs `vesper-rootfs` in the guest), `rootfs-suite --report
  run/systemvm/rootfs-report.json --id vesper-rootfs-test`, `workload
  <args…>` (runs `vesper-workload` in the guest), `workload-suite --report
  run/systemvm/workload-report.json`, `fcparity <args…>` (runs `vesper-fcparity` in the guest), `ctr <args…>` (runs `vesper-ctr` in the guest), `exec-suite --report run/systemvm/exec-report.json`, `shim-suite --report run/systemvm/shim-report.json`, `bundle-suite --report run/systemvm/bundle-report.json`, `restart-suite --report run/systemvm/restart-report.json`, `vesperd <args…>` (runs `vesper-vesperd` in the guest: status|isolation|list|log|reap|kill-holder|kill-fc ID|test-…), `launchapi-suite --report run/systemvm/launchapi-report.json --id vesper-launchapi --swagger third_party/firecracker/firecracker.yaml` (ADR 0007 slice 3; `READY` must report the swagger's `info.version`), `vesperd-suite --report run/systemvm/vesperd-report.json --id vesper-vesperd --vesperd target/debug/vesperd --workdir run/vesperd-suite` (ADR 0007 slice 4; starts `vesperd --backend systemvm` with the control endpoint's bridge and port), `snapshot-suite --report run/systemvm/snapshot-report.json --id vesper-snap --vesperd target/debug/vesperd --workdir run/vesperd-suite --share-dir run/vesperd-share` (ADR 0007 slice 5; the share directory must be inside a share of the launch config), `replay-suite --report run/systemvm/replay-report.json [--publish PATH] [--filter TEXT] --id vesper-rp --vesperd target/debug/vesperd --workdir run/vesperd-suite --share-dir run/vesperd-share` (ADR 0007 slice 6; `src/bin/systemvmctl/replay.rs` joins `vesper::api_replay` to the matrix-agnostic runner in `vesper::control`),
  `parity-suite --report run/systemvm/parity-suite-report.json --id vesper-parity --perf-cycles 20`, `suite --report
  run/systemvm/control-report.json --keepalive-interval 2 --keepalive-seconds
  20 --payload-bytes 2097152`), `boot-smoke` (`--iterations` 5,
  `--timeout-seconds` 20, `--command` `sh`, `--args` `-c,echo boot-ok`,
  optional `--max-boot-ms`, `--log` default `run/systemvm/boot.log`,
  `--report` default `run/systemvm/boot-report.json`; prints `boot smoke
  passed: successful=… failed=… max_boot_ms=… (threshold …)`; the wrapper
  script substitutes the launcher and the lock's threshold). Calls `vesper::systemvm::{build_manifest, write_manifest,
  default_launch_config, launch_config_from_lock, write_launch_config,
  load_image_lock, image_lock_env, verify_image, load_packages_lock,
  packages_list, verify_packages, run_boot_check, attach_data_disk}` and
  `vesper::control::{connect, run_suite, write_report, ControlEndpoint}`.

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
- `vesper-proxy.rs:main`: dial timeout is clamped to at least 1 second; a
  non-octal `--listen-mode` exits 2; with `--validate-upstream` a missing
  upstream socket exits 1 before listening.
- `vesperd.rs:run` (`--backend systemvm`): the socket lock is taken before `LAUNCHAPI`, so a second `vesperd` on
  the socket launches nothing; a `READY` version other than the swagger's `info.version` closes the lease and exits
  1 without binding; `vesperd`'s own failures on a client connection (bridge, agent, `FCAPI`, timeouts, the
  connection cap) close it with no bytes; every HTTP byte comes from Firecracker.
- `vesperd.rs:run`: when `--entitlements-file` is given it is validated first
  and must contain `com.apple.security.virtualization` (no check without the
  flag); `AppState` is built only via
  `new_with_matrix`, so a matrix load failure prevents binding the socket.

## Side Effects

- `vesperd.rs:run` creates the parent directory of `--socket` (mode 0700), takes `<socket>.lock`, removes a stale
  socket (never a live one or a non-socket), binds a `tokio::net::UnixListener`, chmods the socket to `0o600`,
  prints `vesperd listening on <socket>`, and serves the router until shutdown.
- `vesperd.rs:run` with `--backend systemvm` starts one microVM in the System VM (`LAUNCHAPI`), holds its lease,
  opens one bridge connection per client connection (`FCAPI`), and prints `vesperd listening on <socket> (backend
  systemvm, microVM <id>, Firecracker <v> pid <n>)`. On SIGTERM or SIGINT it closes the lease, which stops the
  microVM, waits for the agent's teardown, removes the socket and exits 0. When the agent closes the lease
  (Firecracker exited, the System VM went away) it removes the socket and exits 1. Its status lines never panic on
  a closed stdout. With `--stage` the System VM copies or links files into the new jail before Firecracker starts.
- `vesperd stage` copies a file into a microVM's jail or the snapshot store in the System VM's data disk; `vesperd
  store rm` removes a store file.
- `vesper-proxy.rs:main` listens on the `--listen` Unix socket and forwards
  to `--upstream` through `vesper::proxy::run_server`.
- `baselinectl.rs` reads the lock, swagger, capture, and matrix files; writes
  `--metadata` (generate), `--matrix` (generate-api-matrix), and
  `--behavior-matrix` (generate-api-behavior-matrix), creating parent
  directories as needed.
- `compatexctl.rs:run` reads the registry YAML and the system clock when
  `--today` is absent.
- `contractgen.rs:run` reads the swagger file and writes or reads `--out`.
- `systemvmctl.rs control`: exits non-zero when the bridge is unreachable, the
  agent speaks another protocol version, a reply is unexpected, an echoed
  payload differs, or any suite step fails (the report is still written).
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
