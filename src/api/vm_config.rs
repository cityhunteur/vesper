//! The routes about the machine as a whole: `GET /`, `GET /version`, `GET /vm/config` and `PATCH /vm`.

use axum::Json;
use axum::body::Bytes;
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::*;

#[derive(Debug, Serialize)]
pub(super) struct InstanceInfoResponse {
    app_name: String,
    id: String,
    state: String,
    vmm_version: String,
}

#[derive(Debug, Serialize)]
pub(super) struct FirecrackerVersionResponse {
    firecracker_version: String,
}

/// `GET /vm/config` in Firecracker's shape (`VmmConfig` in `resources.rs` at the pinned revision): every key is
/// present in this order, empty ones as `null` or `[]`; Firecracker never exports the CPU template, the logger or
/// the metrics configuration, and leaves the serial configuration out.
#[derive(Debug, Serialize)]
pub(super) struct FullVmConfigurationResponse {
    balloon: Option<BalloonDeviceConfig>,
    drives: Vec<DriveConfigRequest>,
    #[serde(rename = "boot-source")]
    boot_source: BootSourceConfig,
    #[serde(rename = "cpu-config")]
    cpu_config: Option<CpuConfig>,
    logger: Option<()>,
    #[serde(rename = "machine-config")]
    machine_config: MachineConfig,
    metrics: Option<()>,
    #[serde(rename = "mmds-config")]
    mmds_config: Option<MmdsConfig>,
    #[serde(rename = "network-interfaces")]
    network_interfaces: Vec<NetworkInterfaceConfigRequest>,
    vsock: Option<VsockConfig>,
    entropy: Option<EntropyDeviceConfig>,
    pmem: Vec<PmemConfigRequest>,
    #[serde(rename = "memory-hotplug")]
    memory_hotplug: Option<MemoryHotplugConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct VmStateChangeRequest {
    state: VmStateChange,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub(super) enum VmStateChange {
    Paused,
    Resumed,
}

pub(super) fn instance_info_response(state: &Arc<AppState>) -> Response {
    let vm_state = *state
        .vm_state
        .read()
        .expect("vm state lock poisoned while reading");
    let body = InstanceInfoResponse {
        app_name: DEFAULT_APP_NAME.to_string(),
        id: DEFAULT_INSTANCE_ID.to_string(),
        state: vm_state.instance_str().to_string(),
        vmm_version: state.firecracker_version.clone(),
    };
    (StatusCode::OK, Json(body)).into_response()
}

pub(super) fn firecracker_version_response(state: &Arc<AppState>) -> Response {
    (
        StatusCode::OK,
        Json(FirecrackerVersionResponse {
            firecracker_version: state.firecracker_version.clone(),
        }),
    )
        .into_response()
}

pub(super) fn full_vm_config_response(state: &Arc<AppState>) -> Response {
    let boot_source = state
        .boot_source
        .read()
        .expect("boot source lock poisoned while reading")
        .clone();
    let config = state
        .machine_config
        .read()
        .expect("machine config lock poisoned while reading")
        .clone();
    let balloon = state
        .balloon_config
        .read()
        .expect("balloon config lock poisoned while reading")
        .clone();
    let mut drives: Vec<DriveConfigRequest> = state
        .drives
        .read()
        .expect("drive config lock poisoned while reading")
        .values()
        .cloned()
        .collect();
    if let Some(root_index) = drives.iter().position(|drive| drive.is_root_device)
        && root_index != 0
    {
        let root_drive = drives.remove(root_index);
        drives.insert(0, root_drive);
    }
    let network_interfaces = state
        .network_interfaces
        .read()
        .expect("network interface config lock poisoned while reading")
        .values()
        .cloned()
        .collect();
    let mmds_config = state
        .mmds_config
        .read()
        .expect("mmds config lock poisoned while reading")
        .clone();
    let pmem = state
        .pmem_devices
        .read()
        .expect("pmem config lock poisoned while reading")
        .values()
        .cloned()
        .collect();
    let memory_hotplug = state
        .memory_hotplug
        .read()
        .expect("memory hotplug lock poisoned while reading")
        .as_ref()
        .map(|state| state.config.clone());
    let entropy = state
        .entropy_config
        .read()
        .expect("entropy config lock poisoned while reading")
        .clone();
    let vsock = state
        .vsock_config
        .read()
        .expect("vsock config lock poisoned while reading")
        .clone();
    (
        StatusCode::OK,
        Json(FullVmConfigurationResponse {
            boot_source,
            cpu_config: None,
            logger: None,
            metrics: None,
            machine_config: config,
            balloon,
            drives,
            network_interfaces,
            pmem,
            mmds_config,
            memory_hotplug,
            entropy,
            vsock,
        }),
    )
        .into_response()
}

pub(super) fn handle_patch_vm(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
    body: Bytes,
) -> Response {
    let payload: VmStateChangeRequest = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return error_response(
                &state.firecracker_revision,
                StatusCode::BAD_REQUEST,
                "InvalidRequest",
                "invalid JSON payload",
                template,
                method.as_str(),
            );
        }
    };

    let mut vm_state = state
        .vm_state
        .write()
        .expect("vm state lock poisoned while writing");
    match (payload.state, *vm_state) {
        (_, VmState::PreBoot) => error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidState",
            ACTION_NOT_SUPPORTED_PREBOOT_MESSAGE,
            template,
            method.as_str(),
        ),
        (VmStateChange::Paused, VmState::Running) => {
            *vm_state = VmState::Paused;
            StatusCode::NO_CONTENT.into_response()
        }
        (VmStateChange::Resumed, VmState::Paused) => {
            *vm_state = VmState::Running;
            StatusCode::NO_CONTENT.into_response()
        }
        _ => error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidState",
            &state_invalid_message(operation_key(method.as_str(), template), *vm_state),
            template,
            method.as_str(),
        ),
    }
}
