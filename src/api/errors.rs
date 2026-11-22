//! The error surface of the model backend: the envelope, its builder, and every error message. A message lives
//! here, not beside its handler, so that the texts a client can match on are in one file.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use super::{VmState, clean_path};

pub(super) const ACTION_NOT_SUPPORTED_POSTBOOT_MESSAGE: &str =
    "The requested operation is not supported after starting the microVM.";

pub(super) const ACTION_NOT_SUPPORTED_PREBOOT_MESSAGE: &str =
    "The requested operation is not supported before starting the microVM.";

pub(super) const SEND_CTRL_ALT_DEL_AARCH64_MESSAGE: &str =
    "SendCtrlAltDel does not supported on aarch64.";

pub(super) const MISSING_KERNEL_CONFIG_MESSAGE: &str =
    "Start microvm error: Cannot start microvm without kernel configuration.";

pub(super) const METRICS_ALREADY_INITIALIZED_MESSAGE: &str = "Metrics error: Cannot initialize the metrics system due to bad user input: Reinitialization of metrics not allowed.";

pub(super) const SNAPSHOT_CREATE_REQUIRES_PAUSED_MESSAGE: &str = "Create snapshot error: Cannot save the microVM state: Operation not allowed: save/restore unavailable while running";

pub(super) const SNAPSHOT_LOAD_RESOURCE_GUARD_MESSAGE: &str = "Load snapshot error: Loading a microVM snapshot not allowed after configuring boot-specific resources.";

pub(super) const SNAPSHOT_LOAD_MISSING_MEMORY_SOURCE_MESSAGE: &str = "An error occurred when deserializing the json body of a request: missing field: either `mem_backend` or `mem_file_path` is required.";

pub(super) const SNAPSHOT_LOAD_TOO_MANY_MEMORY_SOURCES_MESSAGE: &str = "An error occurred when deserializing the json body of a request: too many fields: either `mem_backend` or `mem_file_path` exclusively is required.";

pub(super) const BALLOON_NOT_CONFIGURED_MESSAGE: &str =
    "Balloon config error: No balloon device found.";

pub(super) const BALLOON_RUNTIME_NOT_CONFIGURED_MESSAGE: &str =
    "Internal VMM error: Failed perform action on device: Device not found";

pub(super) const DRIVE_ID_PATH_MISMATCH_MESSAGE: &str =
    "The id from the path does not match the id from the body!";

pub(super) const DRIVE_ROOT_PMEM_CONFLICT_MESSAGE: &str = "Drive config error: Attempt to add block as a root device while the root device defined as a pmem device";

pub(super) const DRIVE_ROOT_ALREADY_EXISTS_MESSAGE: &str =
    "Drive config error: A root block device already exists!";

pub(super) const DRIVE_UPDATE_NOT_FOUND_MESSAGE: &str = "Drive config error: Unable to patch the block device: Failed perform action on device: Device not found Please verify the request arguments.";

pub(super) const NETWORK_UPDATE_NOT_FOUND_MESSAGE: &str = "Network config error: Unable to update the net device: Failed perform action on device: Device not found";

pub(super) const NETWORK_TAP_OPEN_BUSY_DETAIL: &str = "Resource busy (os error 16)";

pub(super) const PCI_NOT_ENABLED_MESSAGE: &str = "PCI is not enabled";

#[cfg(target_arch = "aarch64")]
pub(super) const SNAPSHOT_LOAD_CLOCK_REALTIME_AARCH64_MESSAGE: &str = "Load snapshot error: Failed to restore from snapshot: Failed to build microVM from snapshot: clock_realtime is not supported on aarch64.";

pub(super) const BALLOON_TOO_MANY_PAGES_REQUESTED_MESSAGE: &str =
    "Balloon config error: Amount of pages requested is too large.";

pub(super) const BALLOON_UPDATE_NOT_CONFIGURED_MESSAGE: &str =
    "Balloon update error: Failed perform action on device: Device not found";

pub(super) const BALLOON_HINTING_NOT_ENABLED_MESSAGE: &str =
    "Balloon update error: Balloon: Attempting to use hinting when not enabled";

pub(super) const BALLOON_STATS_DISABLED_MESSAGE: &str =
    "Internal VMM error: Balloon: Received stats query when stats are disabled.";

pub(super) const BALLOON_STATS_STATE_CHANGE_MESSAGE: &str =
    "Balloon update error: Balloon: Statistics cannot be enabled/disabled after activation.";

