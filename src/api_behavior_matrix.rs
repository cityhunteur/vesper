use crate::api_matrix::{
    RouteSemantics, documented_route_semantics, route_uses_matrix_backed_placeholder,
};
use crate::spec::Route as RouteSpec;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

pub const DEFAULT_API_BEHAVIOR_MATRIX_PATH: &str =
    "docs/generated/firecracker-api-behavior-matrix.json";
pub const DEFAULT_API_BEHAVIOR_CAPTURE_REGISTRY_PATH: &str =
    "docs/specs/firecracker-api-behavior-captures.yaml";
const DEFAULT_BASELINE_LOCK_PATH: &str = "docs/specs/design-v0.baselines.lock.yaml";
const DEFAULT_SWAGGER_PATH: &str = "third_party/firecracker/firecracker.yaml";
const DEFAULT_MATRIX_NOTE: &str = "This generated matrix records route-level baseline behavior captures for the pinned Firecracker revision. Entries marked pending_baseline_capture still require pinned upstream status/error capture for the current route behavior. Entries marked captured_in_repo carry scenario-level facts for the current concrete-handler or matrix-backed placeholder behavior.";

mod validate;
#[cfg(test)]
use validate::validate_behavior_case;
pub use validate::{validate_api_behavior_capture_registry, validate_api_behavior_matrix};

