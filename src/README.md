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
  `boot_source`, `logger_metrics`, `machine_config`, `pmem`).
- [bin](./bin/README.md): `vesperd`, `vesper-proxy`, `baselinectl`,
  `compatexctl`, `contractgen`, `systemvmctl`.

Public symbols by file:

- `lib.rs`: `pub mod api, api_behavior_matrix, api_matrix, api_replay, baseline,
  buildinfo, compat, control, platform, proxy, spec, systemvm`. `systemvm.rs` declares
  the private children `systemvm/boot_check.rs` (the boot smoke check, TD-043), `systemvm/bridge.rs`, `systemvm/data_disk.rs`, `systemvm/image_lock.rs` and
  `systemvm/packages.rs` and re-exports them, so every symbol below is reachable as
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
  `PUT|PATCH /pmem/{id}`, `GET|PUT|PATCH /hotplug/memory`, `PUT /snapshot/create`,
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
- `api_replay.rs` (ADR 0007 slice 6): `ReplayScripts`, `ReplayFile`, `CaseScript`, `Step`, `Response`, `Verdict`,
  `CaseResult`, `ReplaySummary`, `case_key`, `load_replay_scripts`, `validate_replay_scripts`, `expand`,
  `expand_fixture`, `parse_step`, `applies`, `judge`, `replay`, `summarize`, `DEFAULT_REPLAY_SCRIPTS_PATH`,
  `MAX_WAIT_MS`, `FIXTURE_STATE`, `FIXTURE_MEM`, `SNAPSHOT_STATE`, `SNAPSHOT_MEM`. It turns the scripts in
  `docs/specs/firecracker-api-behavior-replay.yaml` into steps and judges a backend's last response against the
  matrix case; it runs no backend itself.
- `baseline.rs`: `Lock { version, updated_at, baselines: BTreeMap<String,
  BaselineRef>, reporting: Reporting }`, `BaselineRef { repo, revision,
  upstream_suite: Option<UpstreamSuite> }`,
  `Reporting { required_metadata }`, `load(path)`,
  `build_metadata(lock, backend, host_os, host_cpu)`,
  `validate_metadata(lock, metadata)`.
- `buildinfo.rs`: `VERSION`, `COMMIT`, `BUILD_DATE` consts.
- `compat.rs`: `Registry { version, exceptions }`, `Exception { id, owner,
  issue, expiry_date, target_release, status, description }`,
  `Summary { open, closed }`, `load(path)`,
  `validate(registry, today, require_zero_open)`.
- `platform.rs`: `validate_entitlements_file(path: Option<&Path>,
  required_keys: &[&str])`, `HostCapabilities { api_available,
  nested_virtualization_supported, virtualization_supported, macos, chip,
  model }`, `parse_host_capabilities(json)`, `probe_host_capabilities(launcher)`
  (runs `vesper-vmlauncher --host-capabilities`), `NestedPolicy { Auto,
  Require, Off }` with `parse`, `Backend { BackendA, BackendB }`,
  `BackendSelection { backend, nested_virtualization, implemented,
  diagnostics }`, `select_backend(&caps, policy)`.
- `proxy.rs`: `Target { scheme, address, cid, port }`, `parse_target(raw)`,
  `ServerConfig { listen_path, upstream_raw, dial_timeout, vsock_socket_dir,
  listen_mode, validate_upstream, preface, bounds }` with `Default` (5 s, mode `0o600`, no
  validation, no preface, no bounds), `upstream_socket_path(&target, vsock_socket_dir)`, `async
  run_server(config, shutdown)` (validate, bind, then `serve`), `async serve(listener, config, shutdown)` (the
  accept loop on a listener the caller bound), trait `Upstream` and `async open_upstream(&target, &config)` (dial,
  plus the vsock `CONNECT <port>` line), `claim_socket(path) -> File` (slice 1's owner-only directory check,
  `<path>.lock`, live-socket refusal and stale-socket removal, moved here from `vesperd` in ADR 0007 slice 4).
- `proxy/relay.rs` (ADR 0007 slice 4, `super::` only; re-exported by `proxy`): `Preface { greeting_prefix,
  request, accept }`, `Bounds { handshake_timeout, max_line, read_buffer, idle_timeout, request_ceiling,
  max_connections }` (`Default`: none, which keeps `copy_bidirectional`), `RelayEnd { Closed, Idle, Ceiling }`,
  `async read_line_bounded(reader, max, timeout)` (one byte at a time, so nothing past the newline is consumed),
  `async write_line(writer, line, timeout)`, constants `DEFAULT_MAX_LINE` (1 KiB) and `DEFAULT_READ_BUFFER`
  (8 KiB).
