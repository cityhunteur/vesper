//! The balloon device: `/balloon`, `/balloon/statistics` and `/balloon/hinting/*`.

use axum::Json;
use axum::body::Bytes;
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::*;

pub(super) const BALLOON_HINTING_STOP_COMMAND: u32 = 0;

pub(super) const BALLOON_HINTING_DONE_COMMAND: u32 = 1;

pub(super) const BALLOON_HINTING_FIRST_RUN_COMMAND: u32 = 2;

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BalloonDeviceConfig {
    pub(super) amount_mib: u32,
    pub(super) deflate_on_oom: bool,
    #[serde(default)]
    pub(super) stats_polling_interval_s: u16,
    #[serde(default)]
    pub(super) free_page_hinting: bool,
    #[serde(default)]
    pub(super) free_page_reporting: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct BalloonSizeUpdate {
    pub(super) amount_mib: u32,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct BalloonStatsIntervalUpdate {
    pub(super) stats_polling_interval_s: u16,
}

pub(super) fn default_acknowledge_on_stop() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct BalloonHintingStartRequest {
    #[serde(default = "default_acknowledge_on_stop")]
    pub(super) acknowledge_on_stop: bool,
}

impl Default for BalloonHintingStartRequest {
    fn default() -> Self {
        Self {
            acknowledge_on_stop: true,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BalloonStatsResponse {
    pub(super) target_pages: u32,
    pub(super) actual_pages: u32,
    pub(super) target_mib: u32,
    pub(super) actual_mib: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) swap_in: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) swap_out: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) major_faults: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) minor_faults: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) free_memory: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) total_memory: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) available_memory: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) disk_caches: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) hugetlb_allocations: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) hugetlb_failures: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) oom_kill: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) alloc_stall: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) async_scan: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) direct_scan: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) async_reclaim: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) direct_reclaim: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BalloonHintingStatusResponse {
    pub(super) host_cmd: u32,
    pub(super) guest_cmd: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct BalloonHintingState {
    pub(super) host_cmd: u32,
    pub(super) guest_cmd: Option<u32>,
    pub(super) next_run_cmd: u32,
    pub(super) acknowledge_on_stop: bool,
}

impl Default for BalloonHintingState {
    fn default() -> Self {
        Self {
            host_cmd: BALLOON_HINTING_STOP_COMMAND,
            guest_cmd: None,
            next_run_cmd: BALLOON_HINTING_FIRST_RUN_COMMAND,
            acknowledge_on_stop: true,
        }
    }
}

pub(super) fn handle_get_balloon(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
) -> Response {
    let balloon = state
        .balloon_config
        .read()
        .expect("balloon config lock poisoned while reading")
        .clone();

    let Some(balloon) = balloon else {
        let vm_state = *state
            .vm_state
            .read()
            .expect("vm state lock poisoned while reading");
        let message = if vm_state == VmState::PreBoot {
            BALLOON_NOT_CONFIGURED_MESSAGE
        } else {
            BALLOON_RUNTIME_NOT_CONFIGURED_MESSAGE
        };
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            message,
            template,
            method.as_str(),
        );
    };

    (StatusCode::OK, Json(balloon)).into_response()
}

pub(super) fn handle_put_balloon(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
    body: Bytes,
) -> Response {
    let payload: BalloonDeviceConfig = match serde_json::from_slice(&body) {
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

    if let Err(message) = validate_balloon_config(state, &payload) {
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
        .balloon_config
        .write()
        .expect("balloon config lock poisoned while writing") = Some(payload.clone());
    *state
        .balloon_hinting_state
        .write()
        .expect("balloon hinting state lock poisoned while writing") =
        payload.free_page_hinting.then(BalloonHintingState::default);

    StatusCode::NO_CONTENT.into_response()
}

pub(super) fn handle_patch_balloon(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
    body: Bytes,
) -> Response {
    let payload: BalloonSizeUpdate = match serde_json::from_slice(&body) {
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

    let machine_config = state
        .machine_config
        .read()
        .expect("machine config lock poisoned while reading")
        .clone();
    let mut balloon = state
        .balloon_config
        .write()
        .expect("balloon config lock poisoned while writing");
    let Some(balloon) = balloon.as_mut() else {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            BALLOON_UPDATE_NOT_CONFIGURED_MESSAGE,
            template,
            method.as_str(),
        );
    };

    if payload.amount_mib as usize > machine_config.mem_size_mib {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            &balloon_update_too_large_message(payload.amount_mib),
            template,
            method.as_str(),
        );
    }

    balloon.amount_mib = payload.amount_mib;
    StatusCode::NO_CONTENT.into_response()
}

pub(super) fn handle_get_balloon_statistics(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
) -> Response {
    let balloon = state
        .balloon_config
        .read()
        .expect("balloon config lock poisoned while reading")
        .clone();
    let Some(balloon) = balloon else {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            BALLOON_RUNTIME_NOT_CONFIGURED_MESSAGE,
            template,
            method.as_str(),
        );
    };

    if balloon.stats_polling_interval_s == 0 {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            BALLOON_STATS_DISABLED_MESSAGE,
            template,
            method.as_str(),
        );
    }

    (
        StatusCode::OK,
        Json(balloon_stats_response_for(balloon.amount_mib)),
    )
        .into_response()
}

