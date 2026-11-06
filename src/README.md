# src (vesper library crate)

## Overview

The `vesper` library crate (edition 2024). `lib.rs` exposes ten public
modules. `api` is the Firecracker-compatible HTTP surface and in-memory VM
state machine served by `vesperd`. `api_matrix` and `api_behavior_matrix`
generate and validate the pinned route matrices under `docs/generated/`.
`baseline`, `compat`, `platform`, and `spec` load and validate the YAML
baseline lock, compat exception registry, macOS entitlements file, and the
vendored Firecracker OpenAPI. `proxy` forwards a Unix socket to a unix, tcp,
or vsock upstream. `systemvm` builds image manifests and launch configs and
runs boot smoke checks. `control` is the host client for the guest agent over
the vsock bridge (ADR 0003). `buildinfo` carries compile-time version metadata.

Every API error is a deterministic `ErrorEnvelope` that carries the pinned
Firecracker baseline revision. Route-level pre-boot and post-boot gating is
data-driven from the surface stub matrix semantics, not hardcoded per handler.

## Interface

Child contracts:

- [api](./api/README.md): route-family handler modules (`actions`,
  `boot_source`, `logger_metrics`, `machine_config`).
- [bin](./bin/README.md): `vesperd`, `vesper-proxy`, `baselinectl`,
  `compatexctl`, `contractgen`, `systemvmctl`.

Public symbols by file:

- `lib.rs`: `pub mod api, api_behavior_matrix, api_matrix, baseline,
  buildinfo, compat, control, platform, proxy, spec, systemvm`. `systemvm.rs` declares
  the private children `systemvm/image_lock.rs` and `systemvm/packages.rs`
  and re-exports them, so every symbol below is reachable as
  `vesper::systemvm::*`.
- `api.rs:ErrorEnvelope` `{ fault_message, code, path, method,
  baseline_firecracker_revision }`; `api.rs:OperationStatus`
  `{ status_code: StatusCode, code, message }`; `api.rs:RouteSpec`
  `{ method, path }`.
- `api.rs:AppState` (all fields private; `RwLock`-guarded VM state).
  `AppState::new(routes, firecracker_revision, firecracker_version)` generates
  the surface stub matrix then calls `AppState::new_with_matrix(routes,
  revision, version, &SurfaceStubMatrix)`. `AppState::router(self) -> Router`
  registers a single `fallback(any(handle_all))`; all dispatch happens in the
  private `handle_all`.
- `api.rs:handle_all` dispatch table (exact templates): `GET /`,
  `GET /version`, `GET /machine-config`, `GET /vm/config`, `PUT /actions`,
  `PATCH /vm`, `PUT /boot-source`, `PUT /cpu-config`, `PUT /logger`,
  `PUT /metrics`, `PUT|PATCH /machine-config`, `PUT /serial`, `PUT /entropy`,
  `PUT /vsock`, `GET|PUT|PATCH /mmds`, `PUT /mmds/config`, `GET|PUT|PATCH
  /balloon`, `GET|PATCH /balloon/statistics`, `GET /balloon/hinting/status`,
  `PATCH /balloon/hinting/start`, `PATCH /balloon/hinting/stop`,
  `PUT|PATCH /drives/{drive_id}`, `PUT|PATCH /network-interfaces/{iface_id}`,
  `PUT /pmem/{id}`, `GET|PUT|PATCH /hotplug/memory`, `PUT /snapshot/create`,
  `PUT /snapshot/load`. Every pinned route has an arm; the trailing
  `matrix_response` fallback is unreachable with the current swagger.
- `api_matrix.rs`: `RouteSemantics { Any, PreBoot, PostBoot,
  PreBootRuntime }`, `BaselineCaptureState`, `BaselineCaptureRegistry`,
  `BaselineCapture`, `MatrixStatus`, `SurfaceStubMatrixEntry`,
  `SurfaceStubMatrixSource`, `SurfaceStubMatrix`,
  `documented_route_semantics(operation)`, `generate_surface_stub_matrix`,
  `generate_surface_stub_matrix_with_captures`,
  `load_baseline_capture_registry`, `load_surface_stub_matrix`,
  `write_surface_stub_matrix`, `validate_surface_stub_matrix`,
  `validate_baseline_capture_registry`,
  `route_uses_matrix_backed_placeholder`,
  `DEFAULT_SURFACE_STUB_MATRIX_PATH`, `DEFAULT_BASELINE_CAPTURE_REGISTRY_PATH`.