- `spec.rs`: `Route { method, path }`, `load_routes(path)`,
  `load_info_version(path)`.
- `systemvm.rs:SharedDirectory { tag, host_path, read_only, watch }` and `LaunchConfig.shared_directories`
  (serde default, omitted when empty); `shared_directories_from_specs(&[String])` turns `PATH[:ro][:nowatch]`
  into tags `share0`, `share1`, … and rejects relative paths or non-directories. `watch` (GH #150, serde
  default false) is true for a read-write share unless the spec ends in `:nowatch`; never for `:ro`.
- `systemvm/bridge.rs` (TD-027, ADR 0007): `LaunchConfig.bridge_allowed_ports` (serde default, omitted when
  empty); `default_bridge_ports(agent_port)` is the agent port plus `DOCKER_RELAY_PORT` (2375), `PORT_RELAY_PORT`
  (2400) and `SHARE_NOTIFY_PORT` (2401), sorted and deduplicated; `apply_bridge_allow_list(&mut LaunchConfig)`
  sets it when the config serves a bridge, and `launch_config_from_lock` calls it.
- `systemvm/data_disk.rs` (GH #164): `LaunchConfig.data_disk_path` (serde default, omitted when `None`, so
  configs without it still parse); `attach_data_disk(&mut LaunchConfig, path)` sets it and appends
  `vesper_data=<device>` to `kernel_cmdline`, rejecting a relative path or a second disk;
  `data_disk_device(&LaunchConfig)` is the next virtio block device after the boot media and the rootfs
  (`/dev/vda`, `/dev/vdb` or `/dev/vdc`); constant `DATA_DISK_LABEL` (`vesper-data`), the ext4 label the guest
  formats an empty disk with.
- `control.rs:Session::share(args)` runs `vesper-share` in the guest (same shape as the other guest tools);
  `Session::data(args)` runs `vesper-data` (agent command `DATA`); `Session::arch(args)` runs `vesper-arch`.
- `ports.rs`: `PortsConfig { docker_socket, bridge_path, relay_port, status_path, bind_ip }` (`Default`:
  `run/vesper/docker.sock`, `run/systemvm/vsock/3.sock`, 2400, `run/vesper/ports.json`, 127.0.0.1);
  `parse_bindings(inspect) -> Vec<Binding>` (`NetworkSettings.Ports`, IPv6 twins of a `0.0.0.0` binding
  dropped, sorted by host port); `parse_event(event) -> Option<(action, id, name)>` (container events only,
  `action` before `:`); `plan_binding(bind_ip, &Binding) -> Result<SocketAddr, String>` (TCP only; `0.0.0.0`,
  `::` and empty map to `bind_ip`; a loopback host ip is honoured; anything else is an error);
  `docker_get(socket, path) -> (status, body)` (minimal HTTP/1.1, Content-Length or chunked);
  `dial_gateway(bridge, relay_port, host_port)` (writes `CONNECT <relay>\nPORT <n>\n`, no reply expected);
  `Publisher::new(config).run()` (reconcile `/containers/json`, then follow `/events?filters={type:[container]}`;
  `start`/`unpause` publish, `die`/`stop`/`kill`/`destroy`/`pause` release; reconnects after 2 s);
  `Publisher::status() -> Status { path, gateway, updated_at, mappings[] }` with `Mapping { container, name,
  proto, host, host_port, container_port, target, state (listening|conflict|unsupported), error }`.
