use axum::body::Bytes;
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use std::fs::File;
use std::sync::Arc;

use super::{AppState, BootSourceConfig, error_response};

pub(super) fn handle_put_boot_source(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
    body: Bytes,
) -> Response {
    let payload: BootSourceConfig = match serde_json::from_slice(&body) {
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

    if let Err(message) = validate_boot_source(&payload) {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            &message,
            template,
            method.as_str(),
        );
    }

    *state
        .boot_source
        .write()
        .expect("boot source lock poisoned while writing") = payload;

    StatusCode::NO_CONTENT.into_response()
}

fn validate_boot_source(config: &BootSourceConfig) -> Result<(), String> {
    File::open(&config.kernel_image_path)
        .map_err(|err| format!("The kernel file cannot be opened: {err}"))?;

    if let Some(path) = &config.initrd_path {
        File::open(path).map_err(|err| {
            format!(
                "The initrd file cannot be opened due to invalid path or invalid permissions. {err}"
            )
        })?;
    }

    Ok(())
}
