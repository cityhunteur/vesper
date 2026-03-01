use axum::body::Bytes;
use axum::extract::State;
use axum::http::{Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorEnvelope {
    pub fault_message: String,
    pub code: String,
    pub path: String,
    pub method: String,
    pub baseline_firecracker_revision: String,
}

#[derive(Debug, Clone)]
pub struct OperationStatus {
    pub status_code: StatusCode,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VmState {
    PreBoot,
    Running,
}

impl VmState {
    fn as_str(self) -> &'static str {
        match self {
            VmState::PreBoot => "preboot",
            VmState::Running => "running",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RouteSpec {
    pub method: String,
    pub path: String,
}

#[derive(Debug, Clone)]
struct RouteTemplate {
    method: String,
    template: String,
    parts: Vec<RoutePart>,
}

#[derive(Debug, Clone)]
enum RoutePart {
    Literal(String),
    Param,
}

#[derive(Debug)]
pub struct AppState {
    routes: Vec<RouteTemplate>,
    matrix: BTreeMap<String, OperationStatus>,
    firecracker_revision: String,
    vm_state: RwLock<VmState>,
}

impl AppState {
    pub fn new(routes: &[RouteSpec], firecracker_revision: String) -> Result<Self, String> {
        if routes.is_empty() {
            return Err("at least one route is required".to_string());
        }
        if firecracker_revision.trim().is_empty() {
            return Err("firecracker revision is required".to_string());
        }

        let mut unique = HashMap::new();
        let mut templates = Vec::new();
        for route in routes {
            let template = RouteTemplate::new(&route.method, &route.path);
            let key = operation_key(&template.method, &template.template);
            if unique.contains_key(&key) {
                continue;
            }
            unique.insert(key, ());
            templates.push(template);
        }

        let mut matrix = BTreeMap::new();
        for template in &templates {
            matrix.insert(
                operation_key(&template.method, &template.template),
                OperationStatus {
                    status_code: StatusCode::NOT_IMPLEMENTED,
                    code: "NotImplemented".to_string(),
                    message: "operation not implemented".to_string(),
                },
            );
        }

        Ok(Self {
            routes: templates,
            matrix,
            firecracker_revision,
            vm_state: RwLock::new(VmState::PreBoot),
        })
    }

    pub fn router(self) -> Router {
        Router::new().fallback(any(handle_all)).with_state(Arc::new(self))
    }
}

impl RouteTemplate {
    fn new(method: &str, template: &str) -> Self {
        let method = method.trim().to_ascii_uppercase();
        let template = clean_path(template);
        let parts = split_path(&template)
            .iter()
            .map(|part| {
                if part.starts_with('{') && part.ends_with('}') {
                    RoutePart::Param
                } else {
                    RoutePart::Literal(part.to_string())
                }
            })
            .collect();

        Self {
            method,
            template,
            parts,
        }
    }

    fn matches(&self, method: &Method, path: &str) -> bool {
        if self.method != method.as_str().to_ascii_uppercase() {
            return false;
        }

        let incoming_path = clean_path(path);
        let incoming = split_path(&incoming_path);
        if incoming.len() != self.parts.len() {
            return false;
        }

        for (part, incoming_segment) in self.parts.iter().zip(incoming.iter()) {
            match part {
                RoutePart::Param => {
                    if incoming_segment.is_empty() {
                        return false;
                    }
                }
                RoutePart::Literal(expected) => {
                    if expected != incoming_segment {
                        return false;
                    }
                }
            }
        }

        true
    }
}

async fn handle_all(
    State(state): State<Arc<AppState>>,
    method: Method,
    uri: Uri,
    body: Bytes,
) -> Response {
    let path = clean_path(uri.path());

    let route = state
        .routes
        .iter()
        .find(|route| route.matches(&method, &path));

    let Some(route) = route else {
        return error_response(
            &state.firecracker_revision,
            StatusCode::NOT_FOUND,
            "NotFound",
            "route not found",
            &path,
            method.as_str(),
        );
    };

    if route.method == "PUT" && route.template == "/actions" {
        return handle_actions(&state, &method, &route.template, body);
    }

    if let Some(message) = validate_state_guard(&state, route) {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidState",
            &message,
            &route.template,
            &route.method,
        );
    }

    let key = operation_key(&route.method, &route.template);
    let status = state.matrix.get(&key);
    let Some(status) = status else {
        return error_response(
            &state.firecracker_revision,
            StatusCode::INTERNAL_SERVER_ERROR,
            "InternalMatrixError",
            "operation status not found",
            &route.template,
            &route.method,
        );
    };

    error_response(
        &state.firecracker_revision,
        status.status_code,
        &status.code,
        &status.message,
        &route.template,
        &route.method,
    )
}

fn handle_actions(state: &Arc<AppState>, method: &Method, template: &str, body: Bytes) -> Response {
    #[derive(Deserialize)]
    struct ActionRequest {
        action_type: String,
    }

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
            )
        }
    };

    if payload.action_type.trim() == "InstanceStart" {
        let mut vm_state = state
            .vm_state
            .write()
            .expect("vm state lock poisoned while writing");

        if *vm_state != VmState::PreBoot {
            return error_response(
                &state.firecracker_revision,
                StatusCode::BAD_REQUEST,
                "InvalidState",
                &state_invalid_message(operation_key(method.as_str(), template), *vm_state),
                template,
                method.as_str(),
            );
        }

        *vm_state = VmState::Running;
        return StatusCode::NO_CONTENT.into_response();
    }

    error_response(
        &state.firecracker_revision,
        StatusCode::NOT_IMPLEMENTED,
        "NotImplemented",
        "action is not implemented",
        template,
        method.as_str(),
    )
}

