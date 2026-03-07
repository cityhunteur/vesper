use axum::body::Bytes;
use axum::extract::State;
use axum::http::{Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, RwLock};

const DEFAULT_APP_NAME: &str = "Firecracker";
const DEFAULT_INSTANCE_ID: &str = "anonymous-instance";
const DEFAULT_MEM_SIZE_MIB: usize = 128;
const MAX_SUPPORTED_VCPUS: u8 = 32;

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

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VmState {
    PreBoot,
    Running,
    Paused,
}

impl VmState {
    fn guard_str(self) -> &'static str {
        match self {
            VmState::PreBoot => "preboot",
            VmState::Running => "running",
            VmState::Paused => "paused",
        }
    }

    fn instance_str(self) -> &'static str {
        match self {
            VmState::PreBoot => "Not started",
            VmState::Running => "Running",
            VmState::Paused => "Paused",
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
enum HugePages {
    #[default]
    #[serde(rename = "None")]
    None,
    #[serde(rename = "2M")]
    Huge2M,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
enum CpuTemplate {
    #[cfg(target_arch = "x86_64")]
    C3,
    #[cfg(target_arch = "x86_64")]
    T2,
    #[cfg(target_arch = "x86_64")]
    T2S,
    #[cfg(target_arch = "x86_64")]
    T2CL,
    #[cfg(target_arch = "x86_64")]
    T2A,
    #[cfg(target_arch = "aarch64")]
    V1N1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
enum CpuTemplateRequest {
    None,
    #[cfg(target_arch = "x86_64")]
    C3,
    #[cfg(target_arch = "x86_64")]
    T2,
    #[cfg(target_arch = "x86_64")]
    T2S,
    #[cfg(target_arch = "x86_64")]
    T2CL,
    #[cfg(target_arch = "x86_64")]
    T2A,
    #[cfg(target_arch = "aarch64")]
    V1N1,
}

impl CpuTemplateRequest {
    fn into_stored(self) -> Option<CpuTemplate> {
        match self {
            CpuTemplateRequest::None => None,
            #[cfg(target_arch = "x86_64")]
            CpuTemplateRequest::C3 => Some(CpuTemplate::C3),
            #[cfg(target_arch = "x86_64")]
            CpuTemplateRequest::T2 => Some(CpuTemplate::T2),
            #[cfg(target_arch = "x86_64")]
            CpuTemplateRequest::T2S => Some(CpuTemplate::T2S),
            #[cfg(target_arch = "x86_64")]
            CpuTemplateRequest::T2CL => Some(CpuTemplate::T2CL),
            #[cfg(target_arch = "x86_64")]
            CpuTemplateRequest::T2A => Some(CpuTemplate::T2A),
            #[cfg(target_arch = "aarch64")]
            CpuTemplateRequest::V1N1 => Some(CpuTemplate::V1N1),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct MachineConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    cpu_template: Option<CpuTemplate>,
    smt: bool,
    mem_size_mib: usize,
    track_dirty_pages: bool,
    vcpu_count: u8,
    huge_pages: HugePages,
}

impl Default for MachineConfig {
    fn default() -> Self {
        Self {
            cpu_template: None,
            smt: false,
            mem_size_mib: DEFAULT_MEM_SIZE_MIB,
            track_dirty_pages: false,
            vcpu_count: 1,
            huge_pages: HugePages::None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MachineConfigPutRequest {
    vcpu_count: u8,
    mem_size_mib: usize,
    #[serde(default)]
    smt: bool,
    #[serde(default)]
    cpu_template: Option<CpuTemplateRequest>,
    #[serde(default)]
    track_dirty_pages: bool,
    #[serde(default)]
    huge_pages: HugePages,
}

impl From<MachineConfigPutRequest> for MachineConfig {
    fn from(value: MachineConfigPutRequest) -> Self {
        Self {
            cpu_template: value.cpu_template.and_then(CpuTemplateRequest::into_stored),
            smt: value.smt,
            mem_size_mib: value.mem_size_mib,
            track_dirty_pages: value.track_dirty_pages,
            vcpu_count: value.vcpu_count,
            huge_pages: value.huge_pages,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct MachineConfigPatchRequest {
    #[serde(default)]
    vcpu_count: Option<u8>,
    #[serde(default)]
    mem_size_mib: Option<usize>,
    #[serde(default)]
    smt: Option<bool>,
    #[serde(default)]
    cpu_template: Option<CpuTemplateRequest>,
    #[serde(default)]
    track_dirty_pages: Option<bool>,
    #[serde(default)]
    huge_pages: Option<HugePages>,
}

impl MachineConfigPatchRequest {
    fn is_empty(&self) -> bool {
        self.vcpu_count.is_none()
            && self.mem_size_mib.is_none()
            && self.smt.is_none()
            && self.cpu_template.is_none()
            && self.track_dirty_pages.is_none()
            && self.huge_pages.is_none()
    }
}

#[derive(Debug, Serialize)]
struct InstanceInfoResponse {
    app_name: String,
    id: String,
    state: String,
    vmm_version: String,
}

#[derive(Debug, Serialize)]
struct FirecrackerVersionResponse {
    firecracker_version: String,
}

#[derive(Debug, Serialize)]
struct FullVmConfigurationResponse {
    #[serde(rename = "machine-config")]
    machine_config: MachineConfig,
}

#[derive(Debug)]
pub struct AppState {
    routes: Vec<RouteTemplate>,
    matrix: BTreeMap<String, OperationStatus>,
    firecracker_revision: String,
    firecracker_version: String,
    vm_state: RwLock<VmState>,
    machine_config: RwLock<MachineConfig>,
}

impl AppState {
    pub fn new(
        routes: &[RouteSpec],
        firecracker_revision: String,
        firecracker_version: String,
    ) -> Result<Self, String> {
        if routes.is_empty() {
            return Err("at least one route is required".to_string());
        }
        if firecracker_revision.trim().is_empty() {
            return Err("firecracker revision is required".to_string());
        }
        if firecracker_version.trim().is_empty() {
            return Err("firecracker version is required".to_string());
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
            firecracker_version,
            vm_state: RwLock::new(VmState::PreBoot),
            machine_config: RwLock::new(MachineConfig::default()),
        })
    }

    pub fn router(self) -> Router {
        Router::new()
            .fallback(any(handle_all))
            .with_state(Arc::new(self))
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

    match (route.method.as_str(), route.template.as_str()) {
        ("GET", "/") => return instance_info_response(&state),
        ("GET", "/version") => return firecracker_version_response(&state),
        ("GET", "/machine-config") => return machine_config_response(&state),
        ("PUT", "/machine-config") => {
            return handle_put_machine_config(&state, &method, &route.template, body);
        }
        ("PATCH", "/machine-config") => {
            return handle_patch_machine_config(&state, &method, &route.template, body);
        }
        ("GET", "/vm/config") => return full_vm_config_response(&state),
        _ => {}
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

fn instance_info_response(state: &Arc<AppState>) -> Response {
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

fn firecracker_version_response(state: &Arc<AppState>) -> Response {
    (
        StatusCode::OK,
        Json(FirecrackerVersionResponse {
            firecracker_version: state.firecracker_version.clone(),
        }),
    )
        .into_response()
}

fn machine_config_response(state: &Arc<AppState>) -> Response {
    let config = state
        .machine_config
        .read()
        .expect("machine config lock poisoned while reading")
        .clone();
    (StatusCode::OK, Json(config)).into_response()
}

fn full_vm_config_response(state: &Arc<AppState>) -> Response {
    let config = state
        .machine_config
        .read()
        .expect("machine config lock poisoned while reading")
        .clone();
    (
        StatusCode::OK,
        Json(FullVmConfigurationResponse {
            machine_config: config,
        }),
    )
        .into_response()
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
            );
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

fn handle_put_machine_config(
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

    *state
        .machine_config
        .write()
        .expect("machine config lock poisoned while writing") = config;

    StatusCode::NO_CONTENT.into_response()
}

fn handle_patch_machine_config(
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

    *state
        .machine_config
        .write()
        .expect("machine config lock poisoned while writing") = updated;

    StatusCode::NO_CONTENT.into_response()
}

fn validate_state_guard(state: &Arc<AppState>, route: &RouteTemplate) -> Option<String> {
    const PREBOOT_ONLY_OPS: &[&str] = &[
        "PUT /boot-source",
        "PUT /machine-config",
        "PATCH /machine-config",
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

fn state_invalid_message(operation: String, state: VmState) -> String {
    format!(
        "operation {operation} is not allowed while instance state is {}",
        state.guard_str()
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
    format!(
        "{} {}",
        method.trim().to_ascii_uppercase(),
        clean_path(path)
    )
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
                RouteSpec {
                    method: "GET".to_string(),
                    path: "/".to_string(),
                },
                RouteSpec {
                    method: "GET".to_string(),
                    path: "/version".to_string(),
                },
                RouteSpec {
                    method: "GET".to_string(),
                    path: "/machine-config".to_string(),
                },
                RouteSpec {
                    method: "PUT".to_string(),
                    path: "/machine-config".to_string(),
                },
                RouteSpec {
                    method: "PATCH".to_string(),
                    path: "/machine-config".to_string(),
                },
                RouteSpec {
                    method: "GET".to_string(),
                    path: "/vm/config".to_string(),
                },
            ],
            "abc123".to_string(),
            "1.16.0-dev".to_string(),
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
                    .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
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
                    .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
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
    async fn returns_default_instance_identity() {
        let router = test_state().router();
        let response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["app_name"], "Firecracker");
        assert_eq!(payload["id"], "anonymous-instance");
        assert_eq!(payload["state"], "Not started");
        assert_eq!(payload["vmm_version"], "1.16.0-dev");
    }

    #[tokio::test]
    async fn updates_machine_config_and_exports_vm_config() {
        let router = test_state().router();

        let put_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/machine-config")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"vcpu_count":2,"mem_size_mib":256,"track_dirty_pages":true}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(put_response.status(), StatusCode::NO_CONTENT);

        let patch_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/machine-config")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"mem_size_mib":512}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(patch_response.status(), StatusCode::NO_CONTENT);

        let machine_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/machine-config")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(machine_response.status(), StatusCode::OK);
        let machine_body = to_bytes(machine_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let machine_payload: serde_json::Value = serde_json::from_slice(&machine_body).unwrap();
        assert_eq!(machine_payload["vcpu_count"], 2);
        assert_eq!(machine_payload["mem_size_mib"], 512);
        assert_eq!(machine_payload["track_dirty_pages"], true);

        let vm_config_response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/vm/config")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(vm_config_response.status(), StatusCode::OK);
        let vm_config_body = to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let vm_config_payload: serde_json::Value = serde_json::from_slice(&vm_config_body).unwrap();
        assert_eq!(vm_config_payload["machine-config"]["mem_size_mib"], 512);
        assert_eq!(vm_config_payload["machine-config"]["vcpu_count"], 2);
    }

    #[tokio::test]
    async fn rejects_invalid_machine_config_payloads() {
        let router = test_state().router();

        let invalid_mem_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/machine-config")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"vcpu_count":2,"mem_size_mib":0}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(invalid_mem_response.status(), StatusCode::BAD_REQUEST);

        let empty_patch_response = router
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/machine-config")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(empty_patch_response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(empty_patch_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload.code, "InvalidRequest");
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
