# GH-162 Firecracker v1.16.0 baseline move: endpoint baseline packet

- pinned Firecracker revision: `d83d72b710361a10294480131377b1b00b163af8` (tag `v1.16.0`)
- previous pin: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: `PATCH /pmem/{id}` (new), `PUT /pmem/{id}`, `PUT /actions` (`InstanceStart` with pmem devices),
  post-boot `PUT /drives/{drive_id}`, `PUT /pmem/{id}` and `PUT /network-interfaces/{iface_id}` (PCI disabled),
  `PUT /network-interfaces/{iface_id}` (`mtu`), `PUT /serial` (`rate_limiter`),
  `PUT /snapshot/load` (`vsock_override`, `clock_realtime`), `PATCH /balloon/hinting/start|stop` (204)
- GitHub source issue: `#162`; refs D-014, CE-0002, TD-029

Line numbers are for the files at the pinned revision
(`https://raw.githubusercontent.com/firecracker-microvm/firecracker/d83d72b710361a10294480131377b1b00b163af8/<path>`).
The behaviour differences from the previous pin come from `git diff 083c4cf d83d72b -- src/vmm/src src/firecracker/src`.

## Source packet

- `src/firecracker/swagger/firecracker.yaml`
  - adds `PATCH /pmem/{id}` (`patchGuestPmemByID`, post-boot only, body `PartialPmem {id, rate_limiter}`, `204`/`400`/default)
  - `PATCH /balloon/hinting/start` and `/stop` document `204` (the controller already returned `204`: `parsed_request.rs:191-195`, `VmmData::Empty`)
  - adds `Pmem.rate_limiter`, `NetworkInterface.mtu` (`minimum: 68`, `maximum: 65535`), `SnapshotLoadParams.vsock_override` and `clock_realtime`, `SerialConfig.rate_limiter` (`TokenBucket`)
- `src/firecracker/src/api_server/request/pmem.rs:40-68`
  - `parse_patch_pmem` deserializes `PmemDeviceUpdateConfig` and rejects a path/body id mismatch with
    `The id from the path does not match the id from the body!` (line 65)
- `src/vmm/src/vmm_config/pmem.rs`
  - lines 26-33: `PmemDeviceUpdateConfig { id, rate_limiter: Option<RateLimiterConfig> }`, `deny_unknown_fields`
  - lines 36-51: `PmemConfig` gains `rate_limiter: Option<RateLimiterConfig>`
  - line 20: `DeviceUpdate` displays `Unable to patch the pmem device: {0} Please verify the request arguments.`
  - lines 67-88: `PmemBuilder::build` stores the config only; it no longer opens the backing file
- `src/vmm/src/rpc_interface.rs`
  - lines 175-176: `VmmActionError::PmemConfig` displays `Pmem config error: {0}` (was `Pmem device error: {0}` at the previous pin)
  - line 514: `UpdatePmemDevice` is rejected pre-boot with `OperationNotSupportedPreBoot`
  - lines 991-1007: `update_pmem_device` looks the device up only when `rate_limiter` is `Some`
  - line 207: `StartMicrovm` displays `Start microvm error: {0}`
  - line 183 and line 318: `Load snapshot error: {0}`, `Failed to restore from snapshot: {0}`
- `src/vmm/src/lib.rs`
  - line 265: `VmmError::FindDeviceError` displays `Failed perform action on device: {0}`
  - lines 376-378: runtime `GET /vm/config` exports `Pmem::config`, which `update_rate_limiter` does not change
- `src/vmm/src/device_manager/mod.rs:110-113`: `FindDeviceError::DeviceNotFound` displays `Device not found`
- `src/vmm/src/builder.rs`
  - lines 78-79: `StartMicrovmError::CreatePmemDevice` displays `Cannot create pmem device: {0}`
  - lines 734-757: `attach_pmem_devices` runs at `InstanceStart` and calls `Pmem::new` for each config
  - lines 429-430 and 486-490: on aarch64 `clock_realtime: true` fails with `clock_realtime is not supported on aarch64.`
- `src/vmm/src/persist.rs:348`: `RestoreFromSnapshotError::Build` displays `Failed to build microVM from snapshot: {0}`
- `src/vmm/src/devices/virtio/pmem/device.rs`
  - lines 40-43: `Error accessing backing file: {0}`, `Error backing file size is 0`
  - lines 183-192: the backing file is opened read plus write unless `read_only`, then its size is checked
- `src/vmm/src/vmm_config/net.rs`: line 28 `mtu: Option<u16>`; lines 68-69 `Could not create the network device: {0}`
- `src/vmm/src/devices/virtio/net/mod.rs:66-67`: `NetError::InvalidMtu` displays `MTU {0} is out of range [68, 65535]`
- `src/vmm/src/devices/virtio/net/device.rs:300-306`: the range check runs after the tap is opened
- `src/vmm/src/vmm_config/serial.rs:13-18`: `SerialConfig.rate_limiter: Option<TokenBucketConfig>`;
  `resources.rs:553` keeps the serial config out of `GET /vm/config`
- `src/vmm/src/vmm_config/snapshot.rs`
  - lines 60-65: `VsockOverride { uds_path }` without `deny_unknown_fields`
  - lines 117-122: `vsock_override` is optional and `clock_realtime` defaults to `false`

