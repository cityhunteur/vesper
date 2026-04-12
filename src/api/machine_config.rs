use axum::body::Bytes;
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use std::sync::Arc;

use super::{
    AppState, HugePages, MACHINE_CONFIG_INCOMPATIBLE_BALLOON_SIZE_MESSAGE, MAX_SUPPORTED_VCPUS,
    MachineConfig, MachineConfigPatchRequest, MachineConfigPutRequest, error_response,
};

pub(super) fn handle_put_machine_config(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
    body: Bytes,
) -> Response {
    let payload: MachineConfigPutRequest = match serde_json::from_slice(&body) {
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

    let config = MachineConfig::from(payload);
    if let Err(message) = validate_machine_config(&config) {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            message,
            template,
            method.as_str(),
        );
    }

    if let Err(message) = validate_machine_config_against_balloon(state, config.mem_size_mib) {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            message,
            template,
            method.as_str(),
        );
    }

    *state
        .machine_config
        .write()
        .expect("machine config lock poisoned while writing") = config;

    StatusCode::NO_CONTENT.into_response()
}

pub(super) fn handle_patch_machine_config(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
    body: Bytes,
) -> Response {
    let payload: MachineConfigPatchRequest = match serde_json::from_slice(&body) {
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

    if payload.is_empty() {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            "PATCH /machine-config requires at least one field",
            template,
            method.as_str(),
        );
    }

    let current = state
        .machine_config
        .read()
        .expect("machine config lock poisoned while reading")
        .clone();

    let updated = MachineConfig {
        cpu_template: match payload.cpu_template {
            Some(template) => template.into_stored(),
            None => current.cpu_template,
        },
        smt: payload.smt.unwrap_or(current.smt),
        mem_size_mib: payload.mem_size_mib.unwrap_or(current.mem_size_mib),
        track_dirty_pages: payload
            .track_dirty_pages
            .unwrap_or(current.track_dirty_pages),
        vcpu_count: payload.vcpu_count.unwrap_or(current.vcpu_count),
        huge_pages: payload.huge_pages.unwrap_or(current.huge_pages),
    };

    if let Err(message) = validate_machine_config(&updated) {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            message,
            template,
            method.as_str(),
        );
    }

    if let Err(message) = validate_machine_config_against_balloon(state, updated.mem_size_mib) {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            message,
            template,
            method.as_str(),
        );
    }

    *state
        .machine_config
        .write()
        .expect("machine config lock poisoned while writing") = updated;

    StatusCode::NO_CONTENT.into_response()
}

fn validate_machine_config(config: &MachineConfig) -> Result<(), &'static str> {
    #[cfg(target_arch = "aarch64")]
    if config.smt {
        return Err("Enabling simultaneous multithreading is not supported on aarch64.");
    }

    if config.vcpu_count == 0 || config.vcpu_count > MAX_SUPPORTED_VCPUS {
        return Err(
            "The number of vCPUs must be greater than 0, less than 32 and must be 1 or an even number if SMT is enabled.",
        );
    }

    if config.smt && config.vcpu_count > 1 && config.vcpu_count % 2 == 1 {
        return Err(
            "The number of vCPUs must be greater than 0, less than 32 and must be 1 or an even number if SMT is enabled.",
        );
    }

    if config.mem_size_mib == 0
        || matches!(config.huge_pages, HugePages::Huge2M) && config.mem_size_mib % 2 != 0
    {
        return Err(
            "The memory size (MiB) is either 0, or not a multiple of the configured page size.",
        );
    }

    Ok(())
}

fn validate_machine_config_against_balloon(
    state: &Arc<AppState>,
    mem_size_mib: usize,
) -> Result<(), &'static str> {
    let balloon = state
        .balloon_config
        .read()
        .expect("balloon config lock poisoned while reading");
    if balloon
        .as_ref()
        .map(|config| config.amount_mib as usize > mem_size_mib)
        .unwrap_or(false)
    {
        return Err(MACHINE_CONFIG_INCOMPATIBLE_BALLOON_SIZE_MESSAGE);
    }

    Ok(())
}