fn default_capture_registry_path() -> String {
    DEFAULT_API_BEHAVIOR_CAPTURE_REGISTRY_PATH.to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BehaviorPath {
    ConcreteHandler,
    MatrixBackedPlaceholder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BehaviorCaptureState {
    PendingBaselineCapture,
    CapturedInRepo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BehaviorOutcome {
    NoContent,
    SuccessBody,
    ErrorEnvelope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BehaviorArch {
    Any,
    X86_64,
    Aarch64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BehaviorBodyAssertionKind {
    Exact,
    Type,
    Absent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BehaviorJsonType {
    Null,
    Boolean,
    Number,
    String,
    Array,
    Object,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BehaviorBodyAssertion {
    pub pointer: String,
    pub kind: BehaviorBodyAssertionKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_type: Option<BehaviorJsonType>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BehaviorCase {
    pub id: String,
    pub arch: BehaviorArch,
    pub when: String,
    pub status_code: u16,
    pub outcome: BehaviorOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub body_assertions: Vec<BehaviorBodyAssertion>,
    #[serde(default)]
    pub evidence_files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiBehaviorCaptureRegistry {
    pub version: u32,
    #[serde(default)]
    pub captures: Vec<ApiBehaviorCapture>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiBehaviorCapture {
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub cases: Vec<BehaviorCase>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiBehaviorMatrixEntry {
    pub method: String,
    pub path: String,
    pub semantics: RouteSemantics,
    pub behavior_path: BehaviorPath,
    pub capture_state: BehaviorCaptureState,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cases: Vec<BehaviorCase>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub capture_note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiBehaviorMatrixSource {
    pub baseline_lock: String,
    pub swagger: String,
    #[serde(default = "default_capture_registry_path")]
    pub captures: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiBehaviorMatrix {
    pub version: u32,
    pub firecracker_revision: String,
    pub firecracker_version: String,
    pub source: ApiBehaviorMatrixSource,
    pub notes: Vec<String>,
    pub operations: Vec<ApiBehaviorMatrixEntry>,
}

pub fn generate_api_behavior_matrix(
    routes: &[RouteSpec],
    firecracker_revision: &str,
    firecracker_version: &str,
) -> Result<ApiBehaviorMatrix, String> {
    generate_api_behavior_matrix_with_captures(
        routes,
        firecracker_revision,
        firecracker_version,
        None,
        DEFAULT_API_BEHAVIOR_CAPTURE_REGISTRY_PATH,
    )
}

pub fn generate_api_behavior_matrix_with_captures(
    routes: &[RouteSpec],
    firecracker_revision: &str,
    firecracker_version: &str,
    capture_registry: Option<&ApiBehaviorCaptureRegistry>,
    capture_registry_path: &str,
) -> Result<ApiBehaviorMatrix, String> {
    if firecracker_revision.trim().is_empty() {
        return Err("firecracker revision is required".to_string());
    }
    if firecracker_version.trim().is_empty() {
        return Err("firecracker version is required".to_string());
    }
    if capture_registry_path.trim().is_empty() {
        return Err("API behavior capture registry path is required".to_string());
    }

    let expected_routes = routes
        .iter()
        .map(|route| operation_key(&route.method, &route.path))
        .collect::<BTreeSet<_>>();
    let capture_overrides = build_capture_overrides(capture_registry, &expected_routes)?;

    let mut operations = BTreeMap::new();
    for route in routes {
        let key = operation_key(&route.method, &route.path);
        let Some(semantics) = documented_route_semantics(&key) else {
            return Err(format!("missing documented semantics for {key}"));
        };
        let entry = operations
            .entry(key.clone())
            .or_insert_with(|| ApiBehaviorMatrixEntry {
                method: route.method.trim().to_ascii_uppercase(),
                path: clean_path(&route.path),
                semantics,
                behavior_path: behavior_path_for(&key),
                capture_state: BehaviorCaptureState::PendingBaselineCapture,
                cases: Vec::new(),
                capture_note: String::new(),
            });
        if let Some(capture) = capture_overrides.get(&key) {
            entry.capture_state = BehaviorCaptureState::CapturedInRepo;
            entry.cases = capture.cases.clone();
            entry.capture_note = capture.note.trim().to_string();
        }
    }

    let matrix = ApiBehaviorMatrix {
        version: 1,
        firecracker_revision: firecracker_revision.trim().to_string(),
        firecracker_version: firecracker_version.trim().to_string(),
        source: ApiBehaviorMatrixSource {
            baseline_lock: DEFAULT_BASELINE_LOCK_PATH.to_string(),
            swagger: DEFAULT_SWAGGER_PATH.to_string(),
            captures: capture_registry_path.trim().to_string(),
        },
        notes: vec![DEFAULT_MATRIX_NOTE.to_string()],
        operations: operations.into_values().collect(),
    };

    validate_api_behavior_matrix(&matrix, routes, firecracker_revision, firecracker_version)?;
    Ok(matrix)
}

pub fn load_api_behavior_capture_registry(
    path: impl AsRef<Path>,
) -> Result<ApiBehaviorCaptureRegistry, String> {
    let path = path.as_ref();
    let content = fs::read_to_string(path).map_err(|err| {
        format!(
            "read API behavior capture registry {}: {err}",
            path.display()
        )
    })?;
    let registry: ApiBehaviorCaptureRegistry = serde_yaml::from_str(&content).map_err(|err| {
        format!(
            "parse API behavior capture registry {}: {err}",
            path.display()
        )
    })?;

    validate_api_behavior_capture_registry(&registry)?;
    Ok(registry)
}

pub fn load_api_behavior_matrix(path: impl AsRef<Path>) -> Result<ApiBehaviorMatrix, String> {
    let path = path.as_ref();
    let content = fs::read_to_string(path)
        .map_err(|err| format!("read API behavior matrix {}: {err}", path.display()))?;
    serde_json::from_str(&content)
        .map_err(|err| format!("parse API behavior matrix {}: {err}", path.display()))
}

pub fn write_api_behavior_matrix(
    path: impl AsRef<Path>,
    matrix: &ApiBehaviorMatrix,
) -> Result<(), String> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "create API behavior matrix directory {}: {err}",
                parent.display()
            )
        })?;
    }

    let mut content = serde_json::to_string_pretty(matrix)
        .map_err(|err| format!("serialize API behavior matrix: {err}"))?;
    content.push('\n');
    fs::write(path, content)
        .map_err(|err| format!("write API behavior matrix {}: {err}", path.display()))
}

fn build_capture_overrides(
    capture_registry: Option<&ApiBehaviorCaptureRegistry>,
    expected_routes: &BTreeSet<String>,
) -> Result<BTreeMap<String, ApiBehaviorCapture>, String> {
    let Some(capture_registry) = capture_registry else {
        return Ok(BTreeMap::new());
    };

    validate_api_behavior_capture_registry(capture_registry)?;

    let mut capture_overrides = BTreeMap::new();
    for capture in &capture_registry.captures {
        let key = operation_key(&capture.method, &capture.path);
        if !expected_routes.contains(&key) {
            return Err(format!(
                "API behavior capture registry operation {key} does not match a known Firecracker route"
            ));
        }
        capture_overrides.insert(key, capture.clone());
    }

    Ok(capture_overrides)
}

fn behavior_path_for(operation: &str) -> BehaviorPath {
    if route_uses_matrix_backed_placeholder(operation) {
        BehaviorPath::MatrixBackedPlaceholder
    } else {
        BehaviorPath::ConcreteHandler
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn sample_routes() -> Vec<RouteSpec> {
        vec![
            RouteSpec {
                method: "GET".to_string(),
                path: "/".to_string(),
            },
            RouteSpec {
                method: "PUT".to_string(),
                path: "/actions".to_string(),
            },
            RouteSpec {
                method: "PUT".to_string(),
                path: "/snapshot/load".to_string(),
            },
        ]
    }

    fn unique_temp_path(name: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("vesper-api-behavior-matrix-{name}-{suffix}"))
    }

    #[test]
    fn generates_behavior_matrix_for_all_routes() {
        let routes = sample_routes();
        let matrix = generate_api_behavior_matrix(&routes, "rev-1", "1.16.0-dev").unwrap();

        assert_eq!(matrix.operations.len(), routes.len());
        assert_eq!(matrix.firecracker_revision, "rev-1");
        assert_eq!(
            matrix
                .operations
                .iter()
                .find(|operation| operation.method == "PUT" && operation.path == "/actions")
                .unwrap()
                .behavior_path,
            BehaviorPath::ConcreteHandler
        );
        assert_eq!(
            matrix
                .operations
                .iter()
                .find(|operation| operation.method == "PUT" && operation.path == "/snapshot/load")
                .unwrap()
                .behavior_path,
            BehaviorPath::ConcreteHandler
        );
    }

    #[test]
    fn applies_behavior_capture_registry_overrides() {
        let routes = sample_routes();
        let evidence_path = unique_temp_path("actions-proof.md");
        fs::write(&evidence_path, "proof").unwrap();

        let registry = ApiBehaviorCaptureRegistry {
            version: 1,
            captures: vec![ApiBehaviorCapture {
                method: "PUT".to_string(),
                path: "/actions".to_string(),
                note: "Pins the current /actions behavior.".to_string(),
                cases: vec![
                    BehaviorCase {
                        id: "preboot_flush_metrics".to_string(),
                        arch: BehaviorArch::Any,
                        when: "FlushMetrics before boot".to_string(),
                        status_code: 400,
                        outcome: BehaviorOutcome::ErrorEnvelope,
                        code: Some("InvalidState".to_string()),
                        message: Some(
                            "The requested operation is not supported before starting the microVM."
                                .to_string(),
                        ),
                        body_assertions: Vec::new(),
                        evidence_files: vec![evidence_path.to_string_lossy().into_owned()],
                    },
                    BehaviorCase {
                        id: "postboot_flush_metrics".to_string(),
                        arch: BehaviorArch::Any,
                        when: "FlushMetrics after boot".to_string(),
                        status_code: 204,
                        outcome: BehaviorOutcome::NoContent,
                        code: None,
                        message: None,
                        body_assertions: Vec::new(),
                        evidence_files: vec![evidence_path.to_string_lossy().into_owned()],
                    },
                ],
            }],
        };

        let matrix = generate_api_behavior_matrix_with_captures(
            &routes,
            "rev-1",
            "1.16.0-dev",
            Some(&registry),
            "docs/specs/firecracker-api-behavior-captures.yaml",
        )
        .unwrap();

        let actions = matrix
            .operations
            .iter()
            .find(|operation| operation.method == "PUT" && operation.path == "/actions")
            .unwrap();
        assert_eq!(actions.capture_state, BehaviorCaptureState::CapturedInRepo);
        assert_eq!(actions.cases.len(), 2);

        let _ = fs::remove_file(&evidence_path);
    }

    #[test]
    fn rejects_behavior_capture_for_unknown_route() {
        let routes = sample_routes();
        let evidence_path = unique_temp_path("unknown-route-proof.md");
        fs::write(&evidence_path, "proof").unwrap();

        let registry = ApiBehaviorCaptureRegistry {
            version: 1,
            captures: vec![ApiBehaviorCapture {
                method: "PUT".to_string(),
                path: "/not-a-route".to_string(),
                note: "Invalid test capture.".to_string(),
                cases: vec![BehaviorCase {
                    id: "case".to_string(),
                    arch: BehaviorArch::Any,
                    when: "Invalid".to_string(),
                    status_code: 400,
                    outcome: BehaviorOutcome::ErrorEnvelope,
                    code: Some("InvalidState".to_string()),
                    message: Some("invalid".to_string()),
                    body_assertions: Vec::new(),
                    evidence_files: vec![evidence_path.to_string_lossy().into_owned()],
                }],
            }],
        };

        let err = generate_api_behavior_matrix_with_captures(
            &routes,
            "rev-1",
            "1.16.0-dev",
            Some(&registry),
            "docs/specs/firecracker-api-behavior-captures.yaml",
        )
        .unwrap_err();
        assert!(err.contains("does not match a known Firecracker route"));

        let _ = fs::remove_file(&evidence_path);
    }

    #[test]
    fn rejects_captured_behavior_entry_without_cases() {
        let routes = sample_routes();
        let mut matrix = generate_api_behavior_matrix(&routes, "rev-1", "1.16.0-dev").unwrap();
        let actions = matrix
            .operations
            .iter_mut()
            .find(|operation| operation.method == "PUT" && operation.path == "/actions")
            .unwrap();
        actions.capture_state = BehaviorCaptureState::CapturedInRepo;
        actions.capture_note = "Incomplete capture".to_string();

        let err =
            validate_api_behavior_matrix(&matrix, &routes, "rev-1", "1.16.0-dev").unwrap_err();
        assert!(err.contains("must include at least one case"));
    }

    #[test]
    fn rejects_no_content_case_with_error_fields() {
        let case = BehaviorCase {
            id: "bad-case".to_string(),
            arch: BehaviorArch::Any,
            when: "invalid".to_string(),
            status_code: 204,
            outcome: BehaviorOutcome::NoContent,
            code: Some("Invalid".to_string()),
            message: Some("invalid".to_string()),
            body_assertions: Vec::new(),
            evidence_files: vec![env!("CARGO_MANIFEST_DIR").to_string()],
        };

        let err = validate_behavior_case(&case, "case").unwrap_err();
        assert!(err.contains("must not include error code, message, or body assertions"));
    }

    #[test]
    fn rejects_success_body_case_without_body_assertions() {
        let case = BehaviorCase {
            id: "missing-assertions".to_string(),
            arch: BehaviorArch::Any,
            when: "invalid".to_string(),
            status_code: 200,
            outcome: BehaviorOutcome::SuccessBody,
            code: None,
            message: None,
            body_assertions: Vec::new(),
            evidence_files: vec![env!("CARGO_MANIFEST_DIR").to_string()],
        };

        let err = validate_behavior_case(&case, "case").unwrap_err();
        assert!(err.contains("requires at least one body assertion"));
    }

    #[test]
    fn rejects_success_body_case_with_error_fields() {
        let case = BehaviorCase {
            id: "bad-success".to_string(),
            arch: BehaviorArch::Any,
            when: "invalid".to_string(),
            status_code: 200,
            outcome: BehaviorOutcome::SuccessBody,
            code: Some("Invalid".to_string()),
            message: Some("invalid".to_string()),
            body_assertions: vec![BehaviorBodyAssertion {
                pointer: "/state".to_string(),
                kind: BehaviorBodyAssertionKind::Exact,
                value: Some(serde_json::Value::String("Not started".to_string())),
                value_type: None,
            }],
            evidence_files: vec![env!("CARGO_MANIFEST_DIR").to_string()],
        };

        let err = validate_behavior_case(&case, "case").unwrap_err();
        assert!(err.contains("must not include error code or message"));
    }

    #[test]
    fn rejects_invalid_body_assertion_matcher() {
        let case = BehaviorCase {
            id: "bad-assertion".to_string(),
            arch: BehaviorArch::Any,
            when: "invalid".to_string(),
            status_code: 200,
            outcome: BehaviorOutcome::SuccessBody,
            code: None,
            message: None,
            body_assertions: vec![BehaviorBodyAssertion {
                pointer: "/state".to_string(),
                kind: BehaviorBodyAssertionKind::Type,
                value: None,
                value_type: None,
            }],
            evidence_files: vec![env!("CARGO_MANIFEST_DIR").to_string()],
        };

        let err = validate_behavior_case(&case, "case").unwrap_err();
        assert!(err.contains("type body assertion requires a value_type"));
    }

    #[test]
    fn accepts_success_body_case_with_exact_type_and_absent_assertions() {
        let case = BehaviorCase {
            id: "good-success".to_string(),
            arch: BehaviorArch::Any,
            when: "valid".to_string(),
            status_code: 200,
            outcome: BehaviorOutcome::SuccessBody,
            code: None,
            message: None,
            body_assertions: vec![
                BehaviorBodyAssertion {
                    pointer: "/state".to_string(),
                    kind: BehaviorBodyAssertionKind::Exact,
                    value: Some(serde_json::Value::String("Not started".to_string())),
                    value_type: None,
                },
                BehaviorBodyAssertion {
                    pointer: "/app_name".to_string(),
                    kind: BehaviorBodyAssertionKind::Type,
                    value: None,
                    value_type: Some(BehaviorJsonType::String),
                },
                BehaviorBodyAssertion {
                    pointer: "/cpu-config".to_string(),
                    kind: BehaviorBodyAssertionKind::Absent,
                    value: None,
                    value_type: None,
                },
            ],
            evidence_files: vec![env!("CARGO_MANIFEST_DIR").to_string()],
        };

        validate_behavior_case(&case, "case").unwrap();
    }

    #[test]
    fn rejects_absent_body_assertion_with_value() {
        let case = BehaviorCase {
            id: "bad-absent".to_string(),
            arch: BehaviorArch::Any,
            when: "invalid".to_string(),
            status_code: 200,
            outcome: BehaviorOutcome::SuccessBody,
            code: None,
            message: None,
            body_assertions: vec![BehaviorBodyAssertion {
                pointer: "/cpu-config".to_string(),
                kind: BehaviorBodyAssertionKind::Absent,
                value: Some(serde_json::Value::Bool(true)),
                value_type: None,
            }],
            evidence_files: vec![env!("CARGO_MANIFEST_DIR").to_string()],
        };

        let err = validate_behavior_case(&case, "case").unwrap_err();
        assert!(err.contains("absent body assertion must not include an exact value"));
    }

    #[test]
    fn accepts_root_document_body_assertion() {
        let case = BehaviorCase {
            id: "root-success".to_string(),
            arch: BehaviorArch::Any,
            when: "valid".to_string(),
            status_code: 200,
            outcome: BehaviorOutcome::SuccessBody,
            code: None,
            message: None,
            body_assertions: vec![BehaviorBodyAssertion {
                pointer: String::new(),
                kind: BehaviorBodyAssertionKind::Exact,
                value: Some(serde_json::json!({})),
                value_type: None,
            }],
            evidence_files: vec![env!("CARGO_MANIFEST_DIR").to_string()],
        };

        validate_behavior_case(&case, "case").unwrap();
    }

    #[test]
    fn load_capture_registry_rejects_missing_evidence_files() {
        let registry_path = unique_temp_path("registry.yaml");
        let missing_evidence = unique_temp_path("missing-proof.md");
        fs::write(
            &registry_path,
            format!(
                "version: 1\ncaptures:\n  - method: PUT\n    path: /actions\n    note: Pins actions.\n    cases:\n      - id: case\n        arch: any\n        when: test\n        status_code: 400\n        outcome: error_envelope\n        code: InvalidState\n        message: test\n        evidence_files:\n          - {}\n",
                missing_evidence.display()
            ),
        )
        .unwrap();

        let err = load_api_behavior_capture_registry(&registry_path).unwrap_err();
        assert!(err.contains("missing evidence file"));

        let _ = fs::remove_file(&registry_path);
    }

    #[test]
    fn validates_repo_generated_behavior_matrix() {
        let routes = crate::spec::load_routes(DEFAULT_SWAGGER_PATH).unwrap();
        let lock = crate::baseline::load(DEFAULT_BASELINE_LOCK_PATH).unwrap();
        let firecracker_revision = lock.baselines.get("firecracker").unwrap().revision.clone();
        let firecracker_version = crate::spec::load_info_version(DEFAULT_SWAGGER_PATH).unwrap();
        let capture_registry =
            load_api_behavior_capture_registry(DEFAULT_API_BEHAVIOR_CAPTURE_REGISTRY_PATH).unwrap();
        let expected = generate_api_behavior_matrix_with_captures(
            &routes,
            &firecracker_revision,
            &firecracker_version,
            Some(&capture_registry),
            DEFAULT_API_BEHAVIOR_CAPTURE_REGISTRY_PATH,
        )
        .unwrap();
        let matrix = load_api_behavior_matrix(DEFAULT_API_BEHAVIOR_MATRIX_PATH).unwrap();

        assert_eq!(matrix, expected);
        validate_api_behavior_matrix(
            &matrix,
            &routes,
            &firecracker_revision,
            &firecracker_version,
        )
        .unwrap();
    }
}
