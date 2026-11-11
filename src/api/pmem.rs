use axum::body::Bytes;
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::sync::Arc;

use super::{AppState, RateLimiterConfig, RouteMatch, error_response, reject_runtime_hotplug};

pub(super) const PMEM_ID_PATH_MISMATCH_MESSAGE: &str =
    "The id from the path does not match the id from the body!";
pub(super) const PMEM_ROOT_BLOCK_CONFLICT_MESSAGE: &str = "Pmem config error: Attempt to add pmem as a root device while the root device defined as a block device";
pub(super) const PMEM_ROOT_ALREADY_EXISTS_MESSAGE: &str =
    "Pmem config error: A root pmem device already exist";
pub(super) const PMEM_UPDATE_NOT_FOUND_MESSAGE: &str = "Pmem config error: Unable to patch the pmem device: Failed perform action on device: Device not found Please verify the request arguments.";
pub(super) const PMEM_BACKING_FILE_ZERO_SIZE_MESSAGE: &str =
    "Start microvm error: Cannot create pmem device: Error backing file size is 0";

/// Body of `PUT /pmem/{id}` (Firecracker `PmemConfig`).
#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PmemConfigRequest {
    pub(super) id: String,
    pub(super) path_on_host: String,
    #[serde(default)]
    pub(super) root_device: bool,
    #[serde(default)]
    pub(super) read_only: bool,
    #[serde(default)]
    pub(super) rate_limiter: Option<RateLimiterConfig>,
}

/// Body of `PATCH /pmem/{id}` (Firecracker `PmemDeviceUpdateConfig`, swagger `PartialPmem`).
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct PartialPmemConfigRequest {
    id: String,
    #[serde(default)]
    rate_limiter: Option<RateLimiterConfig>,
}

fn invalid_request(
    state: &AppState,
    route: &RouteMatch<'_>,
    method: &Method,
    message: &str,
) -> Response {
    error_response(
        &state.firecracker_revision,
        StatusCode::BAD_REQUEST,
        "InvalidRequest",
        message,
        &route.template.template,
        method.as_str(),
    )
}

fn pmem_backing_file_access_message(err: &std::io::Error) -> String {
    format!("Start microvm error: Cannot create pmem device: Error accessing backing file: {err}")
}

/// `PUT /pmem/{id}`: stores the config. Firecracker opens the backing file only at
/// `InstanceStart` (see `validate_pmem_backing_files`).
pub(super) fn handle_put_pmem(
    state: &Arc<AppState>,
    method: &Method,
    route: &RouteMatch<'_>,
    body: Bytes,
) -> Response {
    let payload: PmemConfigRequest = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => return invalid_request(state, route, method, "invalid JSON payload"),
    };

    let path_id = route
        .template
        .path_param(&route.path, "id")
        .unwrap_or_default();
    if payload.id != path_id {
        return invalid_request(state, route, method, PMEM_ID_PATH_MISMATCH_MESSAGE);
    }

    if let Some(response) = reject_runtime_hotplug(state, method, &route.template.template) {
        return response;
    }

    if payload.root_device
        && state
            .drives
            .read()
            .expect("drive config lock poisoned while reading")
            .values()
            .any(|drive| drive.is_root_device)
    {
        return invalid_request(state, route, method, PMEM_ROOT_BLOCK_CONFLICT_MESSAGE);
    }

    let mut pmem_devices = state
        .pmem_devices
        .write()
        .expect("pmem config lock poisoned while writing");
    if payload.root_device
        && pmem_devices
            .iter()
            .any(|(id, device)| id != &payload.id && device.root_device)
    {
        return invalid_request(state, route, method, PMEM_ROOT_ALREADY_EXISTS_MESSAGE);
    }

    pmem_devices.insert(payload.id.clone(), payload);

    StatusCode::NO_CONTENT.into_response()
}

/// `PATCH /pmem/{id}`: post-boot rate limiter update. Firecracker looks the device up only
/// when `rate_limiter` is present, and does not write the update back into the config that
/// `GET /vm/config` reports, so the stored config is left unchanged.
pub(super) fn handle_patch_pmem(
    state: &Arc<AppState>,
    method: &Method,
    route: &RouteMatch<'_>,
    body: Bytes,
) -> Response {
    let payload: PartialPmemConfigRequest = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => return invalid_request(state, route, method, "invalid JSON payload"),
    };

    let path_id = route
        .template
        .path_param(&route.path, "id")
        .unwrap_or_default();
    if payload.id != path_id {
        return invalid_request(state, route, method, PMEM_ID_PATH_MISMATCH_MESSAGE);
    }

    if payload.rate_limiter.is_some()
        && !state
            .pmem_devices
            .read()
            .expect("pmem config lock poisoned while reading")
            .contains_key(&payload.id)
    {
        return invalid_request(state, route, method, PMEM_UPDATE_NOT_FOUND_MESSAGE);
    }

    StatusCode::NO_CONTENT.into_response()
}

/// Opens every configured pmem backing file the way Firecracker's `attach_pmem_devices` does at
/// `InstanceStart`, returning the first failure as Firecracker's `StartMicrovm` fault message.
pub(super) fn validate_pmem_backing_files(state: &AppState) -> Result<(), String> {
    let pmem_devices = state
        .pmem_devices
        .read()
        .expect("pmem config lock poisoned while reading");
    for device in pmem_devices.values() {
        let file = OpenOptions::new()
            .read(true)
            .write(!device.read_only)
            .open(&device.path_on_host)
            .map_err(|err| pmem_backing_file_access_message(&err))?;
        let len = file
            .metadata()
            .map_err(|err| pmem_backing_file_access_message(&err))?
            .len();
        if len == 0 {
            return Err(PMEM_BACKING_FILE_ZERO_SIZE_MESSAGE.to_string());
        }
    }
    Ok(())
}
