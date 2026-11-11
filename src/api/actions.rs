use axum::body::Bytes;
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use std::sync::Arc;

use super::{
    ACTION_NOT_SUPPORTED_POSTBOOT_MESSAGE, ACTION_NOT_SUPPORTED_PREBOOT_MESSAGE, ActionRequest,
    ActionType, AppState, MISSING_KERNEL_CONFIG_MESSAGE, SEND_CTRL_ALT_DEL_AARCH64_MESSAGE,
    VmState, error_response,
};
use super::pmem::validate_pmem_backing_files;

pub(super) fn handle_actions(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
    body: Bytes,
) -> Response {
    let payload: ActionRequest = match serde_json::from_slice(&body) {
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

    match payload.action_type {
        ActionType::FlushMetrics => {
            let vm_state = *state
                .vm_state
                .read()
                .expect("vm state lock poisoned while reading");
            if vm_state == VmState::PreBoot {
                return error_response(
                    &state.firecracker_revision,
                    StatusCode::BAD_REQUEST,
                    "InvalidState",
                    ACTION_NOT_SUPPORTED_PREBOOT_MESSAGE,
                    template,
                    method.as_str(),
                );
            }

            StatusCode::NO_CONTENT.into_response()
        }
        ActionType::InstanceStart => {
            let mut vm_state = state
                .vm_state
                .write()
                .expect("vm state lock poisoned while writing");

            if *vm_state != VmState::PreBoot {
                return error_response(
                    &state.firecracker_revision,
                    StatusCode::BAD_REQUEST,
                    "InvalidState",
                    ACTION_NOT_SUPPORTED_POSTBOOT_MESSAGE,
                    template,
                    method.as_str(),
                );
            }

            let boot_source = state
                .boot_source
                .read()
                .expect("boot source lock poisoned while reading")
                .clone();
            if boot_source.kernel_image_path.is_empty() {
                return error_response(
                    &state.firecracker_revision,
                    StatusCode::BAD_REQUEST,
                    "InvalidRequest",
                    MISSING_KERNEL_CONFIG_MESSAGE,
                    template,
                    method.as_str(),
                );
            }

            if let Err(message) = validate_pmem_backing_files(state) {
                return error_response(
                    &state.firecracker_revision,
                    StatusCode::BAD_REQUEST,
                    "InvalidRequest",
                    &message,
                    template,
                    method.as_str(),
                );
            }

            *vm_state = VmState::Running;
            StatusCode::NO_CONTENT.into_response()
        }
        ActionType::SendCtrlAltDel => {
            #[cfg(target_arch = "aarch64")]
            {
                error_response(
                    &state.firecracker_revision,
                    StatusCode::BAD_REQUEST,
                    "InvalidRequest",
                    SEND_CTRL_ALT_DEL_AARCH64_MESSAGE,
                    template,
                    method.as_str(),
                )
            }

            #[cfg(target_arch = "x86_64")]
            {
                let vm_state = *state
                    .vm_state
                    .read()
                    .expect("vm state lock poisoned while reading");
                if vm_state == VmState::PreBoot {
                    return error_response(
                        &state.firecracker_revision,
                        StatusCode::BAD_REQUEST,
                        "InvalidState",
                        ACTION_NOT_SUPPORTED_PREBOOT_MESSAGE,
                        template,
                        method.as_str(),
                    );
                }

                StatusCode::NO_CONTENT.into_response()
            }
        }
    }
}
