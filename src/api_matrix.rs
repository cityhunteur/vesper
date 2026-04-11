use crate::api::RouteSpec;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

pub const DEFAULT_SURFACE_STUB_MATRIX_PATH: &str =
    "docs/generated/firecracker-api-surface-stub-matrix.json";
pub const DEFAULT_BASELINE_CAPTURE_REGISTRY_PATH: &str =
    "docs/specs/firecracker-api-baseline-captures.yaml";
const DEFAULT_BASELINE_LOCK_PATH: &str = "docs/specs/design-v0.baselines.lock.yaml";
const DEFAULT_SWAGGER_PATH: &str = "third_party/firecracker/firecracker.yaml";
const DEFAULT_STUB_STATUS_CODE: u16 = 501;
const DEFAULT_STUB_CODE: &str = "NotImplemented";
const DEFAULT_STUB_MESSAGE: &str = "operation not implemented";
const DEFAULT_MATRIX_NOTE: &str = "This generated matrix pins the current Vesper surface-stub responses to the Firecracker baseline revision. Entries marked not_applicable are routed through concrete handlers and do not currently rely on the matrix-backed placeholder path. Entries marked pending_baseline_capture still require upstream behavior capture for the operation's current matrix-backed behavior slice. Entries marked captured_in_repo reference durable repo evidence for the current matrix-backed slice only and do not mean full endpoint parity is complete.";