pub(super) fn handle_get_balloon_hinting_status(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
) -> Response {
    match validated_balloon_hinting_state(state, method, template) {
        Ok(hinting_state) => (
            StatusCode::OK,
            Json(BalloonHintingStatusResponse {
                host_cmd: hinting_state.host_cmd,
                guest_cmd: hinting_state.guest_cmd,
            }),
        )
            .into_response(),
        Err(response) => response,
    }
}

pub(super) fn handle_patch_balloon_hinting_start(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
    body: Bytes,
) -> Response {
    let payload = if body.is_empty() {
        BalloonHintingStartRequest::default()
    } else {
        match serde_json::from_slice::<BalloonHintingStartRequest>(&body) {
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
        }
    };

    if let Err(response) = ensure_balloon_hinting_enabled(state, method, template) {
        return response;
    }

    let mut hinting_state = state
        .balloon_hinting_state
        .write()
        .expect("balloon hinting state lock poisoned while writing");
    let hinting_state = hinting_state.get_or_insert_with(BalloonHintingState::default);
    hinting_state.host_cmd = hinting_state.next_run_cmd;
    hinting_state.next_run_cmd = hinting_state.next_run_cmd.saturating_add(1);
    hinting_state.acknowledge_on_stop = payload.acknowledge_on_stop;

    StatusCode::NO_CONTENT.into_response()
}

pub(super) fn handle_patch_balloon_hinting_stop(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
) -> Response {
    if let Err(response) = ensure_balloon_hinting_enabled(state, method, template) {
        return response;
    }

    let mut hinting_state = state
        .balloon_hinting_state
        .write()
        .expect("balloon hinting state lock poisoned while writing");
    let hinting_state = hinting_state.get_or_insert_with(BalloonHintingState::default);
    hinting_state.host_cmd = BALLOON_HINTING_DONE_COMMAND;

    StatusCode::NO_CONTENT.into_response()
}

pub(super) fn handle_patch_balloon_statistics(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
    body: Bytes,
) -> Response {
    let payload: BalloonStatsIntervalUpdate = match serde_json::from_slice(&body) {
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

    let mut balloon = state
        .balloon_config
        .write()
        .expect("balloon config lock poisoned while writing");
    let Some(balloon) = balloon.as_mut() else {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            BALLOON_UPDATE_NOT_CONFIGURED_MESSAGE,
            template,
            method.as_str(),
        );
    };

    let stats_were_enabled = balloon.stats_polling_interval_s > 0;
    let stats_should_be_enabled = payload.stats_polling_interval_s > 0;
    if stats_were_enabled != stats_should_be_enabled {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            BALLOON_STATS_STATE_CHANGE_MESSAGE,
            template,
            method.as_str(),
        );
    }

    balloon.stats_polling_interval_s = payload.stats_polling_interval_s;
    StatusCode::NO_CONTENT.into_response()
}

pub(super) fn validate_balloon_config(
    state: &Arc<AppState>,
    config: &BalloonDeviceConfig,
) -> Result<(), &'static str> {
    let machine_config = state
        .machine_config
        .read()
        .expect("machine config lock poisoned while reading");
    if config.amount_mib as usize > machine_config.mem_size_mib {
        return Err(BALLOON_TOO_MANY_PAGES_REQUESTED_MESSAGE);
    }

    Ok(())
}

pub(super) fn balloon_stats_response_for(amount_mib: u32) -> BalloonStatsResponse {
    let amount_pages = amount_mib.saturating_mul(256);
    BalloonStatsResponse {
        target_pages: amount_pages,
        actual_pages: amount_pages,
        target_mib: amount_mib,
        actual_mib: amount_mib,
        ..BalloonStatsResponse::default()
    }
}

pub(super) fn ensure_balloon_hinting_enabled(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
) -> Result<(), Response> {
    let balloon = state
        .balloon_config
        .read()
        .expect("balloon config lock poisoned while reading")
        .clone();
    let Some(balloon) = balloon else {
        return Err(error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            BALLOON_UPDATE_NOT_CONFIGURED_MESSAGE,
            template,
            method.as_str(),
        ));
    };

    if !balloon.free_page_hinting {
        return Err(error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            BALLOON_HINTING_NOT_ENABLED_MESSAGE,
            template,
            method.as_str(),
        ));
    }

    Ok(())
}

pub(super) fn validated_balloon_hinting_state(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
) -> Result<BalloonHintingState, Response> {
    ensure_balloon_hinting_enabled(state, method, template)?;

    Ok(state
        .balloon_hinting_state
        .read()
        .expect("balloon hinting state lock poisoned while reading")
        .clone()
        .unwrap_or_default())
}
