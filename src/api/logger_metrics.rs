use axum::body::Bytes;
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use std::fs::OpenOptions;
use std::sync::Arc;

use super::{
    AppState, CpuConfig, LoggerConfigRequest, METRICS_ALREADY_INITIALIZED_MESSAGE, MetricsConfig,
    error_response,
};

pub(super) fn handle_put_logger(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
    body: Bytes,
) -> Response {
    let payload: LoggerConfigRequest = match serde_json::from_slice(&body) {
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

    if let Err(message) = validate_logger_request(&payload) {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            &message,
            template,
            method.as_str(),
        );
    }

    let mut config = state
        .logger_config
        .write()
        .expect("logger config lock poisoned while writing");
    if let Some(log_path) = payload.log_path {
        config.log_path = Some(log_path);
    }
    if let Some(level) = payload.level {
        config.level = level;
    }
    if let Some(show_level) = payload.show_level {
        config.show_level = show_level;
    }
    if let Some(show_log_origin) = payload.show_log_origin {
        config.show_log_origin = show_log_origin;
    }
    if let Some(module) = payload.module {
        config.module = Some(module);
    }

    StatusCode::NO_CONTENT.into_response()
}

pub(super) fn handle_put_cpu_config(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
    body: Bytes,
) -> Response {
    let payload: CpuConfig = match serde_json::from_slice(&body) {
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

    *state
        .cpu_config
        .write()
        .expect("cpu config lock poisoned while writing") = Some(payload);

    StatusCode::NO_CONTENT.into_response()
}

pub(super) fn handle_put_metrics(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
    body: Bytes,
) -> Response {
    let payload: MetricsConfig = match serde_json::from_slice(&body) {
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

    if let Err(message) = validate_metrics_request(&payload) {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            &message,
            template,
            method.as_str(),
        );
    }

    let mut config = state
        .metrics_config
        .write()
        .expect("metrics config lock poisoned while writing");
    if config.is_some() {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            METRICS_ALREADY_INITIALIZED_MESSAGE,
            template,
            method.as_str(),
        );
    }

    *config = Some(payload);
    StatusCode::NO_CONTENT.into_response()
}

fn validate_logger_request(config: &LoggerConfigRequest) -> Result<(), String> {
    if let Some(path) = &config.log_path {
        validate_writable_path(path).map_err(|err| format!("Failed to open target file: {err}"))?;
    }

    Ok(())
}

fn validate_metrics_request(config: &MetricsConfig) -> Result<(), String> {
    validate_writable_path(&config.metrics_path)
        .map_err(|err| format!("Cannot initialize the metrics system due to bad user input: {err}"))
}

fn validate_writable_path(path: &str) -> Result<(), std::io::Error> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map(|_| ())
}