- `api_behavior_matrix.rs`: `BehaviorPath`, `BehaviorCaptureState`,
  `BehaviorOutcome`, `BehaviorArch`, `BehaviorBodyAssertionKind`,
  `BehaviorJsonType`, `BehaviorBodyAssertion`, `BehaviorCase`,
  `ApiBehaviorCaptureRegistry`, `ApiBehaviorCapture`,
  `ApiBehaviorMatrixEntry`, `ApiBehaviorMatrixSource`, `ApiBehaviorMatrix`,
  `generate_api_behavior_matrix`,
  `generate_api_behavior_matrix_with_captures`,
  `load_api_behavior_capture_registry`, `load_api_behavior_matrix`,
  `write_api_behavior_matrix`, `validate_api_behavior_matrix`,
  `validate_api_behavior_capture_registry`,
  `DEFAULT_API_BEHAVIOR_MATRIX_PATH`,
  `DEFAULT_API_BEHAVIOR_CAPTURE_REGISTRY_PATH`.
- `baseline.rs`: `Lock { version, updated_at, baselines: BTreeMap<String,
  BaselineRef>, reporting: Reporting }`, `BaselineRef { repo, revision }`,
  `Reporting { required_metadata }`, `load(path)`,
  `build_metadata(lock, backend, host_os, host_cpu)`,
  `validate_metadata(lock, metadata)`.
- `buildinfo.rs`: `VERSION`, `COMMIT`, `BUILD_DATE` consts.
- `compat.rs`: `Registry { version, exceptions }`, `Exception { id, owner,
  issue, expiry_date, target_release, status, description }`,
  `Summary { open, closed }`, `load(path)`,
  `validate(registry, today, require_zero_open)`.
- `platform.rs`: `validate_entitlements_file(path: Option<&Path>,
  required_keys: &[&str])`.
- `proxy.rs`: `Target { scheme, address, cid, port }`, `parse_target(raw)`,
  `ServerConfig { listen_path, upstream_raw, dial_timeout,
  vsock_socket_dir }`, `async run_server(config, shutdown)`.
- `spec.rs`: `Route { method, path }`, `load_routes(path)`,
  `load_info_version(path)`.
- `control.rs`: `ControlEndpoint { bridge_path, port, io_timeout }` with
  `new(bridge, port)` (10 s timeouts), `Handshake { protocol_version,
  hostname, connect_ms }`, `Session` with `ping`, `version`, `echo(payload)`,
  `health`, `drop_connection`, `shutdown`, `keepalive(interval, total)`,
  `connect(&endpoint)`, `connect_with_retry(&endpoint, attempts, backoff)`,
  `SuiteStep`, `SuiteReport`, `run_suite(&endpoint, interval, total,
  payload_bytes)`, `write_report(path, &report)`; constants
  `AGENT_PROTOCOL_VERSION` (1), `DEFAULT_AGENT_PORT` (1024).
