# Behaviour-matrix corrections from the Backend A replay (ADR 0007 slice 6b, D-026)

- Pinned Firecracker revision: `d83d72b710361a10294480131377b1b00b163af8` (v1.16.0)
- Run: `make systemvm-replay-publish` on an Apple M3 Pro, macOS 27.0.1, 2026-11-12; report
  `docs/generated/backend-a-behavior-replay.json` (stock Firecracker and jailer in the System VM, through `vesperd`)
- Source: a clean checkout at the pinned revision; paths below are relative to its root.

The 33 captures below were written from reading source and never checked against a running Firecracker. In each,
Backend A returned a different answer, and the pinned source gives Backend A's text or rule. None depends on the
host: they are message strings and state rules, so D-026 corrects them without the Linux run.

## Message prefixes (`VmmActionError` is `displaydoc`; `src/vmm/src/rpc_interface.rs`)

| Prefix | Source | Cases |
|---|---|---|
| `MMDS error: ` | `rpc_interface.rs:192` | `GET /mmds#runtime_missing_config`, `PATCH /mmds#missing_store_preboot`, `PATCH /mmds#runtime_missing_config`, `PUT /mmds#runtime_missing_config` |
| `MMMDS config error: ` (Firecracker's typo) | `rpc_interface.rs:194` | `PUT /mmds/config#empty_network_interface_list_preboot`, `#unknown_network_interface_preboot`, `#invalid_ipv4_address_preboot` |
| `MMDS limit exceeded error: ` | `rpc_interface.rs:197` | `PATCH /mmds#oversized_merge_patch`, `PUT /mmds#oversized_store` |
| `Machine config error: ` | `rpc_interface.rs:187` | `PUT /machine-config#shrink_below_balloon_target_preboot`, `PATCH /machine-config#shrink_below_balloon_target_preboot` |
| `Metrics error: ` | `lib.rs:233` | `PUT /metrics#duplicate_init` |
| `Start microvm error: ` | `rpc_interface.rs:207` (`builder.rs:105` `MissingKernelConfig`) | `PUT /actions#instance_start_preboot_missing_kernel` |

## Different text

- Snapshot load after a boot-specific resource: `Load snapshot error: Loading a microVM snapshot not allowed after
  configuring boot-specific resources.` (`rpc_interface.rs:316`, `LoadSnapshotNotAllowed`, raised when `boot_path`
  is set, line 650). Cases `PUT /snapshot/load#blocked_after_{boot_source,memory_hotplug,entropy,vsock,balloon}`,
  and, by the same source line, `#blocked_after_mmds_config` and `#blocked_after_network_interface`, which Backend A
  cannot reach (they need a network interface, CE-0010). That makes 35 corrected captures in all.
- Snapshot create while running: `Create snapshot error: Cannot save the microVM state: Operation not allowed:
  save/restore unavailable while running` (`vstate/vcpu.rs:271`). Case `PUT /snapshot/create#running_requires_paused`.
- Memory hotplug: `Memory hotplug config error: Block size must be a power of 2` (`vmm_config/memory_hotplug.rs:15`;
  the request had `block_size_mib` 3), `Memory hotplug config error: Total size must not be lower than slot size (128
  MiB)` (line 21; `total_size_mib` 100), `Memory hotplug update error: Failed to create memory hotplug device: Size
  3145728 is invalid: it must be a multiple of block size and less than the total size` (`devices/virtio/mem/device.rs:49`;
  `requested_size_mib` 3). Cases `PUT /hotplug/memory#invalid_block_size_preboot`, `#invalid_total_size_preboot`,
  `PATCH /hotplug/memory#invalid_requested_size_runtime`.
- Balloon runtime limit: the message quotes the requested size, not the guest memory (`devices/virtio/balloon/device.rs`
  `update_size`, `TooMuchMemoryRequested(amount_mib)`); a 256 MiB request to a 128 MiB guest gives `Balloon update
  error: Balloon: Requested memory should be less than 256MiB`. Case `PATCH /balloon#oversize_runtime`.

## Different rules

- `PUT /logger`, `PUT /metrics` and `PUT /serial` after boot are refused: `The requested operation is not supported
  after starting the microVM.` (`rpc_interface.rs:845-856`, operations not allowed post-boot). The cases
  `runtime_put_valid` become `postboot_invalid_state`.
- `PUT /cpu-config` does not block a snapshot load: `set_custom_cpu_template` (`rpc_interface.rs:594`) does not set
  `boot_path`. `PUT /snapshot/load#blocked_after_cpu_config` becomes `allowed_after_cpu_config_returns_no_content`.
- `PUT /vsock` accepts `guest_cid` 2: nothing checks a minimum (`vmm_config/vsock.rs`, `VsockUnixBackend::new`).
  `PUT /vsock#invalid_guest_cid_preboot` becomes `low_guest_cid_accepted_preboot`.
- `GET /vm/config` writes `"cpu-config": null`, `"logger": null` and `"metrics": null` (`resources.rs:82-101`,
  `VmmConfig` fields without `skip_serializing_if`; only `serial_config` is `#[serde(skip)]`), and does not export a
  stored CPU template. Cases `GET /vm/config#cpu_config_omitted_preboot`, `#logger_omitted_{preboot,runtime}`,
  `#metrics_omitted_{preboot,runtime}` assert `null`.
