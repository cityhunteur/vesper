//! The metadata service: `/mmds` and `/mmds/config`.

use axum::body::Bytes;
use axum::http::{Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::net::Ipv4Addr;
use std::sync::Arc;

use super::*;

pub(super) const MMDS_DATA_STORE_LIMIT_BYTES: usize = 51_200;

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq, Serialize)]
pub(super) enum MmdsVersion {
    #[default]
    V1,
    V2,
}

pub(super) fn default_mmds_ipv4_address() -> Ipv4Addr {
    Ipv4Addr::new(169, 254, 169, 254)
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct MmdsConfig {
    #[serde(default)]
    pub(super) version: MmdsVersion,
    pub(super) network_interfaces: Vec<String>,
    #[serde(default)]
    pub(super) ipv4_address: Option<Ipv4Addr>,
    #[serde(default)]
    pub(super) imds_compat: bool,
}

impl MmdsConfig {
    pub(super) fn normalized(mut self) -> Self {
        if self.ipv4_address.is_none() {
            self.ipv4_address = Some(default_mmds_ipv4_address());
        }
        self
    }
}

pub(super) fn handle_get_mmds(state: &Arc<AppState>, method: &Method, template: &str) -> Response {
    if let Err(response) = ensure_mmds_runtime_access(state, method, template) {
        return response;
    }

    let value = state
        .mmds_data_store
        .read()
        .expect("mmds data store lock poisoned while reading")
        .clone();

    mmds_value_response(value)
}

pub(super) fn handle_patch_mmds(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
    body: Bytes,
) -> Response {
    if let Err(response) = ensure_mmds_runtime_access(state, method, template) {
        return response;
    }

    let payload: Value = match serde_json::from_slice(&body) {
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

    let mut mmds_data_store = state
        .mmds_data_store
        .write()
        .expect("mmds data store lock poisoned while writing");
    let Some(current_value) = mmds_data_store.as_mut() else {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            MMDS_NOT_INITIALIZED_MESSAGE,
            template,
            method.as_str(),
        );
    };

    let mut merged = current_value.clone();
    apply_json_merge_patch(&mut merged, &payload);

    if let Err(message) = validate_mmds_data_store_limit(&merged) {
        return error_response(
            &state.firecracker_revision,
            StatusCode::PAYLOAD_TOO_LARGE,
            "PayloadTooLarge",
            message,
            template,
            method.as_str(),
        );
    }

    *current_value = merged;
    StatusCode::NO_CONTENT.into_response()
}

pub(super) fn handle_put_mmds(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
    body: Bytes,
) -> Response {
    if let Err(response) = ensure_mmds_runtime_access(state, method, template) {
        return response;
    }

    let payload: Value = match serde_json::from_slice(&body) {
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

    if let Err(message) = validate_mmds_data_store_limit(&payload) {
        return error_response(
            &state.firecracker_revision,
            StatusCode::PAYLOAD_TOO_LARGE,
            "PayloadTooLarge",
            message,
            template,
            method.as_str(),
        );
    }

    *state
        .mmds_data_store
        .write()
        .expect("mmds data store lock poisoned while writing") = Some(payload);

    StatusCode::NO_CONTENT.into_response()
}

pub(super) fn handle_put_mmds_config(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
    body: Bytes,
) -> Response {
    let payload: MmdsConfig = match serde_json::from_slice(&body) {
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

    let normalized = payload.normalized();
    if let Err(message) = validate_mmds_config(state, &normalized) {
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
        .mmds_config
        .write()
        .expect("mmds config lock poisoned while writing") = Some(normalized);

    StatusCode::NO_CONTENT.into_response()
}

pub(super) fn validate_mmds_config(state: &Arc<AppState>, config: &MmdsConfig) -> Result<(), &'static str> {
    if config.network_interfaces.is_empty() {
        return Err(MMDS_EMPTY_NETWORK_INTERFACE_LIST_MESSAGE);
    }

    let ipv4_address = config
        .ipv4_address
        .unwrap_or_else(default_mmds_ipv4_address);
    if !is_valid_link_local_mmds_ipv4(ipv4_address) {
        return Err(MMDS_INVALID_IPV4_ADDRESS_MESSAGE);
    }

    let interfaces = state
        .network_interfaces
        .read()
        .expect("network interface config lock poisoned while reading");
    if !config
        .network_interfaces
        .iter()
        .all(|iface_id| interfaces.contains_key(iface_id))
    {
        return Err(MMDS_INVALID_NETWORK_INTERFACE_ID_MESSAGE);
    }

    Ok(())
}

pub(super) fn validate_mmds_data_store_limit(value: &Value) -> Result<(), &'static str> {
    if to_vec(value)
        .map(|payload| payload.len() > MMDS_DATA_STORE_LIMIT_BYTES)
        .unwrap_or(false)
    {
        return Err(MMDS_DATA_STORE_LIMIT_EXCEEDED_MESSAGE);
    }

    Ok(())
}

pub(super) fn ensure_mmds_runtime_access(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
) -> Result<(), Response> {
    let vm_state = *state
        .vm_state
        .read()
        .expect("vm state lock poisoned while reading");
    if vm_state == VmState::PreBoot {
        return Ok(());
    }

    if state
        .mmds_config
        .read()
        .expect("mmds config lock poisoned while reading")
        .is_some()
    {
        Ok(())
    } else {
        Err(error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            MMDS_NOT_INITIALIZED_MESSAGE,
            template,
            method.as_str(),
        ))
    }
}

pub(super) fn is_valid_link_local_mmds_ipv4(ipv4_address: Ipv4Addr) -> bool {
    match ipv4_address.octets() {
        [169, 254, 0, _] => false,
        [169, 254, 255, _] => false,
        [169, 254, _, _] => true,
        _ => false,
    }
}

pub(super) fn apply_json_merge_patch(target: &mut Value, patch: &Value) {
    let Some(patch_object) = patch.as_object() else {
        *target = patch.clone();
        return;
    };

    if !target.is_object() {
        *target = Value::Object(serde_json::Map::new());
    }

    let target_object = target
        .as_object_mut()
        .expect("target must be an object after normalization");
    for (key, value) in patch_object {
        if value.is_null() {
            target_object.remove(key);
            continue;
        }

        let entry = target_object.entry(key.clone()).or_insert(Value::Null);
        apply_json_merge_patch(entry, value);
    }
}

pub(super) fn mmds_value_response(value: Option<Value>) -> Response {
    let body = match value {
        None => "{}".to_string(),
        Some(Value::Null) => "{}".to_string(),
        Some(other) => serde_json::to_string(&other).expect("serialize MMDS value"),
    };

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        body,
    )
        .into_response()
}