pub(super) const MACHINE_CONFIG_INCOMPATIBLE_BALLOON_SIZE_MESSAGE: &str = "Machine config error: The memory size (MiB) is smaller than the previously set balloon device target size.";

pub(super) const MEMORY_HOTPLUG_NOT_CONFIGURED_MESSAGE: &str =
    "Memory hotplug device is not configured.";

pub(super) const MMDS_NOT_INITIALIZED_MESSAGE: &str =
    "MMDS error: The MMDS data store is not initialized.";

pub(super) const MMDS_DATA_STORE_LIMIT_EXCEEDED_MESSAGE: &str =
    "MMDS limit exceeded error: The MMDS patch request doesn't fit.";

pub(super) const MMDS_EMPTY_NETWORK_INTERFACE_LIST_MESSAGE: &str = "MMMDS config error: The list of network interface IDs that allow forwarding MMDS requests is empty.";

pub(super) const MMDS_INVALID_IPV4_ADDRESS_MESSAGE: &str =
    "MMMDS config error: The MMDS IPv4 address is not link local.";

pub(super) const MMDS_INVALID_NETWORK_INTERFACE_ID_MESSAGE: &str = "MMMDS config error: The list of network interface IDs provided contains at least one ID that does not correspond to any existing network interface.";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorEnvelope {
    pub fault_message: String,
    pub code: String,
    pub path: String,
    pub method: String,
    pub baseline_firecracker_revision: String,
}

pub(super) fn network_id_path_mismatch_message(path_iface_id: &str, body_iface_id: &str) -> String {
    format!(
        "The id from the path [{path_iface_id}] does not match the id from the body [{body_iface_id}]!"
    )
}

pub(super) fn network_guest_mac_in_use_message(guest_mac: &str) -> String {
    format!("Network config error: The MAC address is already in use: {guest_mac}")
}

pub(super) fn network_tap_open_message(host_dev_name: &str) -> String {
    format!(
        "Network config error: Could not create the network device: Open tap device failed: Error while creating ifreq structure: {NETWORK_TAP_OPEN_BUSY_DETAIL}. Invalid TUN/TAP Backend provided by {host_dev_name}. Check our documentation on setting up the network devices."
    )
}

pub(super) fn network_invalid_mtu_message(mtu: u16) -> String {
    format!(
        "Network config error: Could not create the network device: MTU {mtu} is out of range [68, 65535]"
    )
}

pub(super) fn drive_backing_file_access_message(err: &std::io::Error, path: &str) -> String {
    format!(
        "Drive config error: Unable to create the virtio block device: Virtio backend error: Error manipulating the backing file: {err} {path}"
    )
}

/// Firecracker quotes the requested amount, not the guest memory (`balloon/device.rs`, `TooMuchMemoryRequested`).
pub(super) fn balloon_update_too_large_message(amount_mib: u32) -> String {
    format!("Balloon update error: Balloon: Requested memory should be less than {amount_mib}MiB")
}

pub(super) fn state_invalid_message(operation: String, state: VmState) -> String {
    format!(
        "operation {operation} is not allowed while instance state is {}",
        state.guard_str()
    )
}

pub(super) fn error_response(
    firecracker_revision: &str,
    status: StatusCode,
    code: &str,
    message: &str,
    path: &str,
    method: &str,
) -> Response {
    let body = ErrorEnvelope {
        fault_message: message.to_string(),
        code: code.to_string(),
        path: clean_path(path),
        method: method.to_ascii_uppercase(),
        baseline_firecracker_revision: firecracker_revision.to_string(),
    };
    (status, Json(body)).into_response()
}

pub(super) const PMEM_ID_PATH_MISMATCH_MESSAGE: &str =
    "The id from the path does not match the id from the body!";
pub(super) const PMEM_ROOT_BLOCK_CONFLICT_MESSAGE: &str = "Pmem config error: Attempt to add pmem as a root device while the root device defined as a block device";
pub(super) const PMEM_ROOT_ALREADY_EXISTS_MESSAGE: &str =
    "Pmem config error: A root pmem device already exist";
pub(super) const PMEM_UPDATE_NOT_FOUND_MESSAGE: &str = "Pmem config error: Unable to patch the pmem device: Failed perform action on device: Device not found Please verify the request arguments.";
pub(super) const PMEM_BACKING_FILE_ZERO_SIZE_MESSAGE: &str =
    "Start microvm error: Cannot create pmem device: Error backing file size is 0";