fn validate_state_guard(state: &Arc<AppState>, route: &RouteTemplate) -> Option<String> {
    const PREBOOT_ONLY_OPS: &[&str] = &[
        "PUT /boot-source",
        "PUT /machine-config",
        "PUT /drives/{drive_id}",
        "PATCH /drives/{drive_id}",
        "PUT /network-interfaces/{iface_id}",
        "PUT /vsock",
        "PUT /cpu-config",
        "PUT /entropy",
        "PUT /balloon",
        "PUT /balloon/statistics",
        "PUT /snapshot/load",
        "PATCH /snapshot/load",
        "PUT /snapshot/create",
    ];

    let key = operation_key(&route.method, &route.template);
    if !PREBOOT_ONLY_OPS.contains(&key.as_str()) {
        return None;
    }

    let vm_state = *state
        .vm_state
        .read()
        .expect("vm state lock poisoned while reading");

    if vm_state == VmState::PreBoot {
        None
    } else {
        Some(state_invalid_message(key, vm_state))
    }
}

fn state_invalid_message(operation: String, state: VmState) -> String {
    format!(
        "operation {operation} is not allowed while instance state is {}",
        state.as_str()
    )
}

fn error_response(
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

fn operation_key(method: &str, path: &str) -> String {
    format!("{} {}", method.trim().to_ascii_uppercase(), clean_path(path))
}

fn clean_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return "/".to_string();
    }

    let with_leading = if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    };

    if with_leading.len() > 1 {
        with_leading.trim_end_matches('/').to_string()
    } else {
        with_leading
    }
}

fn split_path(path: &str) -> Vec<&str> {
    if path == "/" {
        return Vec::new();
    }
    path.trim_start_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::Request;
    use tower::ServiceExt;

    fn test_state() -> AppState {
        AppState::new(
            &[
                RouteSpec {
                    method: "PUT".to_string(),
                    path: "/actions".to_string(),
                },
                RouteSpec {
                    method: "PUT".to_string(),
                    path: "/boot-source".to_string(),
                },
            ],
            "abc123".to_string(),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn transitions_to_running_on_instance_start() {
        let router = test_state().router();
        let response = router
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/actions")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"action_type":"InstanceStart"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn rejects_preboot_mutation_after_start() {
        let router = test_state().router();

        let _ = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/actions")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"action_type":"InstanceStart"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        let response = router
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/boot-source")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload.code, "InvalidState");
    }

    #[tokio::test]
    async fn returns_not_found_for_unknown_route() {
        let router = test_state().router();
        let response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/unknown")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