- Post-boot device `PUT` (PCI disabled, the default without `--enable-pci`, `src/firecracker/src/main.rs:278, 396`):
  - `request/drive.rs:11-38`, `request/net.rs:11-40`, `request/pmem.rs:11-38`: the body is parsed and the path/body
    id checked first, in every state
  - `rpc_interface.rs:764-781`: the runtime controller sends `InsertBlockDevice`, `InsertPmemDevice` and
    `InsertNetworkDevice` to `Vmm::hotplug_device` (it used to reject them as not supported after boot)
  - `lib.rs:704-717`: `hotplug_device` only logs a dev-preview warning before `DeviceManager::hotplug_device`
  - `device_manager/mod.rs:492-500` and `487-489`: with no PCI segment it returns `VmmActionError::PciNotEnabled`
    before any device is built, so no MTU, root-device or backing-file check runs
  - `rpc_interface.rs:217-218`: `PciNotEnabled` displays `PCI is not enabled`, with no prefix
  - `parsed_request.rs:215-233`: every `VmmActionError` except `MmdsLimitExceeded` is a `400` with that text as
    `fault_message`
  - `builder.rs:211-215`: a booted VM has a PCI segment only when `pci_enabled`; after a snapshot load
    `device_manager/pci_mngr.rs:434-438` takes it from the snapshot's `pci_enabled`

## Derived fault strings

- runtime `PATCH /pmem/{id}` with a `rate_limiter` for an unknown device:
  `Pmem config error: Unable to patch the pmem device: Failed perform action on device: Device not found Please verify the request arguments.`
- `PUT /pmem/{id}` root conflicts: `Pmem config error: Attempt to add pmem as a root device while the root device defined as a block device`
  and `Pmem config error: A root pmem device already exist`
- `InstanceStart` with a missing pmem backing file:
  `Start microvm error: Cannot create pmem device: Error accessing backing file: No such file or directory (os error 2)`
- `InstanceStart` with a zero-sized pmem backing file:
  `Start microvm error: Cannot create pmem device: Error backing file size is 0`
- `PUT /network-interfaces/{iface_id}` with `mtu` 0 to 67:
  `Network config error: Could not create the network device: MTU 67 is out of range [68, 65535]`
- aarch64 `PUT /snapshot/load` with `clock_realtime: true`:
  `Load snapshot error: Failed to restore from snapshot: Failed to build microVM from snapshot: clock_realtime is not supported on aarch64.`

- post-boot (running or paused) `PUT /drives/{drive_id}`, `PUT /pmem/{id}`, `PUT /network-interfaces/{iface_id}`
  with a well-formed body and matching ids: `400` `PCI is not enabled`

## Local parity decision

- `vesperd` answers `PATCH /pmem/{id}` post-boot with `204`, checks the id match, and checks that the device
  exists only when `rate_limiter` is present. It does not change the stored config, because Firecracker's
  `GET /vm/config` does not show the patched limiter.
- `PUT /pmem/{id}` stores the config. `InstanceStart` opens each backing file and returns the first failure.
  `vesperd` checks the devices in id order; Firecracker uses insertion order.
- An `mtu` above 65535 or below 0 fails `u16` deserialization. `vesperd` returns its usual `invalid JSON payload`
  for every deserialization failure.
- Firecracker exits after a failed snapshot restore (`fatal_error`). `vesperd` stays in `Not started`.
- `vesperd` models a Firecracker process started without `--enable-pci`: post-boot device `PUT`s parse the body,
  check the ids and then answer `PCI is not enabled`, leaving the stored config unchanged. Hotplug with PCI
  enabled is not modelled, and neither are the `DELETE` unplug routes, which are not in the pinned swagger.
- Open question: after `PUT /snapshot/load` Firecracker takes PCI from the snapshot file, which `vesperd` does
  not read; `vesperd` assumes the snapshot came from a PCI-disabled process.

## Repo evidence crosswalk

- `tests/api_contract.rs`: `pmem_patch_matches_pinned_behavior_on_full_router`,
  `pmem_backing_files_are_validated_at_instance_start_on_full_router`,
  `pmem_put_accepts_rate_limiter_and_exports_it_on_full_router`, `network_put_validates_mtu_bounds_on_full_router`,
  `serial_put_accepts_token_bucket_rate_limiter_on_full_router`,
  `snapshot_load_accepts_vsock_override_and_clock_realtime_on_full_router`,
  `balloon_hinting_start_and_stop_answer_no_content_on_full_router`, `documented_postboot_only_routes_reject_before_start`,
  `postboot_device_puts_report_pci_not_enabled_on_full_router`
- `src/api/tests.rs`: `pmem_patch_requires_running_vm_and_matching_ids`, `pmem_backing_files_are_validated_at_instance_start`,
  `pmem_put_accepts_and_exports_rate_limiter`, `network_put_validates_mtu_bounds`,
  `serial_put_accepts_token_bucket_rate_limiter`, `snapshot_load_accepts_vsock_override_and_clock_realtime`,
  `snapshot_load_clock_realtime_follows_target_architecture`, `postboot_device_puts_report_pci_not_enabled`
