# api (route-family handler modules)

## Overview

Private submodules of `src/api.rs` that implement individual Firecracker route
families. Each module exposes `pub(super)` handler functions that the parent
dispatcher `handle_all` in `../api.rs` calls after route matching. These
modules never bind sockets or build routers; they deserialize a request body,
validate it against the Firecracker-compatible rules, mutate `AppState`
(defined in `../api.rs`), and return either `204 No Content` or a deterministic
`ErrorEnvelope` built by `error_response` in `../api.rs`.

`tests.rs` in this directory is test code only and is not part of the contract.

## Interface

All handlers share the signature
`fn(state: &Arc<AppState>, method: &Method, template: &str, body: Bytes) -> Response`.

- `actions.rs:handle_actions` serves `PUT /actions`. Matches `ActionType`
  (`FlushMetrics`, `InstanceStart`, `SendCtrlAltDel`); `InstanceStart` moves
  `vm_state` from `PreBoot` to `Running`.
- `boot_source.rs:handle_put_boot_source` serves `PUT /boot-source`. Stores a
  validated `BootSourceConfig`.
- `boot_source.rs:validate_boot_source` (private) opens `kernel_image_path`
  and optional `initrd_path` with `std::fs::File::open`.
- `logger_metrics.rs:handle_put_logger` serves `PUT /logger`. Merges
  `LoggerConfigRequest` fields that are `Some` into `state.logger_config`.
- `logger_metrics.rs:handle_put_cpu_config` serves `PUT /cpu-config`. Stores
  `CpuConfig` without semantic validation.
- `logger_metrics.rs:handle_put_metrics` serves `PUT /metrics`. One-shot
  initialization of `state.metrics_config`.
- `logger_metrics.rs:validate_writable_path` (private) opens the target path
  with `OpenOptions::new().create(true).append(true)`.
- `logger_metrics.rs:validate_logger_request` and
  `logger_metrics.rs:validate_metrics_request` (private) wrap
  `validate_writable_path` and produce the error prefixes listed below.
- `machine_config.rs:handle_put_machine_config` serves `PUT /machine-config`.
  Converts `MachineConfigPutRequest` via `MachineConfig::from` and replaces
  `state.machine_config`.
- `machine_config.rs:handle_patch_machine_config` serves
  `PATCH /machine-config`. Merges `MachineConfigPatchRequest` onto a clone of
  the current config and validates the merged result before storing.
- `machine_config.rs:validate_machine_config` and
  `machine_config.rs:validate_machine_config_against_balloon` (private)
  enforce the vCPU, memory, SMT, and balloon-size rules listed below.
- `pmem.rs:handle_put_pmem` serves `PUT /pmem/{id}`. Stores a
  `PmemConfigRequest` (`id`, `path_on_host`, `root_device`, `read_only`,
  `rate_limiter`) keyed by id; it does not open the backing file.
- `pmem.rs:handle_patch_pmem` serves `PATCH /pmem/{id}`. Validates a
  `PartialPmemConfigRequest` (`id`, optional `rate_limiter`) and returns
  `204`; it does not change the stored config.
- `pmem.rs:validate_pmem_backing_files` is called by `handle_actions` for
  `InstanceStart` and opens every stored pmem backing file.
- `pmem.rs:PmemConfigRequest` is also the element type of
  `AppState::pmem_devices` and of the `pmem` block of `GET /vm/config`.

Subdirectories: none.

## Invariants

- Every handler rejects an undeserializable body with
  `400 InvalidRequest "invalid JSON payload"` before touching `AppState`.
- No module in this directory applies a `VmState` guard except `handle_actions`
  and `handle_put_pmem`, which calls `reject_runtime_hotplug` in `../api.rs`
  after the id check: after boot it returns `400 InvalidRequest`
  `"PCI is not enabled"` (Firecracker without `--enable-pci`). `handle_all`
  dispatches `PUT /pmem/{id}` before `validate_state_guard` for that reason.
  `PATCH /pmem/{id}` is post-boot only through its `PostBoot` semantics in
  `../api_matrix.rs:documented_route_semantics`.
  Pre-boot and post-boot gating for the other routes is applied by
  `validate_state_guard` in `../api.rs` before dispatch.