fn default_capture_registry_path() -> String {
    DEFAULT_BASELINE_CAPTURE_REGISTRY_PATH.to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RouteSemantics {
    #[serde(rename = "Any")]
    Any,
    #[serde(rename = "PB")]
    PreBoot,
    #[serde(rename = "PO")]
    PostBoot,
    #[serde(rename = "PB/RT")]
    PreBootRuntime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BaselineCaptureState {
    NotApplicable,
    PendingBaselineCapture,
    CapturedInRepo,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaselineCaptureRegistry {
    pub version: u32,
    #[serde(default)]
    pub captures: Vec<BaselineCapture>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaselineCapture {
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub evidence_files: Vec<String>,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatrixStatus {
    pub status_code: u16,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceStubMatrixEntry {
    pub method: String,
    pub path: String,
    pub semantics: RouteSemantics,
    pub stub_response: MatrixStatus,
    pub baseline_capture: BaselineCaptureState,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_files: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub capture_note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceStubMatrixSource {
    pub baseline_lock: String,
    pub swagger: String,
    #[serde(default = "default_capture_registry_path")]
    pub captures: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceStubMatrix {
    pub version: u32,
    pub firecracker_revision: String,
    pub firecracker_version: String,
    pub source: SurfaceStubMatrixSource,
    pub notes: Vec<String>,
    pub operations: Vec<SurfaceStubMatrixEntry>,
}

pub fn documented_route_semantics(operation: &str) -> Option<RouteSemantics> {
    use RouteSemantics::{Any, PostBoot, PreBoot};

    Some(match operation {
        "GET /"
        | "PUT /actions"
        | "GET /balloon"
        | "PUT /logger"
        | "GET /machine-config"
        | "PUT /metrics"
        | "GET /mmds"
        | "PATCH /mmds"
        | "PUT /mmds"
        | "PUT /serial"
        | "GET /version"
        | "PATCH /vm"
        | "GET /vm/config" => Any,
        "GET /balloon/hinting/status"
        | "GET /balloon/statistics"
        | "GET /hotplug/memory"
        | "PATCH /balloon"
        | "PATCH /balloon/hinting/start"
        | "PATCH /balloon/hinting/stop"
        | "PATCH /balloon/statistics"
        | "PATCH /drives/{drive_id}"
        | "PATCH /hotplug/memory"
        | "PATCH /network-interfaces/{iface_id}"
        | "PUT /snapshot/create" => PostBoot,
        "PUT /balloon"
        | "PUT /boot-source"
        | "PUT /cpu-config"
        | "PUT /drives/{drive_id}"
        | "PUT /entropy"
        | "PATCH /machine-config"
        | "PUT /machine-config"
        | "PUT /mmds/config"
        | "PUT /network-interfaces/{iface_id}"
        | "PUT /pmem/{id}"
        | "PUT /hotplug/memory"
        | "PUT /snapshot/load"
        | "PUT /vsock" => PreBoot,
        _ => return None,
    })
}

pub fn generate_surface_stub_matrix(
    routes: &[RouteSpec],
    firecracker_revision: &str,
    firecracker_version: &str,
) -> Result<SurfaceStubMatrix, String> {
    generate_surface_stub_matrix_with_captures(
        routes,
        firecracker_revision,
        firecracker_version,
        None,
        DEFAULT_BASELINE_CAPTURE_REGISTRY_PATH,
    )
}

pub fn generate_surface_stub_matrix_with_captures(
    routes: &[RouteSpec],
    firecracker_revision: &str,
    firecracker_version: &str,
    capture_registry: Option<&BaselineCaptureRegistry>,
    capture_registry_path: &str,
) -> Result<SurfaceStubMatrix, String> {
    if firecracker_revision.trim().is_empty() {
        return Err("firecracker revision is required".to_string());
    }
    if firecracker_version.trim().is_empty() {
        return Err("firecracker version is required".to_string());
    }
    if capture_registry_path.trim().is_empty() {
        return Err("baseline capture registry path is required".to_string());
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
            .or_insert_with(|| SurfaceStubMatrixEntry {
                method: route.method.trim().to_ascii_uppercase(),
                path: clean_path(&route.path),
                semantics,
                stub_response: MatrixStatus {
                    status_code: DEFAULT_STUB_STATUS_CODE,
                    code: DEFAULT_STUB_CODE.to_string(),
                    message: DEFAULT_STUB_MESSAGE.to_string(),
                },
                baseline_capture: default_capture_state(&key),
                evidence_files: Vec::new(),
                capture_note: String::new(),
            });
        if let Some(capture) = capture_overrides.get(&key) {
            entry.baseline_capture = BaselineCaptureState::CapturedInRepo;
            entry.evidence_files = capture.evidence_files.clone();
            entry.capture_note = capture.note.trim().to_string();
        }
    }

    let matrix = SurfaceStubMatrix {
        version: 1,
        firecracker_revision: firecracker_revision.trim().to_string(),
        firecracker_version: firecracker_version.trim().to_string(),
        source: SurfaceStubMatrixSource {
            baseline_lock: DEFAULT_BASELINE_LOCK_PATH.to_string(),
            swagger: DEFAULT_SWAGGER_PATH.to_string(),
            captures: capture_registry_path.trim().to_string(),
        },
        notes: vec![DEFAULT_MATRIX_NOTE.to_string()],
        operations: operations.into_values().collect(),
    };

    validate_surface_stub_matrix(&matrix, routes, firecracker_revision, firecracker_version)?;
    Ok(matrix)
}

pub fn load_baseline_capture_registry(
    path: impl AsRef<Path>,
) -> Result<BaselineCaptureRegistry, String> {
    let path = path.as_ref();
    let content = fs::read_to_string(path)
        .map_err(|err| format!("read baseline capture registry {}: {err}", path.display()))?;
    let registry: BaselineCaptureRegistry = serde_yaml::from_str(&content)
        .map_err(|err| format!("parse baseline capture registry {}: {err}", path.display()))?;

    validate_baseline_capture_registry(&registry)?;
    Ok(registry)
}

pub fn load_surface_stub_matrix(path: impl AsRef<Path>) -> Result<SurfaceStubMatrix, String> {
    let path = path.as_ref();
    let content = fs::read_to_string(path)
        .map_err(|err| format!("read surface stub matrix {}: {err}", path.display()))?;
    serde_json::from_str(&content)
        .map_err(|err| format!("parse surface stub matrix {}: {err}", path.display()))
}

pub fn write_surface_stub_matrix(
    path: impl AsRef<Path>,
    matrix: &SurfaceStubMatrix,
) -> Result<(), String> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("create matrix directory {}: {err}", parent.display()))?;
    }

    let mut content = serde_json::to_string_pretty(matrix)
        .map_err(|err| format!("serialize surface stub matrix: {err}"))?;
    content.push('\n');
    fs::write(path, content)
        .map_err(|err| format!("write surface stub matrix {}: {err}", path.display()))
}

pub fn validate_surface_stub_matrix(
    matrix: &SurfaceStubMatrix,
    routes: &[RouteSpec],
    firecracker_revision: &str,
    firecracker_version: &str,
) -> Result<(), String> {
    if matrix.version != 1 {
        return Err(format!(
            "surface stub matrix version must be 1, found {}",
            matrix.version
        ));
    }
    if matrix.firecracker_revision.trim() != firecracker_revision.trim() {
        return Err(format!(
            "surface stub matrix firecracker revision mismatch: expected {}, found {}",
            firecracker_revision.trim(),
            matrix.firecracker_revision
        ));
    }
    if matrix.firecracker_version.trim() != firecracker_version.trim() {
        return Err(format!(
            "surface stub matrix firecracker version mismatch: expected {}, found {}",
            firecracker_version.trim(),
            matrix.firecracker_version
        ));
    }
    if matrix.notes.is_empty() {
        return Err("surface stub matrix notes must not be empty".to_string());
    }
    if matrix.source.captures.trim().is_empty() {
        return Err("surface stub matrix capture registry source must not be empty".to_string());
    }
    if matrix.operations.is_empty() {
        return Err("surface stub matrix operations must not be empty".to_string());
    }

    let expected = routes
        .iter()
        .map(|route| operation_key(&route.method, &route.path))
        .collect::<BTreeSet<_>>();
    let mut actual = BTreeSet::new();

    for operation in &matrix.operations {
        let key = operation_key(&operation.method, &operation.path);
        let Some(semantics) = documented_route_semantics(&key) else {
            return Err(format!(
                "surface stub matrix operation {key} is missing documented semantics"
            ));
        };
        if !actual.insert(key.clone()) {
            return Err(format!("surface stub matrix operation {key} is duplicated"));
        }
        if operation.semantics != semantics {
            return Err(format!(
                "surface stub matrix semantics mismatch for {key}: expected {:?}, found {:?}",
                semantics, operation.semantics
            ));
        }
        validate_status(&operation.stub_response, &key)?;
        validate_capture_entry(operation, &key)?;
    }

    if expected != actual {
        let missing = expected
            .difference(&actual)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        let extra = actual
            .difference(&expected)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "surface stub matrix route coverage mismatch (missing: [{}], extra: [{}])",
            missing, extra
        ));
    }

    Ok(())
}

pub fn validate_baseline_capture_registry(
    registry: &BaselineCaptureRegistry,
) -> Result<(), String> {
    if registry.version == 0 {
        return Err("baseline capture registry version must be set".to_string());
    }

    let mut errors = Vec::new();
    let mut seen = BTreeSet::new();

    for capture in &registry.captures {
        let key = operation_key(&capture.method, &capture.path);
        if capture.method.trim().is_empty() {
            errors.push(format!(
                "baseline capture registry operation {key} method is required"
            ));
        }
        if capture.path.trim().is_empty() {
            errors.push(format!(
                "baseline capture registry operation {key} path is required"
            ));
        }
        if !seen.insert(key.clone()) {
            errors.push(format!(
                "baseline capture registry operation {key} is duplicated"
            ));
        }
        if capture.note.trim().is_empty() {
            errors.push(format!(
                "baseline capture registry operation {key} note is required"
            ));
        }
        if capture.evidence_files.is_empty() {
            errors.push(format!(
                "baseline capture registry operation {key} must reference at least one evidence file"
            ));
        }

        let mut evidence_seen = BTreeSet::new();
        for evidence in &capture.evidence_files {
            if evidence.trim().is_empty() {
                errors.push(format!(
                    "baseline capture registry operation {key} contains an empty evidence file path"
                ));
                continue;
            }
            if !evidence_seen.insert(evidence.clone()) {
                errors.push(format!(
                    "baseline capture registry operation {key} duplicates evidence file {evidence}"
                ));
                continue;
            }
            if !Path::new(evidence).exists() {
                errors.push(format!(
                    "baseline capture registry operation {key} references missing evidence file {evidence}"
                ));
            }
        }
    }

    if !errors.is_empty() {
        errors.sort();
        return Err(errors.join("; "));
    }

    Ok(())
}

fn validate_status(status: &MatrixStatus, operation: &str) -> Result<(), String> {
    if !(100..=599).contains(&status.status_code) {
        return Err(format!(
            "surface stub matrix status code for {operation} must be between 100 and 599"
        ));
    }
    if status.code.trim().is_empty() {
        return Err(format!(
            "surface stub matrix status code field is empty for {operation}"
        ));
    }
    if status.message.trim().is_empty() {
        return Err(format!(
            "surface stub matrix status message field is empty for {operation}"
        ));
    }
    Ok(())
}

fn validate_capture_entry(entry: &SurfaceStubMatrixEntry, operation: &str) -> Result<(), String> {
    match entry.baseline_capture {
        BaselineCaptureState::NotApplicable => {
            if !entry.evidence_files.is_empty() {
                return Err(format!(
                    "surface stub matrix operation {operation} must not include evidence files when the matrix-backed placeholder path is not applicable"
                ));
            }
            if !entry.capture_note.trim().is_empty() {
                return Err(format!(
                    "surface stub matrix operation {operation} must not include a capture note when the matrix-backed placeholder path is not applicable"
                ));
            }
        }
        BaselineCaptureState::PendingBaselineCapture => {
            if !entry.evidence_files.is_empty() {
                return Err(format!(
                    "surface stub matrix operation {operation} must not include evidence files while baseline capture is pending"
                ));
            }
            if !entry.capture_note.trim().is_empty() {
                return Err(format!(
                    "surface stub matrix operation {operation} must not include a capture note while baseline capture is pending"
                ));
            }
        }
        BaselineCaptureState::CapturedInRepo => {
            if entry.evidence_files.is_empty() {
                return Err(format!(
                    "surface stub matrix operation {operation} must reference at least one evidence file when baseline capture is captured_in_repo"
                ));
            }
            if entry.capture_note.trim().is_empty() {
                return Err(format!(
                    "surface stub matrix operation {operation} must include a capture note when baseline capture is captured_in_repo"
                ));
            }

            let mut seen = BTreeSet::new();
            for evidence in &entry.evidence_files {
                if evidence.trim().is_empty() {
                    return Err(format!(
                        "surface stub matrix operation {operation} contains an empty evidence file path"
                    ));
                }
                if !seen.insert(evidence) {
                    return Err(format!(
                        "surface stub matrix operation {operation} duplicates evidence file {evidence}"
                    ));
                }
            }
        }
    }

    Ok(())
}

fn build_capture_overrides(
    capture_registry: Option<&BaselineCaptureRegistry>,
    expected_routes: &BTreeSet<String>,
) -> Result<BTreeMap<String, BaselineCapture>, String> {
    let Some(capture_registry) = capture_registry else {
        return Ok(BTreeMap::new());
    };

    validate_baseline_capture_registry(capture_registry)?;

    let mut capture_overrides = BTreeMap::new();
    for capture in &capture_registry.captures {
        let key = operation_key(&capture.method, &capture.path);
        if !expected_routes.contains(&key) {
            return Err(format!(
                "baseline capture registry operation {key} does not match a known Firecracker route"
            ));
        }
        if !route_uses_matrix_backed_placeholder(&key) {
            return Err(format!(
                "baseline capture registry operation {key} is not eligible for matrix-backed placeholder capture"
            ));
        }
        capture_overrides.insert(key, capture.clone());
    }

    Ok(capture_overrides)
}

fn default_capture_state(operation: &str) -> BaselineCaptureState {
    if route_uses_matrix_backed_placeholder(operation) {
        BaselineCaptureState::PendingBaselineCapture
    } else {
        BaselineCaptureState::NotApplicable
    }
}

pub fn route_uses_matrix_backed_placeholder(operation: &str) -> bool {
    let _ = operation;
    false
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
                path: "/snapshot/load".to_string(),
            },
            RouteSpec {
                method: "PUT".to_string(),
                path: "/snapshot/create".to_string(),
            },
        ]
    }

    fn unique_temp_path(name: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("vesper-api-matrix-{name}-{suffix}"))
    }

    #[test]
    fn generates_matrix_for_all_routes() {
        let routes = sample_routes();
        let matrix = generate_surface_stub_matrix(&routes, "rev-1", "1.16.0-dev").unwrap();

        assert_eq!(matrix.operations.len(), routes.len());
        assert_eq!(matrix.firecracker_revision, "rev-1");
        assert_eq!(matrix.operations[0].stub_response.status_code, 501);
        assert_eq!(
            matrix.operations[0].baseline_capture,
            BaselineCaptureState::NotApplicable
        );
        assert_eq!(
            matrix
                .operations
                .iter()
                .find(|operation| operation.method == "PUT" && operation.path == "/snapshot/load")
                .unwrap()
                .baseline_capture,
            BaselineCaptureState::NotApplicable
        );
    }

    #[test]
    fn rejects_registry_entry_for_snapshot_route_now_that_it_is_concrete() {
        let routes = sample_routes();
        let evidence_path = unique_temp_path("snapshot-load-proof.md");
        fs::write(&evidence_path, "proof").unwrap();

        let registry = BaselineCaptureRegistry {
            version: 1,
            captures: vec![BaselineCapture {
                method: "PUT".to_string(),
                path: "/snapshot/load".to_string(),
                evidence_files: vec![evidence_path.to_string_lossy().into_owned()],
                note: "Pins the current snapshot-load placeholder path after parser and guard validation."
                    .to_string(),
            }],
        };

        let err = generate_surface_stub_matrix_with_captures(
            &routes,
            "rev-1",
            "1.16.0-dev",
            Some(&registry),
            "docs/specs/firecracker-api-baseline-captures.yaml",
        )
        .unwrap_err();
        assert!(err.contains("is not eligible for matrix-backed placeholder capture"));

        let _ = fs::remove_file(&evidence_path);
    }

    #[test]
    fn rejects_registry_entry_for_unknown_route() {
        let routes = sample_routes();
        let evidence_path = unique_temp_path("unknown-route-proof.md");
        fs::write(&evidence_path, "proof").unwrap();

        let registry = BaselineCaptureRegistry {
            version: 1,
            captures: vec![BaselineCapture {
                method: "PUT".to_string(),
                path: "/not-a-route".to_string(),
                evidence_files: vec![evidence_path.to_string_lossy().into_owned()],
                note: "Invalid test capture.".to_string(),
            }],
        };

        let err = generate_surface_stub_matrix_with_captures(
            &routes,
            "rev-1",
            "1.16.0-dev",
            Some(&registry),
            "docs/specs/firecracker-api-baseline-captures.yaml",
        )
        .unwrap_err();
        assert!(err.contains("does not match a known Firecracker route"));

        let _ = fs::remove_file(&evidence_path);
    }

    #[test]
    fn rejects_registry_entry_for_non_placeholder_route() {
        let routes = sample_routes();
        let evidence_path = unique_temp_path("not-applicable-proof.md");
        fs::write(&evidence_path, "proof").unwrap();

        let registry = BaselineCaptureRegistry {
            version: 1,
            captures: vec![BaselineCapture {
                method: "GET".to_string(),
                path: "/".to_string(),
                evidence_files: vec![evidence_path.to_string_lossy().into_owned()],
                note:
                    "This route is handled concretely and should not be in the placeholder registry."
                        .to_string(),
            }],
        };

        let err = generate_surface_stub_matrix_with_captures(
            &routes,
            "rev-1",
            "1.16.0-dev",
            Some(&registry),
            "docs/specs/firecracker-api-baseline-captures.yaml",
        )
        .unwrap_err();
        assert!(err.contains("is not eligible for matrix-backed placeholder capture"));

        let _ = fs::remove_file(&evidence_path);
    }

    #[test]
    fn rejects_captured_matrix_entry_without_evidence() {
        let routes = sample_routes();
        let mut matrix = generate_surface_stub_matrix(&routes, "rev-1", "1.16.0-dev").unwrap();
        let snapshot_load = matrix
            .operations
            .iter_mut()
            .find(|operation| operation.method == "PUT" && operation.path == "/snapshot/load")
            .unwrap();
        snapshot_load.baseline_capture = BaselineCaptureState::CapturedInRepo;
        snapshot_load.capture_note = "Incomplete capture".to_string();

        let err =
            validate_surface_stub_matrix(&matrix, &routes, "rev-1", "1.16.0-dev").unwrap_err();
        assert!(err.contains("must reference at least one evidence file"));
    }

    #[test]
    fn rejects_not_applicable_matrix_entry_with_capture_metadata() {
        let routes = sample_routes();
        let mut matrix = generate_surface_stub_matrix(&routes, "rev-1", "1.16.0-dev").unwrap();
        let instance_info = matrix
            .operations
            .iter_mut()
            .find(|operation| operation.method == "GET" && operation.path == "/")
            .unwrap();
        instance_info.evidence_files = vec!["run/WOR-20/api/not-applicable-proof.md".to_string()];
        instance_info.capture_note = "Should not carry capture metadata.".to_string();

        let err =
            validate_surface_stub_matrix(&matrix, &routes, "rev-1", "1.16.0-dev").unwrap_err();
        assert!(err.contains("matrix-backed placeholder path is not applicable"));
    }

    #[test]
    fn load_capture_registry_rejects_missing_evidence_files() {
        let registry_path = unique_temp_path("registry.yaml");
        let missing_evidence = unique_temp_path("missing-proof.md");
        fs::write(
            &registry_path,
            format!(
                "version: 1\ncaptures:\n  - method: PUT\n    path: /snapshot/load\n    note: Pins the current snapshot-load placeholder path.\n    evidence_files:\n      - {}\n",
                missing_evidence.display()
            ),
        )
        .unwrap();

        let err = load_baseline_capture_registry(&registry_path).unwrap_err();
        assert!(err.contains("references missing evidence file"));

        let _ = fs::remove_file(&registry_path);
    }

    #[test]
    fn validates_repo_generated_matrix() {
        let routes = crate::spec::load_routes(DEFAULT_SWAGGER_PATH)
            .unwrap()
            .into_iter()
            .map(|route| RouteSpec {
                method: route.method,
                path: route.path,
            })
            .collect::<Vec<_>>();
        let lock = crate::baseline::load(DEFAULT_BASELINE_LOCK_PATH).unwrap();
        let firecracker_revision = lock.baselines.get("firecracker").unwrap().revision.clone();
        let firecracker_version = crate::spec::load_info_version(DEFAULT_SWAGGER_PATH).unwrap();
        let capture_registry =
            load_baseline_capture_registry(DEFAULT_BASELINE_CAPTURE_REGISTRY_PATH).unwrap();
        let expected = generate_surface_stub_matrix_with_captures(
            &routes,
            &firecracker_revision,
            &firecracker_version,
            Some(&capture_registry),
            DEFAULT_BASELINE_CAPTURE_REGISTRY_PATH,
        )
        .unwrap();
        let matrix = load_surface_stub_matrix(DEFAULT_SURFACE_STUB_MATRIX_PATH).unwrap();

        assert_eq!(matrix, expected);
        validate_surface_stub_matrix(
            &matrix,
            &routes,
            &firecracker_revision,
            &firecracker_version,
        )
        .unwrap();
    }
}