- `systemvm.rs` (manifest, launch config, boot smoke),
  `systemvm/image_lock.rs` (`ImageLock` and everything lock-related) and
  `systemvm/packages.rs` (`PackagesLock { schema_version, branch, arch,
  requested, packages: Vec<Package { name, version, repo, file, url, sha256,
  size_bytes }> }`, `load_packages_lock`, `validate_packages_lock`,
  `packages_list`, `verify_packages`, `PACKAGES_LOCK_SCHEMA_VERSION`):
  `ImageBuildConfig { kernel_path, rootfs_path: Option,
  initrd_path: Option, boot_media_path: Option }`, `ImageManifest` (schema
  `"2"`, optional rootfs, initrd and boot-media digests), `LaunchConfig {
  kernel_path, rootfs_path?, initrd_path?, boot_media_path?, kernel_cmdline,
  ready_pattern, max_boot_ms?, probe?: ProbeSettings, vsock_bridge_path?,
  agent_port?, network_mode, cpu_count, memory_mib, console_log_path }`, `ProbeSettings { login_user, shell_prompt,
  commands, success_pattern, shutdown_command }` with `validate()`,
  `LaunchInputs` (borrowed inputs for `default_launch_config`),
  `BootCheckConfig { …, max_boot_ms: Option }`, `BootCheckReport {
  iterations, successful, failed, failures, durations_ms, max_duration_ms,
  mean_duration_ms, boot_ms, max_boot_ms, max_boot_allowed_ms? }`, `ImageLock {
  source: ImageSource, artifacts: ImageArtifacts { kernel, initrd:
  ImageArtifact { member, member_sha256, unpack: UnpackMethod, file, sha256,
  format } }, boot_media: Option<BootMedia>, overlay: Option<OverlaySettings { dir,
  packages_lock, apkovl_path, file }>, control: Option<ControlSettings {
  bridge_dir, cid, agent_port }> (`bridge_path()` = `<bridge_dir>/<cid>.sock`),
  boot: BootSettings { kernel_cmdline,
  ready_pattern, max_boot_ms, probe: Option<ProbeSettings> },
  expected_image_id }`, `ImageManifest.overlay_initrd_{path,sha256}`, `UnpackMethod {
  None, ZbootGzip }`, `build_manifest`, `image_id`, `write_manifest`,
  `load_image_lock`, `validate_image_lock`, `image_lock_env`, `verify_image`,
  `default_launch_config`, `launch_config_from_lock`, `write_launch_config`,
  `run_boot_check`; constants `MANIFEST_SCHEMA_VERSION`,
  `IMAGE_LOCK_SCHEMA_VERSION` (5), `DEFAULT_KERNEL_CMDLINE` (`console=hvc0`),
  `DEFAULT_READY_PATTERN` (`login:`).

## Invariants

- `api.rs:AppState::new_with_matrix` rejects empty routes, blank revision,
  blank version, and any matrix that fails `validate_surface_stub_matrix`.
  Routes are deduplicated by `operation_key` (`"{METHOD} {clean_path}"`).
- `api.rs:error_response` always emits `ErrorEnvelope` with `path` set to the
  route template (for example `/drives/{drive_id}`) and
  `baseline_firecracker_revision` set to `AppState.firecracker_revision`.
  Unmatched method or path returns `404 NotFound "route not found"` with the
  actual cleaned path.
- `api.rs:handle_all` dispatches `PUT /actions` and `PATCH /vm` before
  `validate_state_guard`; every other route runs the guard first.
- `api.rs:validate_state_guard` looks up `RouteSemantics` for the operation
  key (default `Any`). `PreBoot` semantics while not `PreBoot` returns
  `ACTION_NOT_SUPPORTED_POSTBOOT_MESSAGE`; `PostBoot` semantics while
  `PreBoot` returns `ACTION_NOT_SUPPORTED_PREBOOT_MESSAGE`; both as
  `400 InvalidState`. The guard does not distinguish `Running` from `Paused`.
- `api.rs:handle_patch_vm`: `Paused` while `Running` and `Resumed` while
  `Paused` succeed; `PreBoot` returns `ACTION_NOT_SUPPORTED_PREBOOT_MESSAGE`;
  other combinations return `state_invalid_message`
  (`"operation PATCH /vm is not allowed while instance state is {state}"`).
- `api.rs:handle_put_snapshot_create_stub` requires `Paused`; `Running`
  returns `SNAPSHOT_CREATE_REQUIRES_PAUSED_MESSAGE`. No snapshot is written.
- `api.rs:handle_put_snapshot_load_stub` requires exactly one of
  `mem_file_path` or `mem_backend`, and `snapshot_load_has_blocking_config`
  must be false (logger, metrics, serial, the MMDS data store, and balloon
  hinting state do not block; any other configured resource does). The route
  has `PreBoot` semantics, so `validate_state_guard` rejects it after boot; on
  success `vm_state` becomes `Paused`, or `Running` when `resume_vm` is true.
- `api.rs:matrix_response` would return the stub status from the surface stub
  matrix (missing key: `500 InternalMatrixError`), but no current route
  reaches it.
- `api.rs:handle_put_vsock` rejects `guest_cid < 3` with
  `INVALID_VSOCK_GUEST_CID_MESSAGE`.
- `api.rs:full_vm_config_response` (`GET /vm/config`) always reports
  `cpu-config` as absent, even after a successful `PUT /cpu-config`.
- `api.rs` MMDS: data store and patch payloads are capped at
  `MMDS_DATA_STORE_LIMIT_BYTES` (51200) with `413 PayloadTooLarge`;
  `PATCH /mmds` applies RFC 7396 merge semantics (`apply_json_merge_patch`);
  `PUT /mmds/config` requires a non-empty interface list, every id present in
  `network_interfaces`, and a `169.254.x.y` address with `x` not 0 or 255.
