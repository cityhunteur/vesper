//! TD-047: property tests of the request path on the model backend. Any method, path and body gets an answer below
//! 500 (a 500 is a handler panic), and every error answer is a baseline-stamped envelope. A second property sends
//! JSON objects built from the API's own field names to every pinned route, so the cases reach past the JSON parser
//! into the handlers and their state checks.

use super::*;
use proptest::prelude::*;

const METHODS: &[&str] = &["GET", "PUT", "PATCH", "POST", "DELETE", "HEAD"];
const FIELDS: &[&str] = &[
    "action_type",
    "kernel_image_path",
    "boot_args",
    "initrd_path",
    "drive_id",
    "path_on_host",
    "is_root_device",
    "is_read_only",
    "vcpu_count",
    "mem_size_mib",
    "smt",
    "track_dirty_pages",
    "iface_id",
    "host_dev_name",
    "guest_mac",
    "amount_mib",
    "deflate_on_oom",
    "stats_polling_interval_s",
    "state",
    "snapshot_type",
    "snapshot_path",
    "mem_file_path",
    "mem_backend",
    "backend_type",
    "backend_path",
    "resume_vm",
    "guest_cid",
    "uds_path",
    "log_path",
    "level",
    "metrics_path",
    "network_interfaces",
    "ipv4_address",
    "version",
    "total_size_mib",
    "slot_size_mib",
    "requested_size_mib",
    "id",
    "root_device",
    "read_only",
];

fn routes() -> Vec<(String, String)> {
    let golden: serde_json::Value = serde_json::from_str(include_str!(
        "../../../testdata/contracts/firecracker-routes.golden.json"
    ))
    .unwrap();
    golden
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["method"].as_str().unwrap().to_string(),
                r["path"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

fn scalar() -> impl Strategy<Value = serde_json::Value> {
    prop_oneof![
        Just(serde_json::Value::Null),
        proptest::arbitrary::any::<bool>().prop_map(serde_json::Value::from),
        proptest::arbitrary::any::<i64>().prop_map(serde_json::Value::from),
        (0u64..70000).prop_map(serde_json::Value::from),
        "[ -~]{0,24}".prop_map(serde_json::Value::from),
        Just(serde_json::json!({})),
        Just(serde_json::json!([])),
    ]
}

fn object() -> impl Strategy<Value = serde_json::Value> {
    prop::collection::vec((0..FIELDS.len(), scalar()), 0..6).prop_map(|pairs| {
        serde_json::Value::Object(
            pairs
                .into_iter()
                .map(|(k, v)| (FIELDS[k].to_string(), v))
                .collect(),
        )
    })
}

/// Sends one request to a fresh model state; returns the status and the body.
fn send(method: &str, path: &str, body: Vec<u8>) -> Option<(StatusCode, Vec<u8>)> {
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json")
        .body(axum::body::Body::from(body))
        .ok()?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    Some(rt.block_on(async {
        let response = test_state().router().oneshot(request).await.unwrap();
        let status = response.status();
        (
            status,
            to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
    }))
}

fn check(method: &str, path: &str, status: StatusCode, body: &[u8]) -> Result<(), TestCaseError> {
    prop_assert!(
        status.as_u16() < 500,
        "{method} {path} -> {status}: {}",
        String::from_utf8_lossy(body)
    );
    if status.as_u16() >= 400 && method != "HEAD" {
        let envelope: serde_json::Value = serde_json::from_slice(body).map_err(|e| {
            TestCaseError::fail(format!("{method} {path} -> {status}: not JSON ({e})"))
        })?;
        prop_assert!(
            envelope["fault_message"].is_string(),
            "{method} {path}: no fault_message: {envelope}"
        );
        prop_assert!(
            envelope["baseline_firecracker_revision"].is_string(),
            "{method} {path}: no revision: {envelope}"
        );
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 300, ..ProptestConfig::default() })]

    #[test]
    fn any_request_gets_an_answer_and_errors_are_envelopes(
        m in 0..METHODS.len(),
        path in "/[A-Za-z0-9_.~/-]{0,48}",
        body in prop::collection::vec(proptest::arbitrary::any::<u8>(), 0..256),
    ) {
        if let Some((status, bytes)) = send(METHODS[m], &path, body) {
            check(METHODS[m], &path, status, &bytes)?;
        }
    }

    #[test]
    fn pinned_routes_with_api_shaped_bodies_never_fail_the_server(
        r in 0..routes().len(),
        id in "[a-z0-9_]{1,12}",
        body in object(),
    ) {
        let (method, template) = routes()[r].clone();
        let path = template.split('/').map(|s| if s.starts_with('{') { id.as_str() } else { s }).collect::<Vec<_>>().join("/");
        let raw = serde_json::to_vec(&body).unwrap();
        if let Some((status, bytes)) = send(&method, &path, raw) {
            check(&method, &path, status, &bytes)?;
        }
    }
}