- `handle_actions`: `FlushMetrics` and x86_64 `SendCtrlAltDel` return
  `400 InvalidState` with `ACTION_NOT_SUPPORTED_PREBOOT_MESSAGE` while
  `PreBoot`. `InstanceStart` returns `400 InvalidState` with
  `ACTION_NOT_SUPPORTED_POSTBOOT_MESSAGE` unless `PreBoot`, then
  `400 InvalidRequest` with `MISSING_KERNEL_CONFIG_MESSAGE` if
  `boot_source.kernel_image_path` is empty, then `400 InvalidRequest` with
  the first `validate_pmem_backing_files` failure; `vm_state` stays
  `PreBoot` on any failure. On aarch64, `SendCtrlAltDel`
  always returns `400 InvalidRequest` with `SEND_CTRL_ALT_DEL_AARCH64_MESSAGE`.
- `validate_boot_source`: kernel path must open for reading, else
  `"The kernel file cannot be opened: {err}"`. An `initrd_path` that is `Some`
  must also open, else
  `"The initrd file cannot be opened due to invalid path or invalid permissions. {err}"`.
- `handle_put_logger`: a provided `log_path` must be creatable or appendable,
  else `400 InvalidRequest "Failed to open target file: {err}"`.
- `handle_put_metrics`: `metrics_path` must be creatable or appendable, else
  `"Cannot initialize the metrics system due to bad user input: {err}"`.
  A second `PUT /metrics` returns `METRICS_ALREADY_INITIALIZED_MESSAGE`.
  Path validation runs before the already-initialized check.
- `handle_patch_machine_config`: a body with all fields `None` returns
  `400 InvalidRequest "PATCH /machine-config requires at least one field"`
  (`MachineConfigPatchRequest::is_empty`).
- `validate_machine_config`: `vcpu_count == 0 || vcpu_count > MAX_SUPPORTED_VCPUS`
  (32), or `smt` with an odd `vcpu_count > 1`, returns the message
  `"The number of vCPUs must be greater than 0, less than 32 and must be 1 or an even number if SMT is enabled."`.
  `mem_size_mib == 0`, or `huge_pages == Huge2M` with odd `mem_size_mib`,
  returns
  `"The memory size (MiB) is either 0, or not a multiple of the configured page size."`.
  On aarch64, `smt == true` returns
  `"Enabling simultaneous multithreading is not supported on aarch64."`.
- `validate_machine_config_against_balloon`: if `state.balloon_config` is set
  and `amount_mib > mem_size_mib`, both PUT and PATCH return
  `MACHINE_CONFIG_INCOMPATIBLE_BALLOON_SIZE_MESSAGE`. Config is written only
  after both validators pass.

- `handle_put_pmem`: path/body id mismatch returns
  `PMEM_ID_PATH_MISMATCH_MESSAGE`; `root_device` with a block root drive
  returns `PMEM_ROOT_BLOCK_CONFLICT_MESSAGE`; `root_device` with another pmem
  root returns `PMEM_ROOT_ALREADY_EXISTS_MESSAGE`. Same-id PUT replaces.
- `handle_patch_pmem`: path/body id mismatch returns
  `PMEM_ID_PATH_MISMATCH_MESSAGE`; a `rate_limiter` for an id that has no
  stored device returns `PMEM_UPDATE_NOT_FOUND_MESSAGE`; without
  `rate_limiter` it returns `204` for any id, as Firecracker does.
- `validate_pmem_backing_files`: devices are checked in id order. An open
  failure returns
  `"Start microvm error: Cannot create pmem device: Error accessing backing file: {err}"`;
  an empty file returns `PMEM_BACKING_FILE_ZERO_SIZE_MESSAGE`.

## Side Effects

- `handle_actions` takes the `state.vm_state` write lock for `InstanceStart`
  and sets it to `VmState::Running`; reads `state.boot_source` under that lock.
- `validate_boot_source` opens the kernel and initrd files read-only; handles
  are dropped immediately.
- `validate_writable_path` creates the log or metrics file on disk if it does
  not exist (create + append open); nothing is written.
- `handle_put_boot_source` replaces `state.boot_source` wholesale.
- `handle_put_logger` merges into `state.logger_config`.
- `handle_put_cpu_config` overwrites `state.cpu_config` with `Some(payload)`.
- `handle_put_metrics` sets `state.metrics_config` only when it was `None`.
- `handle_put_machine_config` and `handle_patch_machine_config` replace
  `state.machine_config`; both read `state.balloon_config`.
- `handle_put_pmem` reads `state.drives` and writes `state.pmem_devices`;
  `handle_patch_pmem` only reads `state.pmem_devices`.
- `validate_pmem_backing_files` opens each pmem backing file (read, write
  unless `read_only`) and reads its metadata; handles are dropped
  immediately.
- All `RwLock` acquisitions use `.expect(...)` and panic on a poisoned lock.

## Verification

- Run: `cargo test`
- Narrow: `cargo test api::`