- `api.rs` devices: drive and pmem ids in the body must equal the path
  parameter; only one root device may exist across drives and pmem; drive and
  pmem backing files are opened for validation; a duplicate `guest_mac` or
  `host_dev_name` on network interfaces is rejected (tap-busy is simulated
  from in-memory state, no tap is opened). Memory hotplug sizes must satisfy
  `validate_memory_hotplug_config` (block >= 2 MiB power of two, slot >= 128
  MiB multiple of block, total >= slot and a multiple of slot); `PATCH
  /hotplug/memory` requires `requested_size_mib <= total` and a multiple of
  `block_size_mib`.
- `api.rs` balloon: `amount_mib` must not exceed `machine_config.mem_size_mib`;
  statistics cannot be toggled between enabled and disabled after
  activation; hinting routes require `free_page_hinting`.
- `api_matrix.rs:route_uses_matrix_backed_placeholder` always returns
  `false`, so every generated entry is `not_applicable` and a non-empty
  capture registry fails generation. All stub entries are
  `{501, "NotImplemented", "operation not implemented"}`.
- `api_matrix.rs:validate_surface_stub_matrix` and
  `api_behavior_matrix.rs:validate_api_behavior_matrix` require version 1,
  matching revision and version, no duplicate keys, semantics equal to
  `documented_route_semantics`, total route coverage (no missing or extra
  operations), non-empty `notes`, `source.captures`, and `operations`, status
  codes in 100..=599 with non-blank code and message, capture-state
  consistency, and (behavior matrix only) a matching `behavior_path`.
- `baseline.rs:load` requires non-zero `version`, non-empty `baselines`, and
  non-empty `reporting.required_metadata`. `validate_metadata` requires
  every key listed in `required_metadata` to be present and non-blank; the
  required key set is data-driven from the lock file.
- `compat.rs:load` rejects `version == 0`. `compat.rs:validate`: `id`,
  `owner`, and `target_release` must be non-blank, `status` must be `open` or
  `closed` (case-insensitive), `issue > 0`, `expiry_date` is `YYYY-MM-DD`, an open
  exception past expiry fails, and `require_zero_open` fails on any open
  exception.
- `platform.rs:validate_entitlements_file`: each required key must appear
  literally as `<key>{key}</key>`; a `None` path is `Ok`.
- `proxy.rs:run_server` returns `Err` for a blank `listen_path` or
  `upstream_raw` before touching the filesystem; a vsock dial errors on a
  blank `vsock_socket_dir`.
- `proxy.rs:parse_target` accepts only `unix://`, `tcp://`, and
  `vsock://<cid>:<port>`. The vsock bridge preface is exactly
  `"CONNECT {port}\n"` with no reply awaited. Upstream dial is bounded by
  `dial_timeout`; relay has no size or idle limits.
- `spec.rs:load_routes` sorts by path then method and dedups; errors on a
  missing `paths` section or empty route list.
- `systemvm.rs:run_boot_check` requires `iterations > 0` and non-blank
  `command`, `log_path`, and `report_path`, clamps `timeout_seconds` to at
  least 1, captures the command's stdout (logged as `stdout=` lines), takes
  `boot_ms` from the first `{"status":"ready","elapsed_ms":N}` JSON line
  (else the wall-clock duration), fails an iteration whose `boot_ms` exceeds
  `max_boot_ms`, and returns `Err` if any iteration fails, after writing the
  log and report.
- `systemvm.rs:ProbeSettings::validate` requires non-blank fields, a
  non-empty command list, no newlines in typed lines, and a
  `success_pattern` that appears in no typed line (the console echoes
  input). `default_launch_config` runs it and rejects `max_boot_ms == 0`.
- `systemvm.rs:build_manifest` and `default_launch_config` require a kernel
  plus at least one of rootfs or initrd; `image_id` is
  `sha256("<kernel>:<rootfs or empty>:<initrd or empty>:<boot media or
  empty>")` over hex digests, so the same blob in different roles yields
  different ids. `default_launch_config` uses `kernel_cmdline` verbatim when
  given, else `console=hvc0` plus ` root=/dev/vda rw` (or `/dev/vdb` when boot
  media precedes the rootfs) only when a rootfs is given; `ready_pattern`
  defaults to `login:`.