- `control.rs` (client), `control/suites.rs` (checklists, re-exported) and `control/suites/{bundle,restart,exec,launchapi,vesperd,snapshot}.rs`
  (the GH #24, #25 and #23 checklists, `super::` helpers only):
  `ControlEndpoint { bridge_path, port, io_timeout }` with
  `new(bridge, port)` (10 s timeouts), `Handshake { protocol_version,
  hostname, connect_ms }`, `Session` with `ping`, `version`, `echo(payload)`,
  `health`, `kill_service(name)`, `logs(name, count)`, `microvm(args)`,
  `rootfs(args)`, `workload(args)`, `fcparity(args)` and `ctr(args)` (guest tools; 600 s read timeout), `drop_connection`,
  `shutdown`, `keepalive(interval, total)`,
  `connect(&endpoint)`, `connect_with_retry(&endpoint, attempts, backoff)`,
  `SuiteStep`, `SuiteReport`, `run_suite(&endpoint, interval, total,
  payload_bytes)`, `run_services_suite(&endpoint, &[service], recovery_timeout)`,
  `run_microvm_suite(&endpoint, id)`, `run_rootfs_suite(&endpoint, id)`,
  `run_workload_suite(&endpoint)`, `run_parity_suite(&endpoint, id, perf_cycles)`, `run_shim_suite(&endpoint)`,
  `run_bundle_suite(&endpoint)`, `run_restart_suite(&endpoint)`, `run_exec_suite(&endpoint)`,
  `run_launchapi_suite(&endpoint, id, expected_version)`,
  `run_vesperd_suite(&endpoint, &VesperdRun { binary, workdir, extra_args }, id, expected_version)` with constants
  `VESPERD_IDLE_TIMEOUT` and `VESPERD_REQUEST_CEILING` (both 120 s, the values in `src/bin/vesperd/systemvm.rs`),
  `run_snapshot_suite(&endpoint, &SnapshotRun { vesperd, share_dir }, id)` with `prepare_workdir(dir)` and constants
  `DEFAULT_CEILING_MIB` (640), `MEASURE_GUEST_MIB` (640) and `MEASURE_FILL_MIB` (512),
  `ReplayRun { vesperd, share_dir }` with `prepare(&endpoint, files) -> ReplaySession`, `make_fixture`, `run_case`
  and `finish`, `ReplayStep`, `ReplayResponse` and `fixture_names(prefix)` (ADR 0007 slice 6, matrix-agnostic),
  `write_report(path, &report)`; constants
  `AGENT_PROTOCOL_VERSION` (1), `DEFAULT_AGENT_PORT` (1024).
- `control/launchapi.rs` (ADR 0007 slice 3, `super::` only): `Ready { id, pid, firecracker_version }`,
  `HttpResponse { status, body }`, `parse_ready(line, id)` (exactly `READY <id> pid=<n> firecracker_version=<digits
  and dots>`; `ERR <reason>` becomes the error), `valid_microvm_id(id)` (1 to 64 ASCII letters, digits, hyphens);
  `Session::launch_api(id, ceiling, timeout)` (sends `LAUNCHAPI`, waits up to `timeout` for `READY`; the session is
  then the lease), `Session::fcapi(id)` (`FCAPI`, waits for `OK`), `Session::http(method, path, body)` (one HTTP/1.1
  exchange on a relayed connection, Content-Length bodies up to 1 MiB, lines up to 8 KiB), `Session::http_raw(request)`
  (the response bytes unchanged), `http_request(method, path, body)`, `read_http_response(reader)` (the raw bytes of
  one response) and `parse_http_response(raw) -> HttpResponse`, `Session::wait_closed(timeout)`
  (end of file or reset from the agent), `Session::vesperd(args)` (agent `VESPERD`, the buffered guest-tool shape).
- `control/stage.rs` (ADR 0007 slice 5, `super::` only): the staging forms, checked as the guest helper checks them.
  `StageSource::{Share, Store, Jail}` and `StageDest::{Jail, Store}` with `parse` and `wire`; `StageSpec { src, dest }`
  with `parse("SRC:DEST")` (the `--stage` form: SRC `share:` or `store:`, DEST a jail path), `launch_token()`
  (`stage=SRC:DEST`) and `links()` (store `*.mem` and `*.vmstate`); `valid_stage_path` (absolute, letters, digits and
  `._+,@=-`, no `.` or `..` component, at most `MAX_STAGE_PATH` 1024 bytes), `valid_store_name` (at most
  `MAX_STORE_NAME` 128, no leading dot), `check_pair` (refuses store to store and jail to jail), `needs_id`,
  `parse_store_list`, `data_disk_in_use("DATA <state> …")` (`formatted` or `mounted`), `MAX_STAGES` (16);
  `Session::stage(id, src, dest)`, `Session::store_list()`, `Session::store_rm(name)` (agent `VESPERD stage`,
  `store-list`, `store-rm`; a refusal is `Err((rc, reason))`).
- `systemvm.rs` (manifest, launch config, boot smoke),
  `systemvm/image_lock.rs` (`ImageLock` and everything lock-related) and
  `systemvm/packages.rs` (`PackagesLock { schema_version, branch, arch,
  requested, packages: Vec<Package { name, version, repo, file, url, sha256,
  size_bytes }>, archives: Vec<Archive { name, version, url, sha256,
  size_bytes, members: Vec<ArchiveMember { member, guest_path, sha256,
  mode }> }> }`, `load_packages_lock`, `validate_packages_lock`,
  `packages_list`, `archive_list`, `verify_packages`, `verify_archives`,
  `PACKAGES_LOCK_SCHEMA_VERSION` (2)):
  `ImageBuildConfig { kernel_path, rootfs_path: Option,
  initrd_path: Option, boot_media_path: Option }`, `ImageManifest` (schema
  `"2"`, optional rootfs, initrd and boot-media digests), `LaunchConfig {
  kernel_path, rootfs_path?, initrd_path?, boot_media_path?, kernel_cmdline,
  ready_pattern, max_boot_ms?, probe?: ProbeSettings, vsock_bridge_path?,
  agent_port?, nested_virtualization?, network_mode, cpu_count, memory_mib,
  console_log_path, shared_directories, data_disk_path? }`, `ProbeSettings { login_user, shell_prompt,
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
  `IMAGE_LOCK_SCHEMA_VERSION` (6), `DEFAULT_KERNEL_CMDLINE` (`console=hvc0`),
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
  parameter; only one root device may exist across drives and pmem; drive
  backing files are opened at `PUT`, pmem backing files at `InstanceStart`
  (`api/pmem.rs`); `mtu` on a network interface must be 68 to 65535; after
  boot the three device `PUT` routes parse the body, check the ids and return
  `PCI is not enabled` (`reject_runtime_hotplug`, PCI disabled is modelled); a
  duplicate `guest_mac` or
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
- `baseline/upstream.rs` (GH #172): `UpstreamSuite { container, artifacts_bucket, artifacts_prefix, release_asset,
  paths, workers, test_timeout_s }` and `validate(arch)`; `TestResult { outcome, message, under_load, alone }` (the last two from the runner's isolated rerun), `Results`
  (`BTreeMap<nodeid, TestResult>`); `ClassFile { version, never_pass_alone, max_passed_alone, classes: Vec<Class> }` with `Class { id, kind, nodeid, any_test,
  message, outcomes, tracked_by, reason }` and `validate`; `glob` (`*` wildcards, no regex crate);
  `compare(results, reference, classes) -> Comparison { reference, total, passed, to_explain, per_class, unmatched, passed_alone,
  stale_classes, not_passing_on_reference, missing_from_run }`. Without a reference every test is taken to pass
  there; with one, only tests that passed there need a class. `Comparison::ok` is "nothing unmatched".
- `baseline.rs:load` requires non-zero `version`, non-empty `baselines`, and
  non-empty `reporting.required_metadata`; an `upstream_suite` is allowed only under `firecracker` and must pass
  `UpstreamSuite::validate` for the architecture its artefact prefix names. `validate_metadata` requires
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
  blank `vsock_socket_dir`. With `validate_upstream` it returns `Err` before
  binding unless the unix path or `<vsock_socket_dir>/<cid>.sock` exists and
  is a socket (tcp upstreams are not checked). After binding it applies
  `listen_mode` to the listen socket.
- `proxy.rs:parse_target` accepts only `unix://`, `tcp://`, and
  `vsock://<cid>:<port>`. The vsock bridge preface is exactly
  `"CONNECT {port}\n"` with no reply awaited. Upstream dial is bounded by
  `dial_timeout`. With the default `Bounds` the relay has no size or idle limits (`copy_bidirectional`), as before.
  With bounds, `proxy/relay.rs` applies `handshake_timeout` to the `CONNECT` write, the preface write and each
  preface line, refuses a preface line over `max_line` or a reply other than `accept` (the client connection is
  closed with no bytes), reads at most `read_buffer` bytes at a time with no total limit, passes a half-close on,
  and closes the connection when `idle_timeout` passes while the upstream spoke last (or nothing was sent yet) or
  `request_ceiling` passes while the client spoke last. Which side spoke last is the only signal; pipelined
  requests defeat it.
- `spec.rs:load_routes` sorts by path then method and dedups; errors on a
  missing `paths` section or empty route list.
- `systemvm/boot_check.rs:run_boot_check` requires `iterations > 0` and non-blank
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
  `6`, an `https://` URL, non-zero `size_bytes`, non-blank `boot.*`, a valid
  `boot.nested_virtualization` policy, a
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
  stopping at the first failure; `passed` requires all four. `run_services_suite`
  runs `handshake` then, per service, `<svc>_healthy_at_boot` (`HEALTH` state
  `started` and `<svc>_ok` true), `<svc>_simulated_crash` (`KILL`),
  `<svc>_recovers` (polls `HEALTH` every 500 ms until the timeout) and
  `<svc>_logs_capture_startup_and_recovery` (at least two startup lines in
  `/var/log/<svc>.log`); `passed` requires `1 + 4 × services` ok steps.
- `platform.rs:select_backend` never panics: `auto` on a host without
  nested virtualization returns `BackendB` with `implemented: false` and
  diagnostics naming the missing requirement (macOS 15, M3 or later);
  `require` on such a host returns `Err` with the same text; `off` returns
  `BackendB` without nested virtualization even on an M3.
- `control.rs:run_microvm_suite` runs `handshake`, a tolerated `cleanup_before`,
  `prepare`, `launch`, `jailer_isolation` (all of `uid_ok`, `chroot_ok`,
  `cgroup_ok`, `seccomp_ok` true in the manager's JSON), `stop`,
  `cleanup_after_stop`, `relaunch`, `abnormal_termination`,
  `cleanup_after_crash`, `guest_poweroff_launch`, `guest_poweroff_exits`
  (status `EXITED` within 15 s) and `cleanup_after_poweroff`; `passed`
  requires every step ok.
- `control/suites.rs:run_rootfs_suite` runs `handshake`, `import_trivial_image`,
  `build` (`--no-cache`), `reproducible_rebuild` (same `sha256=` from a
  second `--no-cache` build), `cache_hit` (`cached=true`, same digest),
  `provenance_embedded` (JSON with the source image id, non-empty layers, a
  baseline revision and `output_sha256` equal to the build digest),
  `invalid_image_rejected` (`ERR image not found`), `cleanup_before`,
  `launch_with_rootfs`, `rootfs_boots` (`VESPER-ROOTFS-OK <image id>` and
  `VESPER-ROOTFS-EXEC-OK` in the microVM log within 30 s) and
  `cleanup_after_rootfs_boot`; `passed` requires every step ok.
- `control/suites.rs:run_workload_suite` removes `wa`..`wd`, then runs
  `handshake`, `import_trivial_image`, `run_wa`, `run_wb_concurrent`,
  `run_wc_concurrent` (`STARTED`), `three_microvms_running` (three `RUNNING`
  rows with distinct `pid=`), `isolation_between_microvms` (every isolation
  flag true, three distinct cgroups, `wb` sees `absent` for `wa`'s file),
  `stop_wa_tears_down_its_microvm` (`STOPPED`), `others_unaffected_by_stop`
  (`wa` not RUNNING, `wb`/`wc` RUNNING, `wl-wa` CLEAN), `run_wd_short`,
  `wait_maps_exit_code_and_output` (`EXITED wd code=3`, output present),
  `rm_wa`..`rm_wd` (`REMOVED`) and `nothing_left` (empty `ps`); `passed`
  requires every step ok and at least 14 steps.
- `control/suites.rs:run_parity_suite` runs `handshake`, then `vesper-fcparity conformance <id>`
  turning each `CHECK <name> ok|fail <detail>` line into a `conformance:<name>` step and the
  `CONFORMANCE …` line into `conformance_summary` (ok when `failed=0`), then `perf_run_1` and
  `perf_run_2` (`vesper-fcparity perf <cycles>`; ok when the `PERF` line has `failures=0`; detail is
  that line's tokens) and `perf_comparable` (two runs, boot p50 drift at most 20 %); `passed` requires
  every step ok and at least one conformance step.
- `control/suites.rs:run_shim_suite` force-deletes `e2e`, `det`, `ca`, `cb`, `cc`, then runs `handshake`,
  `import_image` (`IMPORTED`), `e2e_run_exit_code_and_output` (`vesper-ctr run e2e echo hello-from-task; exit 3`
  → `RC 3` and an `OUT hello-from-task` line), `start_detached` (`STARTED`), `state_running` (`RUNNING` with a
  non-zero `pid=`), `task_metrics` (`vesper-ctr metrics det` → `METRICS det memory_bytes=<non-zero> …`),
  `invalid_start_running` and `invalid_delete_running` (`vesper-ctr try task start|delete det`
  must fail), `kill_detached`, `delete_detached`, `invalid_unknown_task`, `start_ca|cb|cc`,
  `three_tasks_running` (three `RUNNING` rows with distinct non-zero pids), `kill_*`/`delete_*` for each and
  `no_leftovers` (`CLEAN`); `passed` requires every step ok and at least 19 steps.
- `control/suites/bundle.rs:run_bundle_suite` force-deletes `l1`, `la`, `lb`, `e2e-base`, then runs
  `handshake`, `import_base_image`, `import_layered_image` (`IMPORTED … layers=2`), `layered_rootfs_boots`
  (`cat /etc/layer2-marker` → `layer-two`, rc 0), `start_la`, `meta_la_recorded` (`strategy` block-first,
  `kind` overlay, two lower layers), `start_lb_same_chain`, `meta_lb_cache_hit` (`cached` true, same `key` and
  `sha256` as `la`), `cache_one_drive_per_chain` (`CACHE n` with n lines), `layer_dedup_shared_base` (the trivial
  image's lower snapshot appears in the layered chain), `kill_*`, `delete_*`, `no_leftovers`; `passed` requires
  every step ok and at least 13 steps.
- `control/suites/restart.rs:run_restart_suite` force-deletes `rs1`..`rs3`, then runs `handshake`,
  `import_image`, `start_rs1`, `state_file_running` (`vesper-task.json` has `phase running` and the VMM pid),
  `containerd_killed_and_respawned` (`kill_service("containerd")`, then HEALTH shows containerd ok and started
  within 60 s), `task_survives_restart` (`RUNNING` with the same pid within 30 s), `kill_after_restart`,
  `delete_after_restart`, `start_rs2`, `kill_shim_rs2`, `task_gone_after_shim_crash` (`wait-gone` 30 s),
  `no_orphaned_microvm` (microVM status not RUNNING within 20 s, then `cleanup` CLEAN),
  `container_delete_after_crash`, `inflight_start_rs3`, `containerd_killed_during_inflight`,
  `inflight_fails_gracefully` (`ctr run --rm` exits non-zero), `inflight_task_reconciled` (whatever remains is
  killed and deleted) and `no_leftovers`; `passed` requires every step ok and at least 16 steps.
- `control/suites/launchapi.rs:run_launchapi_suite` ages and reaps leftovers of its ids (`id`, `id-fcexit`,
  `id-reap`, `id-reuse`, `id-orphan`), then runs `handshake`, `launch` (`READY` with `expected_version`, the pinned
  swagger's `info.version`), `duplicate_id_refused`, `fcapi_get_not_started`, `boot_through_api` (`PUT
  /machine-config`, `/boot-source`, `/actions` InstanceStart, each 204), `get_root_running`, `guest_boots`
  (`VESPER-MICROVM-OK` in the console log within 20 s), `jailer_isolation` (uid 20000, cgroup `/vesperd/<id>`,
  seccomp, chroot), `fcapi_refuses_bad_and_unknown_ids`, `fcparity_refuses_bad_id`, `shim_namespace_separate` (a
  shim microVM with the same id launched and cleaned without touching the `vesperd` one), `teardown_lease_closed`,
  `reused_id_after_teardown`, `cleanup_reused_id`, `teardown_firecracker_killed`,
  `teardown_lease_shell_killed_reaped` (no manual reap: the periodic reaper within 35 s),
  `reaper_ignores_reused_pid`, `reaper_spares_new_unrecorded_jail`, `default_container_cannot_open_vsock` and
  `nothing_left`; `passed` requires all 20 steps ok.
- `control/suites/vesperd.rs:run_vesperd_suite` (ADR 0007 slice 4) creates `workdir` with mode 0700, ages and
  reaps leftovers of its ids (`id`, `id-kill`, `id-fcexit`), starts `vesperd --backend systemvm` as a child process
  and drives the microVM only through its socket: `handshake`, `vesperd_starts` (socket 0600, the pinned version in
  its first line), `second_vesperd_on_socket_refused` (no microVM launched), `get_root_not_started`,
  `boot_through_vesperd`, `get_root_running`, `guest_boots`, `firecracker_error_byte_for_byte` (a 400 through
  `vesperd` equals the same request over `FCAPI`), `pause`, `snapshot_create_under_ceiling` (under a tenth of the
  ceiling), `resume`, `firecracker_connection_limit_passes_through` (Firecracker's own 503 to the 11th connection),
  `idle_timeout_closes_only_vesperd_connections` (about 125 s), `same_id_refused`, `sigterm_stops_microvm`,
  `sigkill_removes_microvm`, `firecracker_killed_vesperd_exits`, `tap_devices_in_own_netns` (two concurrent
  microVMs with `--tap tap0 --tap tap1`, each namespace holding exactly `lo` and the two taps owned by uid 20000, an
  unlisted tap Firecracker's own 400, `netns_ok`, the namespace gone after SIGTERM; CE-0010) and `nothing_left`
  (leases, jails and network namespaces); `passed` requires all 19 steps ok. It uses the control channel only to inspect and fault the guest.
- `control/suites/sharenet.rs:run_sharenet_suite` (GH #160 M2) cleans its four ids, then `namespace_holds_lo_and_one_tap`
  (`SHARENET` line checked field by field), `second_microvm_gets_its_own`,
  `nothing_outside_reaches_the_server_address` (`REACH root|namespace:<b>|container-bridge|container-host closed`),
  `its_own_guest_reaches_the_nfs_port_only`, `another_guest_does_not_reach_it`, `a_plain_launch_has_no_network`,
  `cleanup_removes_the_namespace`, `aborted_launch_is_reaped_after_its_grace_period`, `nothing_left`.
- `control/suites/bounds.rs:run_bounds_suite` (TD-055, TD-056) ages and reaps its ids, then `console_log_bounded`
  (a guest with `vesper_console_flood=2500000`; `VESPERD logsize` shows both files within 1 MiB plus one line),
  `log_pruned_after_keep_time` (`VESPERD test-age-log`, then `reap` prints `PRUNED`), `budget_refuses_a_second_vesperd`
  (`VESPERD test-mem-reserve` leaves room for one 640 MiB ceiling; the second `vesperd` fails with `ERR memory`),
  `budget_counts_shim_microvms` (`MICROVM launch` takes the room; `vesperd` is refused until `MICROVM cleanup`),
  `budget_whole_again` (`VESPERD mem-budget` as before) and `nothing_left`; `passed` requires all 7 steps ok.
- `control/suites/egress.rs:run_egress_suite` (ADR 0008) ages and reaps its ids, starts `VESPERD test-listen 7777`,
  boots `vesperd --network nat` microVMs with `vesper_net=` and `vesper_probe=` and judges the guest's
  `VESPER-PROBE` lines (`denied`, `judge`) and the `DROPS` counters of `VESPERD net`: `listeners_and_internet`,
  `nat_guest_reaches_public_only` (two leases), `isolation_reports_egress`, `teardown_removes_veths`,
  `root_mangle_alone_after_docker_restart` (`KILL docker` after the launch, `HEALTH` until dockerd is back, then
  `VESPERD test-open-ns ID filter`), `root_raw_drops_sources_outside_the_pool` (`test-open-ns ID all`),
  `flow_cap_drops_excess_in_namespace` and `flow_cap_drops_excess_in_root_alone` (`flood:1.1.1.1:9:1400`, judged by
  `FLOWS id ns= root= cap_ns= cap_root= capdrop_ns= capdrop_root=` of `VESPERD net`: both caps installed at `FLOW_CAP`, the active layer's cap rule dropping between the excess less `FLOOD_SLACK` and the excess, the other rule nothing; `VESPERD flows` before and after teardown),
  `none_lease_has_no_uplink`, `reaper_removes_veth_of_killed_holder` and `listeners_stopped`; the System VM's
  addresses come from `POLICY ... eth0= gw=`; `passed` requires all 12 steps ok.
- `control/suites/replay.rs:ReplayRun` (ADR 0007 slice 6) writes the replay files into a new
  `share_dir/replay-<ms>/` (a removed and rewritten share file can read as empty in the System VM), makes the snapshot
  fixture in its own `vesperd` and copies it into the store with `vesperd stage jail:… store:…`, then runs each case
  in a fresh `vesperd --backend systemvm --id ID` with every file staged (and the fixture linked when the case needs
  it), one connection per step, then SIGTERM and `wait_absent`. It never judges; `systemvmctl` does.
- `control/suites/snapshot.rs:run_snapshot_suite` (ADR 0007 slice 5) ages and reaps its ids (`id-a`, `id-b`, `id-c`,
  `id-m`), clears the reserve override and removes its store names, then runs `handshake`, `data_disk_and_user`,
  `share_files` (writes `d1.img`, `d2.img` and a `link -> /etc/shadow` into `share_dir`), `start_with_share_stage`,
  `stage_into_running_jail`, `boot_with_both_drives` (`vesper_mark=1`), `refuses_symlinked_src`,
  `refuses_symlinked_dest`, `refuses_src_outside_roots`, `jail_side_read_runs_as_uid_20000`,
  `reserve_refuses_breach`, `store_quota_refuses_breach` (`VESPERD test-store-quota` set half a drive above the store's size; the copy is refused with `store quota` and leaves no file), `pause_and_create`, `copy_into_store`, `stop_original`, `new_vesperd_stages_by_link`,
  `load_and_resume`, `guest_state_survives` (the same token, later ticks), `create_onto_linked_file_fails` (CE-0008),
  `stop_restored`, `stage_refused_at_start`, `measure_create_at_default_ceiling` (a 640 MiB guest with 512 MiB
  dirtied, under a tenth of the request ceiling), `store_rm` and `nothing_left`; `passed` requires all 24 steps ok.
  It drives the microVMs through their sockets and `vesperd stage|store`, and uses the control channel to inspect.
- `systemvm/packages.rs:validate_packages_lock` requires schema `2`,
  non-blank branch and arch, every requested package in the closure,
  packages sorted by name without duplicates, `file ==
  <name>-<version>.apk`, an `https://` URL ending in the file name, 64-char
  lowercase hex digests and non-zero sizes; archives need unique names,
  https URLs, digests, sizes, at least one member, absolute `guest_path`s and
  octal modes. `verify_packages(lock, dir)` requires every file in `dir` with
  its digest; `verify_archives(lock, dir)` requires every member under
  `dir/<archive>/` with its digest.

## Side Effects

- `api.rs` mutates `AppState` fields under `std::sync::RwLock` (`vm_state`,
  `boot_source`, `cpu_config`, `logger_config`, `machine_config`,
  `metrics_config`, `serial_config`, `balloon_config`,
  `balloon_hinting_state`, `drives`, `network_interfaces`, `mmds_config`,
  `mmds_data_store`, `pmem_devices`, `memory_hotplug`, `entropy_config`,
  `vsock_config`). Lock acquisitions use `.expect` and panic on poison.
- `api.rs:validate_drive_backing_file` and `api/pmem.rs:validate_pmem_backing_files`
  (called by `InstanceStart`) open the host backing file (read, write unless
  read-only) and drop the handle. `validate_pmem_backing_files` also calls
  `metadata()`.
- `api_matrix.rs`, `api_behavior_matrix.rs`, `baseline.rs`, `compat.rs`,
  `platform.rs`, `spec.rs` read files at caller-supplied paths; the matrix
  `write_*` functions call `create_dir_all` then `fs::write`. Capture
  registry validation checks `Path::exists` on every evidence file.
- `proxy.rs:run_server` creates the listen socket's parent directory, removes
  any existing socket file, binds a `UnixListener`, and spawns one tokio task
  per client (`serve`). `handle_client` connects to the unix, tcp, or
  `<vsock_socket_dir>/<cid>.sock` upstream, runs the preface when one is set, and relays. With
  `bounds.max_connections` a client accepted beyond the cap is dropped at once. `claim_socket` creates a missing
  parent directory with mode 0700, creates `<path>.lock` and removes a stale socket.
- `systemvm/boot_check.rs:run_boot_check` spawns `std::process::Command::new(command)`
  per iteration, kills it on timeout, and writes `log_path` and
  `report_path`. `build_manifest` reads the kernel and rootfs files to
  compute SHA-256 digests.
- `buildinfo.rs` reads `CARGO_PKG_VERSION`, `VESPER_GIT_COMMIT`, and
  `VESPER_BUILD_DATE` at compile time only.

## Verification

- Run: `cargo test`
- Broader: `make test`, `make contract-test`, `make contract-check`
- Matrix freshness: `cargo run --bin baselinectl -- --mode validate-api-matrix`
