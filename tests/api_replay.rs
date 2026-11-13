//! Behaviour-matrix replay on the model backend (ADR 0007 slice 6). Every script in
//! `docs/specs/firecracker-api-behavior-replay.yaml` runs against a fresh in-memory router with its own temporary
//! root, and the last response must satisfy the matrix case, including the model's `code`. This proves the scripts
//! reproduce the matrix on the model; `make systemvm-replay-suite` runs the same scripts on Backend A.

use axum::body::{Body, to_bytes};
use axum::http::Request;
use std::path::Path;
use tower::ServiceExt;
use vesper::api_replay::{self, Response, Step, Verdict};

const MATRIX: &str = "docs/generated/firecracker-api-behavior-matrix.json";

fn router() -> axum::Router {
    let lock = vesper::baseline::load("docs/specs/design-v0.baselines.lock.yaml").unwrap();
    let revision = lock.baselines.get("firecracker").unwrap().revision.clone();
    let version = vesper::spec::load_info_version("third_party/firecracker/firecracker.yaml").unwrap();
    let routes = vesper::spec::load_routes("third_party/firecracker/firecracker.yaml")
        .unwrap()
        .into_iter()
        .map(|route| vesper::api::RouteSpec { method: route.method, path: route.path })
        .collect::<Vec<_>>();
    let stubs = vesper::api_matrix::load_surface_stub_matrix(vesper::api_matrix::DEFAULT_SURFACE_STUB_MATRIX_PATH).unwrap();
    vesper::api::AppState::new_with_matrix(&routes, revision, version, &stubs).unwrap().router()
}

fn make_root(dir: &Path, scripts: &api_replay::ReplayScripts, snapshot: bool) {
    let mut files: Vec<(String, u64)> = scripts.files.iter().map(|f| (f.path.clone(), f.bytes)).collect();
    files.push(("/Image".into(), 4096));
    files.push(("/initrd.gz".into(), 4096));
    if snapshot {
        files.push((api_replay::SNAPSHOT_STATE.into(), 4096));
        files.push((api_replay::SNAPSHOT_MEM.into(), 4096));
    }
    for (path, bytes) in files {
        let file = dir.join(path.trim_start_matches('/'));
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, vec![0u8; bytes as usize]).unwrap();
    }
}

async fn send(router: &axum::Router, step: &Step) -> Response {
    let Step::Http { method, path, body } = step else {
        return Response { status: 0, body: String::new() };
    };
    let mut request = Request::builder().method(method.as_str()).uri(path.as_str());
    if body.is_some() {
        request = request.header("content-type", "application/json");
    }
    let request = request.body(body.clone().map(Body::from).unwrap_or_else(Body::empty)).unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status().as_u16();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    Response { status, body: String::from_utf8_lossy(&bytes).into_owned() }
}

#[test]
fn replay_scripts_cover_the_matrix_exactly() {
    let scripts = api_replay::load_replay_scripts(api_replay::DEFAULT_REPLAY_SCRIPTS_PATH).unwrap();
    let matrix = vesper::api_behavior_matrix::load_api_behavior_matrix(MATRIX).unwrap();
    let errors = api_replay::validate_replay_scripts(&scripts, &matrix);
    assert!(errors.is_empty(), "replay scripts:\n  {}", errors.join("\n  "));
}

#[test]
fn every_host_dev_name_is_a_listed_tap() {
    let mut scripts = api_replay::load_replay_scripts(api_replay::DEFAULT_REPLAY_SCRIPTS_PATH).unwrap();
    let matrix = vesper::api_behavior_matrix::load_api_behavior_matrix(MATRIX).unwrap();
    assert!(scripts.taps.iter().any(|t| t == "tap0"), "taps: {:?}", scripts.taps);
    scripts.taps.retain(|t| t != "tap1");
    let errors = api_replay::validate_replay_scripts(&scripts, &matrix);
    assert!(!errors.is_empty() && errors.iter().all(|e| e.contains(r#"host_dev_name "tap1" is not in taps"#)), "{errors:?}");
    scripts.taps = vec!["tap0".into(), "tap1".into(), "tap0".into(), "lo".into(), "-x".into()];
    let errors = api_replay::validate_replay_scripts(&scripts, &matrix);
    assert!(errors.iter().any(|e| e.contains("listed twice")) && errors.iter().filter(|e| e.contains("not lo")).count() == 2, "{errors:?}");
    scripts.taps = (0..9).map(|i| format!("tap{i}")).collect();
    let errors = api_replay::validate_replay_scripts(&scripts, &matrix);
    assert!(errors.iter().any(|e| e.contains("9 taps exceed 8")), "{errors:?}");
    // The fixture is checked too, and a name nested in network_overrides counts.
    scripts.taps = vec!["tap0".into(), "tap1".into()];
    scripts.snapshot_fixture.insert(0, r#"PUT /network-interfaces/eth0 {"iface_id":"eth0","host_dev_name":"tap7"}"#.into());
    let key = scripts.cases.keys().next().unwrap().clone();
    scripts.cases.get_mut(&key).unwrap().setup.push(
        r#"PUT /snapshot/load {"snapshot_path":"/s","mem_file_path":"/m","network_overrides":[{"iface_id":"eth0","host_dev_name":"tap8"}]}"#.into(),
    );
    let errors = api_replay::validate_replay_scripts(&scripts, &matrix);
    assert!(errors.iter().any(|e| e.contains(r#"snapshot_fixture: host_dev_name "tap7""#)), "{errors:?}");
    assert!(errors.iter().any(|e| e.contains(r#"host_dev_name "tap8" is not in taps"#)), "{errors:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn every_case_replays_on_the_model() {
    let scripts = api_replay::load_replay_scripts(api_replay::DEFAULT_REPLAY_SCRIPTS_PATH).unwrap();
    let matrix = vesper::api_behavior_matrix::load_api_behavior_matrix(MATRIX).unwrap();
    // The model answers as the host architecture does, so cases for the other architecture are not applicable.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().into_owned();
    let mut run = |_key: &str, steps: &[Step], snapshot: bool| -> Result<Vec<Response>, String> {
        // A fresh root per case at the same path, so `{root}` stays one string.
        let _ = std::fs::remove_dir_all(dir.path());
        make_root(dir.path(), &scripts, snapshot);
        let router = router();
        let handle = tokio::runtime::Handle::current();
        Ok(tokio::task::block_in_place(|| handle.block_on(send_all(&router, steps))))
    };
    let results = api_replay::replay(&scripts, &matrix, &root, std::env::consts::ARCH, true, &|_| true, &mut run);
    let total: usize = matrix.operations.iter().map(|op| op.cases.len()).sum();
    let bad: Vec<String> = results
        .iter()
        .filter(|r| !matches!(r.verdict, Verdict::Match | Verdict::NotApplicable))
        .map(|r| format!("{} [{:?}] {}", r.key, r.verdict, r.detail))
        .collect();
    assert!(bad.is_empty(), "{} of {} cases differ on the model:\n  {}", bad.len(), results.len(), bad.join("\n  "));
    assert_eq!(results.len(), total, "every case is judged once");
    let skipped = results.iter().filter(|r| r.verdict == Verdict::NotApplicable).count();
    assert!(skipped <= 3, "{skipped} cases not applicable on {}", std::env::consts::ARCH);
}

async fn send_all(router: &axum::Router, steps: &[Step]) -> Vec<Response> {
    let mut out = Vec::new();
    for step in steps {
        out.push(send(router, step).await);
    }
    out
}