- `systemvm.rs:load_image_lock` parses the YAML into `ImageLock` with
  `deny_unknown_fields`, then `validate_image_lock` requires schema version
  `5`, an `https://` URL, non-zero `size_bytes`, non-blank `boot.*`, a
  `control` (when present) with a relative `bridge_dir`, `cid >= 3` and a
  non-zero `agent_port`,
  `boot.max_boot_ms > 0`, a valid `boot.probe` when present, an `overlay`
  (when present) with relative paths, a bare `file`, an absolute
  `*.apkovl.tar.gz` `apkovl_path` named by `apkovl=` in the command line,
  64-char
  lowercase hex for every digest and `expected_image_id`, `member_sha256 ==
  sha256` for artifacts with `unpack: none`, `boot_media.sha256 ==
  source.sha256`, and distinct bare file names for kernel, initrd and boot
  media. `verify_image(lock, dir)` requires `dir/<file>` to exist for both
  artifacts and the boot media, match the pinned digests, and produce
  `expected_image_id`; with an overlay it verifies `dir/<overlay.file>`
  when present (it must start with the pinned initramfs bytes) and records
  its digest, and with `require_overlay` its absence is an error.
  `launch_config_from_lock` errors when the overlay file is missing.
  `launch_config_from_lock` names those files (the derived initramfs when
  there is an overlay), the lock's `kernel_cmdline` and `ready_pattern`, and
  NAT networking.
- `control.rs:connect` writes `CONNECT <port>\n`, requires a greeting
  `VESPER-AGENT <version> <hostname>` with `version ==
  AGENT_PROTOCOL_VERSION`, and fails with a `make systemvm-up` hint when the
  bridge socket is missing. `echo` writes the payload from a second thread
  while reading, because the agent streams bytes back as they arrive.
  `run_suite` runs `handshake`, `keepalive`, `reconnect` (server-side `DROP`,
  then `connect_with_retry` 5 × 500 ms) and `large_payload` in that order,
  stopping at the first failure; `passed` requires all four.
- `systemvm/packages.rs:validate_packages_lock` requires schema `1`,
  non-blank branch and arch, every requested package in the closure,
  packages sorted by name without duplicates, `file ==
  <name>-<version>.apk`, an `https://` URL ending in the file name, 64-char
  lowercase hex digests and non-zero sizes. `verify_packages(lock, dir)`
  requires every file in `dir` with its digest.

## Side Effects

- `api.rs` mutates `AppState` fields under `std::sync::RwLock` (`vm_state`,
  `boot_source`, `cpu_config`, `logger_config`, `machine_config`,
  `metrics_config`, `serial_config`, `balloon_config`,
  `balloon_hinting_state`, `drives`, `network_interfaces`, `mmds_config`,
  `mmds_data_store`, `pmem_devices`, `memory_hotplug`, `entropy_config`,
  `vsock_config`). Lock acquisitions use `.expect` and panic on poison.
- `api.rs:validate_drive_backing_file` and `handle_put_pmem_stub` open the
  host backing file (read, write unless read-only) and drop the handle.
  `handle_put_pmem_stub` also calls `metadata()`.
- `api_matrix.rs`, `api_behavior_matrix.rs`, `baseline.rs`, `compat.rs`,
  `platform.rs`, `spec.rs` read files at caller-supplied paths; the matrix
  `write_*` functions call `create_dir_all` then `fs::write`. Capture
  registry validation checks `Path::exists` on every evidence file.
- `proxy.rs:run_server` creates the listen socket's parent directory, removes
  any existing socket file, binds a `UnixListener`, and spawns one tokio task
  per client. `handle_client` connects to the unix, tcp, or
  `<vsock_socket_dir>/<cid>.sock` upstream.
- `systemvm.rs:run_boot_check` spawns `std::process::Command::new(command)`
  per iteration, kills it on timeout, and writes `log_path` and
  `report_path`. `build_manifest` reads the kernel and rootfs files to
  compute SHA-256 digests.
- `buildinfo.rs` reads `CARGO_PKG_VERSION`, `VESPER_GIT_COMMIT`, and
  `VESPER_BUILD_DATE` at compile time only.

## Verification

- Run: `cargo test`
- Broader: `make test`, `make contract-test`, `make contract-check`
- Matrix freshness: `cargo run --bin baselinectl -- --mode validate-api-matrix`
