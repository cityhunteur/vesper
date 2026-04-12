use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use tempfile::{NamedTempFile, tempdir};
use tower::ServiceExt;

const PREBOOT_ONLY_ROUTES: &[(&str, &str)] = &[
    ("PUT", "/balloon"),
    ("PUT", "/boot-source"),
    ("PUT", "/cpu-config"),
    ("PUT", "/drives/{drive_id}"),
    ("PUT", "/entropy"),
    ("PUT", "/hotplug/memory"),
    ("PATCH", "/machine-config"),
    ("PUT", "/machine-config"),
    ("PUT", "/mmds/config"),
    ("PUT", "/network-interfaces/{iface_id}"),
    ("PUT", "/pmem/{id}"),
    ("PUT", "/snapshot/load"),
    ("PUT", "/vsock"),
];

const POSTBOOT_ONLY_ROUTES: &[(&str, &str)] = &[
    ("PATCH", "/balloon"),
    ("PATCH", "/balloon/hinting/start"),
    ("GET", "/balloon/hinting/status"),
    ("PATCH", "/balloon/hinting/stop"),
    ("GET", "/balloon/statistics"),
    ("PATCH", "/balloon/statistics"),
    ("PATCH", "/drives/{drive_id}"),
    ("GET", "/hotplug/memory"),
    ("PATCH", "/hotplug/memory"),
    ("PATCH", "/network-interfaces/{iface_id}"),
    ("PUT", "/snapshot/create"),
];

fn concrete_path(template: &str) -> String {
    let mut out = String::new();
    let mut chars = template.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '{' {
            while let Some(next) = chars.next() {
                if next == '}' {
                    break;
                }
            }
            out.push_str("test");
            continue;
        }
        out.push(ch);
    }
    out
}

fn app_router() -> (axum::Router, String) {
    let lock = vesper::baseline::load("docs/specs/design-v0.baselines.lock.yaml").unwrap();
    let firecracker = lock.baselines.get("firecracker").unwrap().revision.clone();
    let firecracker_version =
        vesper::spec::load_info_version("third_party/firecracker/firecracker.yaml").unwrap();

    let routes = vesper::spec::load_routes("third_party/firecracker/firecracker.yaml")
        .unwrap()
        .into_iter()
        .map(|route| vesper::api::RouteSpec {
            method: route.method,
            path: route.path,
        })
        .collect::<Vec<_>>();
    let surface_stub_matrix = vesper::api_matrix::load_surface_stub_matrix(
        vesper::api_matrix::DEFAULT_SURFACE_STUB_MATRIX_PATH,
    )
    .unwrap();

    let router = vesper::api::AppState::new_with_matrix(
        &routes,
        firecracker.clone(),
        firecracker_version,
        &surface_stub_matrix,
    )
    .unwrap()
    .router();

    (router, firecracker)
}

async fn start_instance(router: axum::Router) {
    let start_response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/actions")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"action_type":"InstanceStart"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start_response.status(), StatusCode::NO_CONTENT);
}

async fn boot_instance(router: axum::Router) {
    let kernel = NamedTempFile::new().unwrap();

    let boot_source_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/boot-source")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"kernel_image_path":"{}"}}"#,
                    kernel.path().display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

    start_instance(router).await;
}

async fn assert_invalid_state(
    router: axum::Router,
    method: &str,
    path: &str,
    firecracker_revision: &str,
) {
    let response = router
        .oneshot(
            Request::builder()
                .method(method)
                .uri(concrete_path(path))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "{method} {path}"
    );
    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["code"], "InvalidState", "{method} {path}");
    assert_eq!(
        payload["baseline_firecracker_revision"].as_str().unwrap(),
        firecracker_revision,
        "{method} {path}"
    );
}

#[tokio::test]
async fn all_pinned_routes_are_reachable() {
    let (router, firecracker) = app_router();
    let routes = vesper::spec::load_routes("third_party/firecracker/firecracker.yaml").unwrap();

    for route in routes {
        let body = if route.method == "PUT" && route.path == "/actions" {
            Body::from(r#"{"action_type":"FlushMetrics"}"#)
        } else {
            Body::empty()
        };

        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(route.method.as_str())
                    .uri(concrete_path(&route.path))
                    .body(body)
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_ne!(response.status(), StatusCode::NOT_FOUND);

        if response.status().is_success() {
            continue;
        }

        let payload: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert_eq!(
            payload["baseline_firecracker_revision"].as_str().unwrap(),
            firecracker
        );
    }
}

#[tokio::test]
async fn balloon_get_rejects_preboot_reads_without_configured_device_on_full_router() {
    let (router, firecracker) = app_router();

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/balloon")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["code"], "InvalidRequest");
    assert_eq!(
        payload["fault_message"],
        "Balloon config error: No balloon device found."
    );
    assert_eq!(
        payload["baseline_firecracker_revision"].as_str().unwrap(),
        firecracker
    );
}

#[tokio::test]
async fn balloon_get_returns_preboot_configured_defaults_on_full_router() {
    let (router, _) = app_router();

    let put_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/balloon")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"amount_mib":1,"deflate_on_oom":true}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_response.status(), StatusCode::NO_CONTENT);

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/balloon")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["amount_mib"], 1);
    assert_eq!(payload["deflate_on_oom"], true);
    assert_eq!(payload["stats_polling_interval_s"], 0);
    assert_eq!(payload["free_page_hinting"], false);
    assert_eq!(payload["free_page_reporting"], false);
}

#[tokio::test]
async fn balloon_config_matches_pinned_store_behavior() {
    let (router, firecracker) = app_router();

    let unconfigured_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/balloon")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(unconfigured_response.status(), StatusCode::BAD_REQUEST);
    let unconfigured_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(unconfigured_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(unconfigured_payload["code"], "InvalidRequest");
    assert_eq!(
        unconfigured_payload["fault_message"],
        "Balloon config error: No balloon device found."
    );
    assert_eq!(
        unconfigured_payload["baseline_firecracker_revision"]
            .as_str()
            .unwrap(),
        firecracker
    );

    let oversized_put_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/balloon")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"amount_mib":129,"deflate_on_oom":true}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(oversized_put_response.status(), StatusCode::BAD_REQUEST);
    let oversized_put_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(oversized_put_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(oversized_put_payload["code"], "InvalidRequest");
    assert_eq!(
        oversized_put_payload["fault_message"],
        "Balloon config error: Amount of pages requested is too large."
    );
    assert_eq!(
        oversized_put_payload["baseline_firecracker_revision"]
            .as_str()
            .unwrap(),
        firecracker
    );

    let put_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/balloon")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"amount_mib":1,"deflate_on_oom":true}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_response.status(), StatusCode::NO_CONTENT);

    let get_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/balloon")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get_response.status(), StatusCode::OK);
    let get_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(get_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(get_payload["amount_mib"], 1);
    assert_eq!(get_payload["deflate_on_oom"], true);
    assert_eq!(get_payload["stats_polling_interval_s"], 0);
    assert_eq!(get_payload["free_page_hinting"], false);
    assert_eq!(get_payload["free_page_reporting"], false);

    let vm_config_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/vm/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vm_config_response.status(), StatusCode::OK);
    let vm_config_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(vm_config_payload["balloon"]["amount_mib"], 1);
    assert_eq!(vm_config_payload["balloon"]["deflate_on_oom"], true);
    assert_eq!(vm_config_payload["balloon"]["stats_polling_interval_s"], 0);
    assert_eq!(vm_config_payload["balloon"]["free_page_hinting"], false);
    assert_eq!(vm_config_payload["balloon"]["free_page_reporting"], false);
}

#[tokio::test]
async fn balloon_runtime_read_and_update_require_configured_device() {
    let (router, firecracker) = app_router();
    boot_instance(router.clone()).await;

    let get_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/balloon")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get_response.status(), StatusCode::BAD_REQUEST);
    let get_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(get_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(get_payload["code"], "InvalidRequest");
    assert_eq!(
        get_payload["fault_message"],
        "Internal VMM error: Failed perform action on device: Device not found"
    );
    assert_eq!(
        get_payload["baseline_firecracker_revision"]
            .as_str()
            .unwrap(),
        firecracker
    );

    let patch_response = router
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/balloon")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"amount_mib":1}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(patch_response.status(), StatusCode::BAD_REQUEST);
    let patch_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(patch_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(patch_payload["code"], "InvalidRequest");
    assert_eq!(
        patch_payload["fault_message"],
        "Balloon update error: Failed perform action on device: Device not found"
    );
    assert_eq!(
        patch_payload["baseline_firecracker_revision"]
            .as_str()
            .unwrap(),
        firecracker
    );
}

#[tokio::test]
async fn balloon_get_rejects_runtime_reads_without_configured_device_on_full_router() {
    let (router, firecracker) = app_router();
    boot_instance(router.clone()).await;

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/balloon")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["code"], "InvalidRequest");
    assert_eq!(
        payload["fault_message"],
        "Internal VMM error: Failed perform action on device: Device not found"
    );
    assert_eq!(
        payload["baseline_firecracker_revision"].as_str().unwrap(),
        firecracker
    );
}

#[tokio::test]
async fn balloon_get_returns_runtime_updated_target_on_full_router() {
    let (router, _) = app_router();

    let put_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/balloon")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"amount_mib":1,"deflate_on_oom":true}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_response.status(), StatusCode::NO_CONTENT);

    boot_instance(router.clone()).await;

    let patch_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/balloon")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"amount_mib":64}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(patch_response.status(), StatusCode::NO_CONTENT);

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/balloon")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["amount_mib"], 64);
    assert_eq!(payload["deflate_on_oom"], true);
}

#[tokio::test]
async fn balloon_runtime_update_matches_pinned_behavior() {
    let (router, firecracker) = app_router();

    let put_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/balloon")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"amount_mib":1,"deflate_on_oom":true}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_response.status(), StatusCode::NO_CONTENT);

    boot_instance(router.clone()).await;

    let patch_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/balloon")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"amount_mib":64}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(patch_response.status(), StatusCode::NO_CONTENT);

    let get_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/balloon")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get_response.status(), StatusCode::OK);
    let get_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(get_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(get_payload["amount_mib"], 64);
    assert_eq!(get_payload["deflate_on_oom"], true);

    let oversized_response = router
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/balloon")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"amount_mib":129}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(oversized_response.status(), StatusCode::BAD_REQUEST);
    let oversized_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(oversized_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(oversized_payload["code"], "InvalidRequest");
    assert_eq!(
        oversized_payload["fault_message"],
        "Balloon update error: Balloon: Requested memory should be less than 128MiB"
    );
    assert_eq!(
        oversized_payload["baseline_firecracker_revision"]
            .as_str()
            .unwrap(),
        firecracker
    );
}

#[tokio::test]
async fn balloon_patch_rejects_preboot_updates_on_full_router() {
    let (router, firecracker) = app_router();

    let response = router
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/balloon")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"amount_mib":64}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["code"], "InvalidState");
    assert_eq!(
        payload["fault_message"],
        "The requested operation is not supported before starting the microVM."
    );
    assert_eq!(payload["baseline_firecracker_revision"], firecracker);
}

#[tokio::test]
async fn balloon_stats_get_rejects_preboot_reads_on_full_router() {
    let (router, firecracker) = app_router();

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/balloon/statistics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["code"], "InvalidState");
    assert_eq!(
        payload["fault_message"],
        "The requested operation is not supported before starting the microVM."
    );
    assert_eq!(payload["baseline_firecracker_revision"], firecracker);
}

#[tokio::test]
async fn balloon_stats_patch_rejects_preboot_updates_on_full_router() {
    let (router, firecracker) = app_router();

    let response = router
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/balloon/statistics")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"stats_polling_interval_s":1}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["code"], "InvalidState");
    assert_eq!(
        payload["fault_message"],
        "The requested operation is not supported before starting the microVM."
    );
    assert_eq!(payload["baseline_firecracker_revision"], firecracker);
}

#[tokio::test]
async fn balloon_stats_require_configured_device_and_enabled_preboot_stats() {
    let (router, firecracker) = app_router();
    boot_instance(router.clone()).await;

    let get_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/balloon/statistics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get_response.status(), StatusCode::BAD_REQUEST);
    let get_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(get_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(get_payload["code"], "InvalidRequest");
    assert_eq!(
        get_payload["fault_message"],
        "Internal VMM error: Failed perform action on device: Device not found"
    );
    assert_eq!(
        get_payload["baseline_firecracker_revision"]
            .as_str()
            .unwrap(),
        firecracker
    );

    let patch_response = router
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/balloon/statistics")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"stats_polling_interval_s":1}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(patch_response.status(), StatusCode::BAD_REQUEST);
    let patch_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(patch_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(patch_payload["code"], "InvalidRequest");
    assert_eq!(
        patch_payload["fault_message"],
        "Balloon update error: Failed perform action on device: Device not found"
    );
    assert_eq!(
        patch_payload["baseline_firecracker_revision"]
            .as_str()
            .unwrap(),
        firecracker
    );

    let (router, firecracker) = app_router();
    let put_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/balloon")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"amount_mib":64,"deflate_on_oom":true,"stats_polling_interval_s":0}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_response.status(), StatusCode::NO_CONTENT);

    boot_instance(router.clone()).await;

    let disabled_get_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/balloon/statistics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(disabled_get_response.status(), StatusCode::BAD_REQUEST);
    let disabled_get_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(disabled_get_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(disabled_get_payload["code"], "InvalidRequest");
    assert_eq!(
        disabled_get_payload["fault_message"],
        "Internal VMM error: Balloon: Received stats query when stats are disabled."
    );
    assert_eq!(
        disabled_get_payload["baseline_firecracker_revision"]
            .as_str()
            .unwrap(),
        firecracker
    );

    let disabled_patch_response = router
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/balloon/statistics")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"stats_polling_interval_s":1}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(disabled_patch_response.status(), StatusCode::BAD_REQUEST);
    let disabled_patch_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(disabled_patch_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(disabled_patch_payload["code"], "InvalidRequest");
    assert_eq!(
        disabled_patch_payload["fault_message"],
        "Balloon update error: Balloon: Statistics cannot be enabled/disabled after activation."
    );
    assert_eq!(
        disabled_patch_payload["baseline_firecracker_revision"]
            .as_str()
            .unwrap(),
        firecracker
    );
}

#[tokio::test]
async fn balloon_stats_get_rejects_runtime_reads_without_configured_device_on_full_router() {
    let (router, firecracker) = app_router();
    boot_instance(router.clone()).await;

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/balloon/statistics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["code"], "InvalidRequest");
    assert_eq!(
        payload["fault_message"],
        "Internal VMM error: Failed perform action on device: Device not found"
    );
    assert_eq!(
        payload["baseline_firecracker_revision"].as_str().unwrap(),
        firecracker
    );
}

#[tokio::test]
async fn balloon_stats_get_rejects_runtime_reads_when_disabled_on_full_router() {
    let (router, firecracker) = app_router();

    let put_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/balloon")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"amount_mib":64,"deflate_on_oom":true,"stats_polling_interval_s":0}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_response.status(), StatusCode::NO_CONTENT);

    boot_instance(router.clone()).await;

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/balloon/statistics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["code"], "InvalidRequest");
    assert_eq!(
        payload["fault_message"],
        "Internal VMM error: Balloon: Received stats query when stats are disabled."
    );
    assert_eq!(
        payload["baseline_firecracker_revision"].as_str().unwrap(),
        firecracker
    );
}

#[tokio::test]
async fn balloon_stats_get_returns_enabled_runtime_stats_on_full_router() {
    let (router, _) = app_router();

    let put_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/balloon")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"amount_mib":64,"deflate_on_oom":true,"stats_polling_interval_s":1}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_response.status(), StatusCode::NO_CONTENT);

    boot_instance(router.clone()).await;

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/balloon/statistics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["target_mib"], 64);
    assert_eq!(payload["actual_mib"], 64);
    assert_eq!(payload["target_pages"], 16_384);
    assert_eq!(payload["actual_pages"], 16_384);
}

#[tokio::test]
async fn balloon_stats_update_matches_pinned_behavior() {
    let (router, firecracker) = app_router();

    let put_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/balloon")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"amount_mib":64,"deflate_on_oom":true,"stats_polling_interval_s":1}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_response.status(), StatusCode::NO_CONTENT);

    boot_instance(router.clone()).await;

    let get_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/balloon/statistics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get_response.status(), StatusCode::OK);
    let get_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(get_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(get_payload["target_mib"], 64);
    assert_eq!(get_payload["actual_mib"], 64);
    assert_eq!(get_payload["target_pages"], 16_384);
    assert_eq!(get_payload["actual_pages"], 16_384);

    let patch_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/balloon/statistics")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"stats_polling_interval_s":2}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(patch_response.status(), StatusCode::NO_CONTENT);

    let disable_response = router
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/balloon/statistics")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"stats_polling_interval_s":0}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(disable_response.status(), StatusCode::BAD_REQUEST);
    let disable_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(disable_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(disable_payload["code"], "InvalidRequest");
    assert_eq!(
        disable_payload["fault_message"],
        "Balloon update error: Balloon: Statistics cannot be enabled/disabled after activation."
    );
    assert_eq!(
        disable_payload["baseline_firecracker_revision"]
            .as_str()
            .unwrap(),
        firecracker
    );
}

#[tokio::test]
async fn balloon_hinting_status_rejects_preboot_reads_on_full_router() {
    let (router, firecracker) = app_router();

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/balloon/hinting/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["code"], "InvalidState");
    assert_eq!(
        payload["fault_message"],
        "The requested operation is not supported before starting the microVM."
    );
    assert_eq!(payload["baseline_firecracker_revision"], firecracker);
}

#[tokio::test]
async fn balloon_hinting_start_rejects_preboot_updates_on_full_router() {
    let (router, firecracker) = app_router();

    let response = router
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/balloon/hinting/start")
                .header("content-type", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["code"], "InvalidState");
    assert_eq!(
        payload["fault_message"],
        "The requested operation is not supported before starting the microVM."
    );
    assert_eq!(payload["baseline_firecracker_revision"], firecracker);
}

#[tokio::test]
async fn balloon_hinting_stop_rejects_preboot_updates_on_full_router() {
    let (router, firecracker) = app_router();

    let response = router
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/balloon/hinting/stop")
                .header("content-type", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["code"], "InvalidState");
    assert_eq!(
        payload["fault_message"],
        "The requested operation is not supported before starting the microVM."
    );
    assert_eq!(payload["baseline_firecracker_revision"], firecracker);
}

#[tokio::test]
async fn balloon_hinting_status_rejects_runtime_reads_without_configured_device_on_full_router() {
    let (router, firecracker) = app_router();
    boot_instance(router.clone()).await;

    let unconfigured_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/balloon/hinting/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unconfigured_response.status(), StatusCode::BAD_REQUEST);
    let unconfigured_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(unconfigured_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(unconfigured_payload["code"], "InvalidRequest");
    assert_eq!(
        unconfigured_payload["fault_message"],
        "Balloon update error: Failed perform action on device: Device not found"
    );
    assert_eq!(
        unconfigured_payload["baseline_firecracker_revision"]
            .as_str()
            .unwrap(),
        firecracker
    );
}

#[tokio::test]
async fn balloon_hinting_status_rejects_runtime_reads_when_disabled_on_full_router() {
    let (router, firecracker) = app_router();
    let put_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/balloon")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"amount_mib":64,"deflate_on_oom":true,"free_page_hinting":false}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_response.status(), StatusCode::NO_CONTENT);

    boot_instance(router.clone()).await;

    let disabled_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/balloon/hinting/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(disabled_response.status(), StatusCode::BAD_REQUEST);
    let disabled_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(disabled_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(disabled_payload["code"], "InvalidRequest");
    assert_eq!(
        disabled_payload["fault_message"],
        "Balloon update error: Balloon: Attempting to use hinting when not enabled"
    );
    assert_eq!(
        disabled_payload["baseline_firecracker_revision"]
            .as_str()
            .unwrap(),
        firecracker
    );
}

#[tokio::test]
async fn balloon_hinting_status_returns_idle_runtime_status_on_full_router() {
    let (router, _) = app_router();

    let put_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/balloon")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"amount_mib":64,"deflate_on_oom":true,"free_page_hinting":true}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_response.status(), StatusCode::NO_CONTENT);

    boot_instance(router.clone()).await;

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/balloon/hinting/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["host_cmd"], 0);
    assert!(payload["guest_cmd"].is_null());
}

#[tokio::test]
async fn balloon_hinting_control_requires_configured_enabled_device() {
    let (router, firecracker) = app_router();
    boot_instance(router.clone()).await;

    for (path, body) in [
        ("/balloon/hinting/start", Body::empty()),
        ("/balloon/hinting/stop", Body::from("{not-json")),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(path)
                    .header("content-type", "application/json")
                    .body(body)
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let payload: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert_eq!(payload["code"], "InvalidRequest");
        assert_eq!(
            payload["fault_message"],
            "Balloon update error: Failed perform action on device: Device not found"
        );
        assert_eq!(
            payload["baseline_firecracker_revision"].as_str().unwrap(),
            firecracker
        );
    }

    let (router, firecracker) = app_router();
    let put_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/balloon")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"amount_mib":64,"deflate_on_oom":true,"free_page_hinting":false}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_response.status(), StatusCode::NO_CONTENT);

    boot_instance(router.clone()).await;

    for path in ["/balloon/hinting/start", "/balloon/hinting/stop"] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(path)
                    .header("content-type", "application/json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let payload: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert_eq!(payload["code"], "InvalidRequest");
        assert_eq!(
            payload["fault_message"],
            "Balloon update error: Balloon: Attempting to use hinting when not enabled"
        );
        assert_eq!(
            payload["baseline_firecracker_revision"].as_str().unwrap(),
            firecracker
        );
    }
}

#[tokio::test]
async fn balloon_hinting_control_matches_pinned_low_risk_flow() {
    let (router, _) = app_router();

    let put_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/balloon")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"amount_mib":64,"deflate_on_oom":true,"free_page_hinting":true}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_response.status(), StatusCode::NO_CONTENT);

    boot_instance(router.clone()).await;

    let first_start_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/balloon/hinting/start")
                .header("content-type", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(first_start_response.status(), StatusCode::NO_CONTENT);

    let first_status_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/balloon/hinting/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(first_status_response.status(), StatusCode::OK);
    let first_status: serde_json::Value = serde_json::from_slice(
        &to_bytes(first_status_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(first_status["host_cmd"], 2);
    assert!(first_status["guest_cmd"].is_null());

    let second_start_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/balloon/hinting/start")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"acknowledge_on_stop":false}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(second_start_response.status(), StatusCode::NO_CONTENT);

    let second_status_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/balloon/hinting/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(second_status_response.status(), StatusCode::OK);
    let second_status: serde_json::Value = serde_json::from_slice(
        &to_bytes(second_status_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(second_status["host_cmd"], 3);
    assert!(second_status["guest_cmd"].is_null());

    let stop_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/balloon/hinting/stop")
                .header("content-type", "application/json")
                .body(Body::from("{not-json"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(stop_response.status(), StatusCode::NO_CONTENT);

    let stopped_status_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/balloon/hinting/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(stopped_status_response.status(), StatusCode::OK);
    let stopped_status: serde_json::Value = serde_json::from_slice(
        &to_bytes(stopped_status_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(stopped_status["host_cmd"], 1);
    assert!(stopped_status["guest_cmd"].is_null());
}

#[tokio::test]
async fn machine_config_cannot_shrink_below_balloon_target() {
    let (router, firecracker) = app_router();

    let put_balloon_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/balloon")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"amount_mib":64,"deflate_on_oom":true}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_balloon_response.status(), StatusCode::NO_CONTENT);

    for request in [
        Request::builder()
            .method("PATCH")
            .uri("/machine-config")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"mem_size_mib":32}"#))
            .unwrap(),
        Request::builder()
            .method("PUT")
            .uri("/machine-config")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"vcpu_count":1,"mem_size_mib":32}"#))
            .unwrap(),
    ] {
        let response = router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let payload: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert_eq!(payload["code"], "InvalidRequest");
        assert_eq!(
            payload["fault_message"],
            "The memory size (MiB) is smaller than the previously set balloon device target size."
        );
        assert_eq!(
            payload["baseline_firecracker_revision"].as_str().unwrap(),
            firecracker
        );
    }
}

#[tokio::test]
async fn documented_preboot_only_routes_reject_after_start() {
    let (router, firecracker) = app_router();
    boot_instance(router.clone()).await;

    for (method, path) in PREBOOT_ONLY_ROUTES {
        assert_invalid_state(router.clone(), method, path, &firecracker).await;
    }
}

#[tokio::test]
async fn documented_postboot_only_routes_reject_before_start() {
    let (router, firecracker) = app_router();

    for (method, path) in POSTBOOT_ONLY_ROUTES {
        assert_invalid_state(router.clone(), method, path, &firecracker).await;
    }
}

#[tokio::test]
async fn logger_metrics_and_serial_remain_callable_after_start() {
    let (router, _) = app_router();
    let tempdir = tempdir().unwrap();
    let metrics_path = tempdir.path().join("metrics-after-start.jsonl");

    boot_instance(router.clone()).await;

    for (path, body) in [
        ("/logger", r#"{"level":"info"}"#.to_string()),
        (
            "/metrics",
            format!(r#"{{"metrics_path":"{}"}}"#, metrics_path.display()),
        ),
        ("/serial", r#"{"serial_out_path":"serial.log"}"#.to_string()),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(path)
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT, "{path}");
    }
}

#[tokio::test]
async fn vsock_put_matches_pinned_store_behavior() {
    let (router, _) = app_router();

    let vsock_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/vsock")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"guest_cid":5,"uds_path":"/tmp/vesper-vsock.sock","vsock_id":"deprecated"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vsock_response.status(), StatusCode::NO_CONTENT);

    let vm_config_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/vm/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vm_config_response.status(), StatusCode::OK);
    let vm_config_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(vm_config_payload["vsock"]["guest_cid"], 5);
    assert_eq!(
        vm_config_payload["vsock"]["uds_path"],
        "/tmp/vesper-vsock.sock"
    );
    assert!(vm_config_payload["vsock"].get("vsock_id").is_none());
}

#[tokio::test]
async fn vm_config_vsock_export_remains_available_after_boot_on_full_router() {
    let (router, _) = app_router();

    let vsock_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/vsock")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"guest_cid":5,"uds_path":"/tmp/vesper-vsock.sock","vsock_id":"deprecated"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vsock_response.status(), StatusCode::NO_CONTENT);

    boot_instance(router.clone()).await;

    let vm_config_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/vm/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vm_config_response.status(), StatusCode::OK);
    let vm_config_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(vm_config_payload["vsock"]["guest_cid"], 5);
    assert_eq!(
        vm_config_payload["vsock"]["uds_path"],
        "/tmp/vesper-vsock.sock"
    );
    assert!(vm_config_payload["vsock"].get("vsock_id").is_none());
}

#[tokio::test]
async fn vsock_put_rejects_invalid_guest_cid_on_full_router() {
    let (router, firecracker) = app_router();

    let response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/vsock")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"guest_cid":2,"uds_path":"/tmp/vesper-vsock.sock"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["code"], "InvalidRequest");
    assert_eq!(
        payload["fault_message"],
        "Guest CID must be greater than or equal to 3."
    );
    assert_eq!(payload["baseline_firecracker_revision"], firecracker);
}

#[tokio::test]
async fn vsock_put_rejects_postboot_updates_on_full_router() {
    let (router, firecracker) = app_router();
    let firecracker = firecracker.to_string();

    boot_instance(router.clone()).await;

    let response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/vsock")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"guest_cid":5,"uds_path":"/tmp/vesper-vsock.sock"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["code"], "InvalidState");
    assert_eq!(
        payload["fault_message"],
        "The requested operation is not supported after starting the microVM."
    );
    assert_eq!(
        payload["baseline_firecracker_revision"].as_str().unwrap(),
        firecracker
    );
}

#[tokio::test]
async fn entropy_put_matches_pinned_store_behavior() {
    let (router, _) = app_router();

    let entropy_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/entropy")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"rate_limiter":{"bandwidth":{"size":4096,"refill_time":1000,"one_time_burst":512},"ops":{"size":32,"refill_time":100}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(entropy_response.status(), StatusCode::NO_CONTENT);

    let vm_config_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/vm/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vm_config_response.status(), StatusCode::OK);
    let vm_config_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        vm_config_payload["entropy"]["rate_limiter"]["bandwidth"]["size"],
        4096
    );
    assert_eq!(
        vm_config_payload["entropy"]["rate_limiter"]["bandwidth"]["refill_time"],
        1000
    );
    assert_eq!(
        vm_config_payload["entropy"]["rate_limiter"]["bandwidth"]["one_time_burst"],
        512
    );
    assert_eq!(
        vm_config_payload["entropy"]["rate_limiter"]["ops"]["size"],
        32
    );
    assert_eq!(
        vm_config_payload["entropy"]["rate_limiter"]["ops"]["refill_time"],
        100
    );
    assert!(
        vm_config_payload["entropy"]["rate_limiter"]["ops"]
            .get("one_time_burst")
            .is_none()
    );
}

#[tokio::test]
async fn entropy_put_rejects_postboot_updates_on_full_router() {
    let (router, firecracker) = app_router();
    let firecracker = firecracker.to_string();
    let kernel = NamedTempFile::new().unwrap();

    let boot_source_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/boot-source")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"kernel_image_path":"{}"}}"#,
                    kernel.path().display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

    let start_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/actions")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"action_type":"InstanceStart"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

    let response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/entropy")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"rate_limiter":{"bandwidth":{"size":4096,"refill_time":1000,"one_time_burst":512},"ops":{"size":32,"refill_time":100}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["code"], "InvalidState");
    assert_eq!(
        payload["fault_message"],
        "The requested operation is not supported after starting the microVM."
    );
    assert_eq!(
        payload["baseline_firecracker_revision"].as_str().unwrap(),
        firecracker
    );
}

#[tokio::test]
async fn boot_source_put_matches_pinned_store_behavior() {
    let (router, _) = app_router();
    let kernel = NamedTempFile::new().unwrap();
    let initrd = NamedTempFile::new().unwrap();

    let boot_source_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/boot-source")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"kernel_image_path":"{}","initrd_path":"{}","boot_args":"console=ttyS0 reboot=k panic=1 pci=off"}}"#,
                    kernel.path().display(),
                    initrd.path().display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

    let vm_config_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/vm/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vm_config_response.status(), StatusCode::OK);
    let vm_config_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        vm_config_payload["boot-source"]["kernel_image_path"],
        kernel.path().to_str().unwrap()
    );
    assert_eq!(
        vm_config_payload["boot-source"]["initrd_path"],
        initrd.path().to_str().unwrap()
    );
    assert_eq!(
        vm_config_payload["boot-source"]["boot_args"],
        "console=ttyS0 reboot=k panic=1 pci=off"
    );
}

#[tokio::test]
async fn vm_config_boot_source_export_remains_available_after_boot_on_full_router() {
    let (router, _) = app_router();
    let kernel = NamedTempFile::new().unwrap();
    let initrd = NamedTempFile::new().unwrap();

    let boot_source_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/boot-source")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"kernel_image_path":"{}","initrd_path":"{}","boot_args":"console=ttyS0 reboot=k panic=1 pci=off"}}"#,
                    kernel.path().display(),
                    initrd.path().display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

    start_instance(router.clone()).await;

    let vm_config_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/vm/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vm_config_response.status(), StatusCode::OK);
    let vm_config_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        vm_config_payload["boot-source"]["kernel_image_path"],
        kernel.path().to_str().unwrap()
    );
    assert_eq!(
        vm_config_payload["boot-source"]["initrd_path"],
        initrd.path().to_str().unwrap()
    );
    assert_eq!(
        vm_config_payload["boot-source"]["boot_args"],
        "console=ttyS0 reboot=k panic=1 pci=off"
    );
}

#[tokio::test]
async fn boot_source_put_rejects_postboot_updates_on_full_router() {
    let (router, firecracker) = app_router();
    let kernel = NamedTempFile::new().unwrap();

    let boot_source_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/boot-source")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"kernel_image_path":"{}"}}"#,
                    kernel.path().display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

    let start_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/actions")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"action_type":"InstanceStart"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

    let response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/boot-source")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"kernel_image_path":"{}"}}"#,
                    kernel.path().display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["code"], "InvalidState");
    assert_eq!(
        payload["fault_message"],
        "The requested operation is not supported after starting the microVM."
    );
    assert_eq!(
        payload["baseline_firecracker_revision"].as_str().unwrap(),
        firecracker
    );
}

#[tokio::test]
async fn machine_config_put_patch_matches_pinned_store_behavior() {
    let (router, _) = app_router();

    let put_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/machine-config")
                .header("content-type", "application/json")
                .body(Body::from(
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
                .body(Body::from(r#"{"mem_size_mib":512}"#))
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
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(machine_response.status(), StatusCode::OK);
    let machine_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(machine_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(machine_payload["vcpu_count"], 2);
    assert_eq!(machine_payload["mem_size_mib"], 512);
    assert_eq!(machine_payload["track_dirty_pages"], true);

    let vm_config_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/vm/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vm_config_response.status(), StatusCode::OK);
    let vm_config_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(vm_config_payload["machine-config"]["vcpu_count"], 2);
    assert_eq!(vm_config_payload["machine-config"]["mem_size_mib"], 512);
    assert_eq!(
        vm_config_payload["machine-config"]["track_dirty_pages"],
        true
    );
}

#[tokio::test]
async fn machine_config_read_remains_available_after_boot_on_full_router() {
    let (router, _) = app_router();

    let put_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/machine-config")
                .header("content-type", "application/json")
                .body(Body::from(
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
                .body(Body::from(r#"{"mem_size_mib":512}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(patch_response.status(), StatusCode::NO_CONTENT);

    boot_instance(router.clone()).await;

    let machine_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/machine-config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(machine_response.status(), StatusCode::OK);
    let machine_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(machine_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(machine_payload["vcpu_count"], 2);
    assert_eq!(machine_payload["mem_size_mib"], 512);
    assert_eq!(machine_payload["track_dirty_pages"], true);
}

#[tokio::test]
async fn vm_config_machine_export_remains_available_after_boot_on_full_router() {
    let (router, _) = app_router();

    let put_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/machine-config")
                .header("content-type", "application/json")
                .body(Body::from(
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
                .body(Body::from(r#"{"mem_size_mib":512}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(patch_response.status(), StatusCode::NO_CONTENT);

    boot_instance(router.clone()).await;

    let vm_config_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/vm/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vm_config_response.status(), StatusCode::OK);
    let vm_config_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(vm_config_payload["machine-config"]["vcpu_count"], 2);
    assert_eq!(vm_config_payload["machine-config"]["mem_size_mib"], 512);
    assert_eq!(
        vm_config_payload["machine-config"]["track_dirty_pages"],
        true
    );
}

#[tokio::test]
async fn cpu_config_put_matches_pinned_store_behavior_without_vm_config_export() {
    let (router, _) = app_router();

    let cpu_config_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/cpu-config")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"kvm_capabilities":["1","!2"],"cpuid_modifiers":[{"leaf":"0x1","subleaf":"0x0","flags":0,"modifiers":[{"register":"eax","bitmap":"0bxxxxxxxxxxxxxxxxxxxxxxxxxxxx0001"}]}],"msr_modifiers":[{"addr":"0x10a","bitmap":"0b0000000000000000000000000000000000000000000000000000000000000001"}],"reg_modifiers":[{"addr":"0x0","bitmap":"0b1"}],"vcpu_features":[{"index":0,"bitmap":"0b00000000000000000000000001100000"}]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(cpu_config_response.status(), StatusCode::NO_CONTENT);

    let vm_config_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/vm/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vm_config_response.status(), StatusCode::OK);
    let vm_config_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(vm_config_payload.get("cpu-config").is_none());
}

#[tokio::test]
async fn cpu_config_put_rejects_postboot_updates_on_full_router() {
    let (router, firecracker) = app_router();
    let kernel = NamedTempFile::new().unwrap();

    let boot_source_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/boot-source")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"kernel_image_path":"{}"}}"#,
                    kernel.path().display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

    let start_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/actions")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"action_type":"InstanceStart"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

    let response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/cpu-config")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"kvm_capabilities":["1","!2"],"cpuid_modifiers":[{"leaf":"0x1","subleaf":"0x0","flags":0,"modifiers":[{"register":"eax","bitmap":"0bxxxxxxxxxxxxxxxxxxxxxxxxxxxx0001"}]}],"msr_modifiers":[{"addr":"0x10a","bitmap":"0b0000000000000000000000000000000000000000000000000000000000000001"}],"reg_modifiers":[{"addr":"0x0","bitmap":"0b1"}],"vcpu_features":[{"index":0,"bitmap":"0b00000000000000000000000001100000"}]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["code"], "InvalidState");
    assert_eq!(
        payload["fault_message"],
        "The requested operation is not supported after starting the microVM."
    );
    assert_eq!(
        payload["baseline_firecracker_revision"].as_str().unwrap(),
        firecracker
    );
}

#[tokio::test]
async fn serial_put_matches_pinned_store_behavior_without_vm_config_export() {
    let (router, _) = app_router();

    let serial_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/serial")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"serial_out_path":"serial.log"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(serial_response.status(), StatusCode::NO_CONTENT);

    let vm_config_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/vm/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vm_config_response.status(), StatusCode::OK);
    let vm_config_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(vm_config_payload.get("serial").is_none());
}

#[tokio::test]
async fn serial_put_remains_available_after_boot_on_full_router() {
    let (router, _) = app_router();

    boot_instance(router.clone()).await;

    let serial_response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/serial")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"serial_out_path":"serial.log"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(serial_response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn vm_config_serial_omission_persists_after_boot_on_full_router() {
    let (router, _) = app_router();

    boot_instance(router.clone()).await;

    let serial_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/serial")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"serial_out_path":"serial.log"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(serial_response.status(), StatusCode::NO_CONTENT);

    let vm_config_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/vm/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vm_config_response.status(), StatusCode::OK);
    let vm_config_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(vm_config_payload.get("serial").is_none());
}

#[tokio::test]
async fn snapshot_load_succeeds_after_serial_configuration_on_full_router() {
    let (router, _) = app_router();

    let serial_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/serial")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"serial_out_path":"serial.log"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(serial_response.status(), StatusCode::NO_CONTENT);

    let snapshot_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/snapshot/load")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"snapshot_path":"state.snap","mem_backend":{"backend_type":"File","backend_path":"mem.snap"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(snapshot_response.status(), StatusCode::NO_CONTENT);

    let instance_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(instance_response.status(), StatusCode::OK);
    let instance_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(instance_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(instance_payload["state"], "Paused");
}

#[tokio::test]
async fn snapshot_load_succeeds_after_logger_metrics_and_serial_on_full_router() {
    let (router, _) = app_router();
    let tempdir = tempdir().unwrap();
    let metrics_path = tempdir.path().join("snapshot-load-metrics.jsonl");

    let logger_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/logger")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"level":"info"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(logger_response.status(), StatusCode::NO_CONTENT);

    let metrics_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/metrics")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"metrics_path":"{}"}}"#,
                    metrics_path.display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(metrics_response.status(), StatusCode::NO_CONTENT);

    let serial_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/serial")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"serial_out_path":"serial.log"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(serial_response.status(), StatusCode::NO_CONTENT);

    let snapshot_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/snapshot/load")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"snapshot_path":"state.snap","mem_backend":{"backend_type":"File","backend_path":"mem.snap"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(snapshot_response.status(), StatusCode::NO_CONTENT);

    let instance_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(instance_response.status(), StatusCode::OK);
    let instance_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(instance_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(instance_payload["state"], "Paused");
}

#[tokio::test]
async fn snapshot_create_rejects_preboot_requests_on_full_router() {
    let (router, firecracker) = app_router();

    let response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/snapshot/create")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"mem_file_path":"mem.snap","snapshot_path":"state.snap"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["code"], "InvalidState");
    assert_eq!(
        payload["fault_message"],
        "The requested operation is not supported before starting the microVM."
    );
    assert_eq!(payload["baseline_firecracker_revision"], firecracker);
}

#[tokio::test]
async fn snapshot_create_requires_paused_vm_on_full_router() {
    let (router, firecracker) = app_router();

    boot_instance(router.clone()).await;

    let running_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/snapshot/create")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"mem_file_path":"mem.snap","snapshot_path":"state.snap"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(running_response.status(), StatusCode::BAD_REQUEST);
    let running_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(running_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(running_payload["code"], "InvalidState");
    assert_eq!(
        running_payload["fault_message"],
        "operation PUT /snapshot/create is not allowed while instance state is running"
    );
    assert_eq!(
        running_payload["baseline_firecracker_revision"],
        firecracker
    );

    let pause_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/vm")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"state":"Paused"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(pause_response.status(), StatusCode::NO_CONTENT);

    let paused_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/snapshot/create")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"mem_file_path":"mem.snap","snapshot_path":"state.snap"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(paused_response.status(), StatusCode::NO_CONTENT);

    let instance_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(instance_response.status(), StatusCode::OK);
    let instance_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(instance_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(instance_payload["state"], "Paused");
}

#[tokio::test]
async fn snapshot_load_resumes_vm_when_requested_on_full_router() {
    let (router, _) = app_router();

    let snapshot_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/snapshot/load")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"snapshot_path":"state.snap","mem_backend":{"backend_type":"File","backend_path":"mem.snap"},"resume_vm":true}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(snapshot_response.status(), StatusCode::NO_CONTENT);

    let instance_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(instance_response.status(), StatusCode::OK);
    let instance_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(instance_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(instance_payload["state"], "Running");
}

#[tokio::test]
async fn snapshot_load_rejects_boot_source_configuration_on_full_router() {
    let (router, firecracker) = app_router();
    let kernel = NamedTempFile::new().unwrap();

    let boot_source_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/boot-source")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"kernel_image_path":"{}"}}"#,
                    kernel.path().display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

    let snapshot_response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/snapshot/load")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"snapshot_path":"state.snap","mem_backend":{"backend_type":"File","backend_path":"mem.snap"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(snapshot_response.status(), StatusCode::BAD_REQUEST);
    let payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(snapshot_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(payload["code"], "InvalidState");
    assert_eq!(
        payload["fault_message"],
        "Load snapshot can only be called before configuring resources other than logger and metrics."
    );
    assert_eq!(payload["baseline_firecracker_revision"], firecracker);
}

#[tokio::test]
async fn snapshot_load_rejects_memory_hotplug_configuration_on_full_router() {
    let (router, firecracker) = app_router();

    let hotplug_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/hotplug/memory")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"total_size_mib":1024}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(hotplug_response.status(), StatusCode::NO_CONTENT);

    let snapshot_response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/snapshot/load")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"snapshot_path":"state.snap","mem_backend":{"backend_type":"File","backend_path":"mem.snap"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(snapshot_response.status(), StatusCode::BAD_REQUEST);
    let payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(snapshot_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(payload["code"], "InvalidState");
    assert_eq!(
        payload["fault_message"],
        "Load snapshot can only be called before configuring resources other than logger and metrics."
    );
    assert_eq!(payload["baseline_firecracker_revision"], firecracker);
}

#[tokio::test]
async fn snapshot_load_rejects_cpu_config_configuration_on_full_router() {
    let (router, firecracker) = app_router();

    let cpu_config_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/cpu-config")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"kvm_capabilities":["1","!2"],"cpuid_modifiers":[{"leaf":"0x1","subleaf":"0x0","flags":0,"modifiers":[{"register":"eax","bitmap":"0bxxxxxxxxxxxxxxxxxxxxxxxxxxxx0001"}]}],"msr_modifiers":[{"addr":"0x10a","bitmap":"0b0000000000000000000000000000000000000000000000000000000000000001"}],"reg_modifiers":[{"addr":"0x0","bitmap":"0b1"}],"vcpu_features":[{"index":0,"bitmap":"0b00000000000000000000000001100000"}]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(cpu_config_response.status(), StatusCode::NO_CONTENT);

    let snapshot_response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/snapshot/load")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"snapshot_path":"state.snap","mem_backend":{"backend_type":"File","backend_path":"mem.snap"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(snapshot_response.status(), StatusCode::BAD_REQUEST);
    let payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(snapshot_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(payload["code"], "InvalidState");
    assert_eq!(
        payload["fault_message"],
        "Load snapshot can only be called before configuring resources other than logger and metrics."
    );
    assert_eq!(payload["baseline_firecracker_revision"], firecracker);
}

#[tokio::test]
async fn snapshot_load_rejects_entropy_configuration_on_full_router() {
    let (router, firecracker) = app_router();

    let entropy_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/entropy")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"rate_limiter":{"bandwidth":{"size":4096,"refill_time":1000,"one_time_burst":512},"ops":{"size":32,"refill_time":100}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(entropy_response.status(), StatusCode::NO_CONTENT);

    let snapshot_response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/snapshot/load")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"snapshot_path":"state.snap","mem_backend":{"backend_type":"File","backend_path":"mem.snap"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(snapshot_response.status(), StatusCode::BAD_REQUEST);
    let payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(snapshot_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(payload["code"], "InvalidState");
    assert_eq!(
        payload["fault_message"],
        "Load snapshot can only be called before configuring resources other than logger and metrics."
    );
    assert_eq!(payload["baseline_firecracker_revision"], firecracker);
}

#[tokio::test]
async fn snapshot_load_rejects_vsock_configuration_on_full_router() {
    let (router, firecracker) = app_router();

    let vsock_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/vsock")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"guest_cid":5,"uds_path":"/tmp/vesper-vsock.sock","vsock_id":"deprecated"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vsock_response.status(), StatusCode::NO_CONTENT);

    let snapshot_response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/snapshot/load")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"snapshot_path":"state.snap","mem_backend":{"backend_type":"File","backend_path":"mem.snap"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(snapshot_response.status(), StatusCode::BAD_REQUEST);
    let payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(snapshot_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(payload["code"], "InvalidState");
    assert_eq!(
        payload["fault_message"],
        "Load snapshot can only be called before configuring resources other than logger and metrics."
    );
    assert_eq!(payload["baseline_firecracker_revision"], firecracker);
}

#[tokio::test]
async fn snapshot_load_rejects_balloon_configuration_on_full_router() {
    let (router, firecracker) = app_router();

    let balloon_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/balloon")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"amount_mib":64,"deflate_on_oom":true}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(balloon_response.status(), StatusCode::NO_CONTENT);

    let snapshot_response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/snapshot/load")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"snapshot_path":"state.snap","mem_backend":{"backend_type":"File","backend_path":"mem.snap"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(snapshot_response.status(), StatusCode::BAD_REQUEST);
    let payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(snapshot_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(payload["code"], "InvalidState");
    assert_eq!(
        payload["fault_message"],
        "Load snapshot can only be called before configuring resources other than logger and metrics."
    );
    assert_eq!(payload["baseline_firecracker_revision"], firecracker);
}

#[tokio::test]
async fn snapshot_load_rejects_network_interface_configuration_on_full_router() {
    let (router, firecracker) = app_router();

    let network_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/network-interfaces/eth0")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"iface_id":"eth0","host_dev_name":"tap0"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(network_response.status(), StatusCode::NO_CONTENT);

    let snapshot_response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/snapshot/load")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"snapshot_path":"state.snap","mem_backend":{"backend_type":"File","backend_path":"mem.snap"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(snapshot_response.status(), StatusCode::BAD_REQUEST);
    let payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(snapshot_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(payload["code"], "InvalidState");
    assert_eq!(
        payload["fault_message"],
        "Load snapshot can only be called before configuring resources other than logger and metrics."
    );
    assert_eq!(payload["baseline_firecracker_revision"], firecracker);
}

#[tokio::test]
async fn snapshot_load_rejects_mmds_configuration_on_full_router() {
    let (router, firecracker) = app_router();

    let network_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/network-interfaces/eth0")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"iface_id":"eth0","host_dev_name":"tap0"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(network_response.status(), StatusCode::NO_CONTENT);

    let mmds_config_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/mmds/config")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"network_interfaces":["eth0"]}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(mmds_config_response.status(), StatusCode::NO_CONTENT);

    let snapshot_response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/snapshot/load")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"snapshot_path":"state.snap","mem_backend":{"backend_type":"File","backend_path":"mem.snap"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(snapshot_response.status(), StatusCode::BAD_REQUEST);
    let payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(snapshot_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(payload["code"], "InvalidState");
    assert_eq!(
        payload["fault_message"],
        "Load snapshot can only be called before configuring resources other than logger and metrics."
    );
    assert_eq!(payload["baseline_firecracker_revision"], firecracker);
}

#[tokio::test]
async fn metrics_put_matches_pinned_store_behavior_without_vm_config_export() {
    let (router, _) = app_router();
    let tempdir = tempdir().unwrap();
    let metrics_path = tempdir.path().join("metrics.jsonl");

    let metrics_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/metrics")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"metrics_path":"{}"}}"#,
                    metrics_path.display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(metrics_response.status(), StatusCode::NO_CONTENT);

    let vm_config_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/vm/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vm_config_response.status(), StatusCode::OK);
    let vm_config_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(vm_config_payload.get("metrics").is_none());
}

#[tokio::test]
async fn metrics_put_remains_available_after_boot_on_full_router() {
    let (router, _) = app_router();
    let tempdir = tempdir().unwrap();
    let metrics_path = tempdir.path().join("metrics-after-boot.jsonl");

    boot_instance(router.clone()).await;

    let metrics_response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/metrics")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"metrics_path":"{}"}}"#,
                    metrics_path.display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(metrics_response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn vm_config_metrics_omission_persists_after_boot_on_full_router() {
    let (router, _) = app_router();
    let tempdir = tempdir().unwrap();
    let metrics_path = tempdir.path().join("metrics-omitted-after-boot.jsonl");

    boot_instance(router.clone()).await;

    let metrics_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/metrics")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"metrics_path":"{}"}}"#,
                    metrics_path.display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(metrics_response.status(), StatusCode::NO_CONTENT);

    let vm_config_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/vm/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vm_config_response.status(), StatusCode::OK);
    let vm_config_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(vm_config_payload.get("metrics").is_none());
}

#[tokio::test]
async fn metrics_put_rejects_reinitialization_on_full_router() {
    let (router, firecracker) = app_router();
    let tempdir = tempdir().unwrap();
    let metrics_path = tempdir.path().join("metrics-reinit.jsonl");
    let metrics_payload = format!(r#"{{"metrics_path":"{}"}}"#, metrics_path.display());

    let first_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/metrics")
                .header("content-type", "application/json")
                .body(Body::from(metrics_payload.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(first_response.status(), StatusCode::NO_CONTENT);

    let duplicate_response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/metrics")
                .header("content-type", "application/json")
                .body(Body::from(metrics_payload))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(duplicate_response.status(), StatusCode::BAD_REQUEST);
    let duplicate_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(duplicate_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(duplicate_payload["code"], "InvalidRequest");
    assert_eq!(
        duplicate_payload["fault_message"],
        "Cannot initialize the metrics system due to bad user input: Reinitialization of metrics not allowed."
    );
    assert_eq!(
        duplicate_payload["baseline_firecracker_revision"],
        firecracker
    );
}

#[tokio::test]
async fn vm_config_entropy_export_remains_available_after_boot_on_full_router() {
    let (router, _) = app_router();

    let entropy_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/entropy")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"rate_limiter":{"bandwidth":{"size":4096,"refill_time":1000,"one_time_burst":512},"ops":{"size":32,"refill_time":100}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(entropy_response.status(), StatusCode::NO_CONTENT);

    boot_instance(router.clone()).await;

    let vm_config_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/vm/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vm_config_response.status(), StatusCode::OK);
    let vm_config_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        vm_config_payload["entropy"]["rate_limiter"]["bandwidth"]["size"],
        4096
    );
    assert_eq!(
        vm_config_payload["entropy"]["rate_limiter"]["bandwidth"]["refill_time"],
        1000
    );
    assert_eq!(
        vm_config_payload["entropy"]["rate_limiter"]["bandwidth"]["one_time_burst"],
        512
    );
    assert_eq!(
        vm_config_payload["entropy"]["rate_limiter"]["ops"]["size"],
        32
    );
    assert_eq!(
        vm_config_payload["entropy"]["rate_limiter"]["ops"]["refill_time"],
        100
    );
    assert!(
        vm_config_payload["entropy"]["rate_limiter"]["ops"]
            .get("one_time_burst")
            .is_none()
    );
}

#[tokio::test]
async fn logger_put_matches_pinned_acceptance_without_vm_config_export() {
    let (router, _) = app_router();

    let logger_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/logger")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"level":"warning","show_level":true}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(logger_response.status(), StatusCode::NO_CONTENT);

    let vm_config_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/vm/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vm_config_response.status(), StatusCode::OK);
    let vm_config_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(vm_config_payload.get("logger").is_none());
}

#[tokio::test]
async fn logger_put_remains_available_after_boot_on_full_router() {
    let (router, _) = app_router();

    boot_instance(router.clone()).await;

    let logger_response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/logger")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"level":"info"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(logger_response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn vm_config_logger_omission_persists_after_boot_on_full_router() {
    let (router, _) = app_router();

    boot_instance(router.clone()).await;

    let logger_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/logger")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"level":"info"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(logger_response.status(), StatusCode::NO_CONTENT);

    let vm_config_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/vm/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vm_config_response.status(), StatusCode::OK);
    let vm_config_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(vm_config_payload.get("logger").is_none());
}

#[tokio::test]
async fn instance_info_reports_preboot_not_started_on_full_router() {
    let (router, _) = app_router();

    let instance_info_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(instance_info_response.status(), StatusCode::OK);
    let instance_info_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(instance_info_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(instance_info_payload["state"], "Not started");
    assert!(instance_info_payload["app_name"].is_string());
    assert!(instance_info_payload["id"].is_string());
    assert!(instance_info_payload["vmm_version"].is_string());
}

#[tokio::test]
async fn version_endpoint_matches_pinned_preboot_surface_on_full_router() {
    let (router, _) = app_router();

    let version_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/version")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(version_response.status(), StatusCode::OK);
    let version_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(version_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(version_payload["firecracker_version"].is_string());
}

#[tokio::test]
async fn instance_info_reports_runtime_states_after_snapshot_loads_on_full_router() {
    let (paused_router, _) = app_router();

    let paused_snapshot_response = paused_router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/snapshot/load")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"snapshot_path":"state.snap","mem_backend":{"backend_type":"File","backend_path":"mem.snap"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(paused_snapshot_response.status(), StatusCode::NO_CONTENT);

    let paused_instance_response = paused_router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(paused_instance_response.status(), StatusCode::OK);
    let paused_instance_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(paused_instance_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(paused_instance_payload["state"], "Paused");

    let (running_router, _) = app_router();

    let running_snapshot_response = running_router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/snapshot/load")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"snapshot_path":"state.snap","mem_backend":{"backend_type":"File","backend_path":"mem.snap"},"resume_vm":true}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(running_snapshot_response.status(), StatusCode::NO_CONTENT);

    let running_instance_response = running_router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(running_instance_response.status(), StatusCode::OK);
    let running_instance_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(running_instance_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(running_instance_payload["state"], "Running");
}

#[tokio::test]
async fn version_endpoint_remains_stable_after_boot_on_full_router() {
    let (router, _) = app_router();

    let preboot_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/version")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(preboot_response.status(), StatusCode::OK);
    let preboot_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(preboot_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(preboot_payload["firecracker_version"].is_string());

    boot_instance(router.clone()).await;

    let runtime_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/version")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(runtime_response.status(), StatusCode::OK);
    let runtime_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(runtime_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(runtime_payload["firecracker_version"].is_string());
    assert_eq!(
        runtime_payload["firecracker_version"],
        preboot_payload["firecracker_version"]
    );
}

#[tokio::test]
async fn vm_config_balloon_export_remains_available_after_boot_on_full_router() {
    let (router, _) = app_router();

    let put_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/balloon")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"amount_mib":1,"deflate_on_oom":true}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_response.status(), StatusCode::NO_CONTENT);

    boot_instance(router.clone()).await;

    let vm_config_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/vm/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vm_config_response.status(), StatusCode::OK);
    let vm_config_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(vm_config_payload["balloon"]["amount_mib"], 1);
    assert_eq!(vm_config_payload["balloon"]["deflate_on_oom"], true);
    assert_eq!(vm_config_payload["balloon"]["stats_polling_interval_s"], 0);
    assert_eq!(vm_config_payload["balloon"]["free_page_hinting"], false);
    assert_eq!(vm_config_payload["balloon"]["free_page_reporting"], false);
}

#[tokio::test]
async fn actions_route_matches_pinned_instance_start_and_flush_metrics_guards() {
    let (router, firecracker) = app_router();

    let preboot_flush_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/actions")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"action_type":"FlushMetrics"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(preboot_flush_response.status(), StatusCode::BAD_REQUEST);
    let preboot_flush_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(preboot_flush_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(preboot_flush_payload["code"], "InvalidState");
    assert_eq!(
        preboot_flush_payload["fault_message"],
        "The requested operation is not supported before starting the microVM."
    );
    assert_eq!(
        preboot_flush_payload["baseline_firecracker_revision"],
        firecracker
    );

    let missing_kernel_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/actions")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"action_type":"InstanceStart"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing_kernel_response.status(), StatusCode::BAD_REQUEST);
    let missing_kernel_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(missing_kernel_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(missing_kernel_payload["code"], "InvalidRequest");
    assert_eq!(
        missing_kernel_payload["fault_message"],
        "Cannot start microvm without kernel configuration."
    );
    assert_eq!(
        missing_kernel_payload["baseline_firecracker_revision"],
        firecracker
    );

    let kernel = NamedTempFile::new().unwrap();
    let boot_source_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/boot-source")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"kernel_image_path":"{}"}}"#,
                    kernel.path().display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

    let first_start_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/actions")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"action_type":"InstanceStart"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(first_start_response.status(), StatusCode::NO_CONTENT);

    let second_start_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/actions")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"action_type":"InstanceStart"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(second_start_response.status(), StatusCode::BAD_REQUEST);
    let second_start_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(second_start_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(second_start_payload["code"], "InvalidState");
    assert_eq!(
        second_start_payload["fault_message"],
        "The requested operation is not supported after starting the microVM."
    );
    assert_eq!(
        second_start_payload["baseline_firecracker_revision"],
        firecracker
    );

    let postboot_flush_response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/actions")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"action_type":"FlushMetrics"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(postboot_flush_response.status(), StatusCode::NO_CONTENT);
}

#[cfg(target_arch = "x86_64")]
#[tokio::test]
async fn send_ctrl_alt_del_matches_pinned_x86_state_guards() {
    let (router, firecracker) = app_router();

    let preboot_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/actions")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"action_type":"SendCtrlAltDel"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(preboot_response.status(), StatusCode::BAD_REQUEST);
    let preboot_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(preboot_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(preboot_payload["code"], "InvalidState");
    assert_eq!(
        preboot_payload["fault_message"],
        "The requested operation is not supported before starting the microVM."
    );
    assert_eq!(
        preboot_payload["baseline_firecracker_revision"],
        firecracker
    );

    boot_instance(router.clone()).await;

    let postboot_response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/actions")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"action_type":"SendCtrlAltDel"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(postboot_response.status(), StatusCode::NO_CONTENT);
}

#[cfg(target_arch = "aarch64")]
#[tokio::test]
async fn send_ctrl_alt_del_is_rejected_on_aarch64_on_full_router() {
    let (router, firecracker) = app_router();

    let response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/actions")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"action_type":"SendCtrlAltDel"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["code"], "InvalidRequest");
    assert_eq!(
        payload["fault_message"],
        "SendCtrlAltDel does not supported on aarch64."
    );
    assert_eq!(payload["baseline_firecracker_revision"], firecracker);
}

#[tokio::test]
async fn vm_patch_matches_pinned_pause_resume_state_machine_on_full_router() {
    let (router, firecracker) = app_router();

    let preboot_pause_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/vm")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"state":"Paused"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(preboot_pause_response.status(), StatusCode::BAD_REQUEST);
    let preboot_pause_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(preboot_pause_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(preboot_pause_payload["code"], "InvalidState");
    assert_eq!(
        preboot_pause_payload["fault_message"],
        "The requested operation is not supported before starting the microVM."
    );
    assert_eq!(
        preboot_pause_payload["baseline_firecracker_revision"],
        firecracker
    );

    boot_instance(router.clone()).await;

    let pause_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/vm")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"state":"Paused"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(pause_response.status(), StatusCode::NO_CONTENT);

    let paused_state_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(paused_state_response.status(), StatusCode::OK);
    let paused_state_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(paused_state_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(paused_state_payload["state"], "Paused");

    let resume_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/vm")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"state":"Resumed"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resume_response.status(), StatusCode::NO_CONTENT);

    let resumed_state_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resumed_state_response.status(), StatusCode::OK);
    let resumed_state_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(resumed_state_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(resumed_state_payload["state"], "Running");
}

#[tokio::test]
async fn drive_stub_rejects_path_body_id_mismatch_on_full_router() {
    let (router, firecracker) = app_router();

    let response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/drives/rootfs")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"drive_id":"data","is_root_device":true,"path_on_host":"/tmp/rootfs.ext4"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["code"], "InvalidRequest");
    assert_eq!(
        payload["fault_message"],
        "The id from the path does not match the id from the body!"
    );
    assert_eq!(
        payload["baseline_firecracker_revision"].as_str().unwrap(),
        firecracker
    );
}

#[tokio::test]
async fn drive_patch_rejects_preboot_updates_on_full_router() {
    let (router, firecracker) = app_router();

    let response = router
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/drives/rootfs")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"drive_id":"rootfs"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["code"], "InvalidState");
    assert_eq!(
        payload["fault_message"],
        "The requested operation is not supported before starting the microVM."
    );
    assert_eq!(payload["baseline_firecracker_revision"], firecracker);
}

#[tokio::test]
async fn drive_patch_matches_pinned_runtime_behavior() {
    let (router, firecracker) = app_router();
    let root_backing_file = NamedTempFile::new().unwrap();

    let create_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/drives/rootfs")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"drive_id":"rootfs","is_root_device":true,"path_on_host":"{}"}}"#,
                    root_backing_file.path().display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::NO_CONTENT);

    boot_instance(router.clone()).await;

    let missing_drive_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/drives/data")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"drive_id":"data","path_on_host":"/tmp/data.ext4"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing_drive_response.status(), StatusCode::BAD_REQUEST);
    let missing_drive_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(missing_drive_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(missing_drive_payload["code"], "InvalidRequest");
    assert_eq!(
        missing_drive_payload["fault_message"],
        "Drive config error: Unable to patch the block device: Failed perform action on device: Device not found Please verify the request arguments."
    );
    assert_eq!(
        missing_drive_payload["baseline_firecracker_revision"]
            .as_str()
            .unwrap(),
        firecracker
    );

    let update_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/drives/rootfs")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"drive_id":"rootfs","path_on_host":"/tmp/rootfs-updated.ext4"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(update_response.status(), StatusCode::NO_CONTENT);

    let vm_config_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/vm/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vm_config_response.status(), StatusCode::OK);
    let vm_config_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        vm_config_payload["drives"][0]["path_on_host"],
        "/tmp/rootfs-updated.ext4"
    );
}

#[tokio::test]
async fn drive_patch_rejects_path_body_id_mismatch_after_boot() {
    let (router, firecracker) = app_router();
    let root_backing_file = NamedTempFile::new().unwrap();

    let create_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/drives/rootfs")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"drive_id":"rootfs","is_root_device":true,"path_on_host":"{}"}}"#,
                    root_backing_file.path().display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::NO_CONTENT);

    boot_instance(router.clone()).await;

    let mismatch_response = router
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/drives/rootfs")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"drive_id":"data","path_on_host":"/tmp/data.ext4"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(mismatch_response.status(), StatusCode::BAD_REQUEST);
    let mismatch_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(mismatch_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(mismatch_payload["code"], "InvalidRequest");
    assert_eq!(
        mismatch_payload["fault_message"],
        "The id from the path does not match the id from the body!"
    );
    assert_eq!(
        mismatch_payload["baseline_firecracker_revision"]
            .as_str()
            .unwrap(),
        firecracker
    );
}

#[tokio::test]
async fn drive_put_matches_pinned_root_device_behavior() {
    let (router, firecracker) = app_router();
    let pmem_root_backing_file = NamedTempFile::new().unwrap();
    pmem_root_backing_file.as_file().set_len(0x1000).unwrap();

    let pmem_root_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/pmem/pmem0")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"id":"pmem0","path_on_host":"{}","root_device":true}}"#,
                    pmem_root_backing_file.path().display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(pmem_root_response.status(), StatusCode::NO_CONTENT);

    let pmem_conflict_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/drives/rootfs")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"drive_id":"rootfs","is_root_device":true,"path_on_host":"/tmp/rootfs.ext4"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(pmem_conflict_response.status(), StatusCode::BAD_REQUEST);
    let pmem_conflict_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(pmem_conflict_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(pmem_conflict_payload["code"], "InvalidRequest");
    assert_eq!(
        pmem_conflict_payload["fault_message"],
        "Drive config error: Attempt to add block as a root device while the root device defined as a pmem device"
    );
    assert_eq!(
        pmem_conflict_payload["baseline_firecracker_revision"]
            .as_str()
            .unwrap(),
        firecracker
    );

    let (second_router, _) = app_router();
    let first_root_backing_file = NamedTempFile::new().unwrap();
    let replacement_root_backing_file = NamedTempFile::new().unwrap();
    let first_root_response = second_router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/drives/rootfs")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"drive_id":"rootfs","is_root_device":true,"path_on_host":"{}"}}"#,
                    first_root_backing_file.path().display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(first_root_response.status(), StatusCode::NO_CONTENT);

    let second_root_response = second_router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/drives/data")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"drive_id":"data","is_root_device":true,"path_on_host":"/tmp/data.ext4"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(second_root_response.status(), StatusCode::BAD_REQUEST);
    let second_root_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(second_root_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(second_root_payload["code"], "InvalidRequest");
    assert_eq!(
        second_root_payload["fault_message"],
        "Drive config error: A root block device already exists!"
    );

    let update_existing_root_response = second_router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/drives/rootfs")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"drive_id":"rootfs","is_root_device":true,"path_on_host":"{}"}}"#,
                    replacement_root_backing_file.path().display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        update_existing_root_response.status(),
        StatusCode::NO_CONTENT
    );

    let vm_config_response = second_router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/vm/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vm_config_response.status(), StatusCode::OK);
    let vm_config_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        vm_config_payload["drives"][0]["path_on_host"],
        replacement_root_backing_file.path().display().to_string()
    );
}

#[tokio::test]
async fn drive_put_rejects_missing_backing_files() {
    let (router, firecracker) = app_router();
    let missing_backing_file = "/definitely/missing-drive.ext4";

    let missing_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/drives/rootfs")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"drive_id":"rootfs","is_root_device":true,"path_on_host":"{}"}}"#,
                    missing_backing_file
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing_response.status(), StatusCode::BAD_REQUEST);
    let missing_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(missing_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(missing_payload["code"], "InvalidRequest");
    assert_eq!(
        missing_payload["fault_message"],
        format!(
            "Drive config error: Unable to create the virtio block device: Virtio backend error: Error manipulating the backing file: No such file or directory (os error 2) {}",
            missing_backing_file
        )
    );
    assert_eq!(
        missing_payload["baseline_firecracker_revision"]
            .as_str()
            .unwrap(),
        firecracker
    );

    let vm_config_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/vm/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vm_config_response.status(), StatusCode::OK);
    let vm_config_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(vm_config_payload["drives"].is_null());
}

#[tokio::test]
async fn drive_put_defaults_cache_type_to_unsafe() {
    let (router, _) = app_router();
    let root_backing_file = NamedTempFile::new().unwrap();

    let create_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/drives/rootfs")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"drive_id":"rootfs","is_root_device":true,"path_on_host":"{}"}}"#,
                    root_backing_file.path().display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::NO_CONTENT);

    let vm_config_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/vm/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vm_config_response.status(), StatusCode::OK);
    let vm_config_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(vm_config_payload["drives"][0]["cache_type"], "Unsafe");
}

#[tokio::test]
async fn drive_export_orders_root_device_first() {
    let (router, _) = app_router();
    let data_backing_file = NamedTempFile::new().unwrap();
    let root_backing_file = NamedTempFile::new().unwrap();

    let data_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/drives/data")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"drive_id":"data","is_root_device":false,"path_on_host":"{}"}}"#,
                    data_backing_file.path().display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(data_response.status(), StatusCode::NO_CONTENT);

    let root_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/drives/rootfs")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"drive_id":"rootfs","is_root_device":true,"path_on_host":"{}"}}"#,
                    root_backing_file.path().display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(root_response.status(), StatusCode::NO_CONTENT);

    let vm_config_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/vm/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vm_config_response.status(), StatusCode::OK);
    let vm_config_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(vm_config_payload["drives"][0]["drive_id"], "rootfs");
    assert_eq!(vm_config_payload["drives"][1]["drive_id"], "data");
}

#[tokio::test]
async fn network_put_matches_pinned_preboot_behavior() {
    let (router, _) = app_router();

    let create_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/network-interfaces/eth0")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"iface_id":"eth0","host_dev_name":"tap0","guest_mac":"06:00:ac:10:00:02"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::NO_CONTENT);

    let vm_config_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/vm/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vm_config_response.status(), StatusCode::OK);
    let vm_config_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        vm_config_payload["network-interfaces"][0]["iface_id"],
        "eth0"
    );
    assert_eq!(
        vm_config_payload["network-interfaces"][0]["host_dev_name"],
        "tap0"
    );
    assert_eq!(
        vm_config_payload["network-interfaces"][0]["guest_mac"],
        "06:00:ac:10:00:02"
    );
}

#[tokio::test]
async fn network_put_rejects_path_body_id_mismatch_on_full_router() {
    let (router, firecracker) = app_router();

    let mismatch_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/network-interfaces/eth0")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"iface_id":"eth1","host_dev_name":"tap0","guest_mac":"06:00:ac:10:00:02"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(mismatch_response.status(), StatusCode::BAD_REQUEST);
    let mismatch_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(mismatch_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(mismatch_payload["code"], "InvalidRequest");
    assert_eq!(
        mismatch_payload["fault_message"],
        "The id from the path [eth0] does not match the id from the body [eth1]!"
    );
    assert_eq!(
        mismatch_payload["baseline_firecracker_revision"]
            .as_str()
            .unwrap(),
        firecracker
    );

    let vm_config_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/vm/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vm_config_response.status(), StatusCode::OK);
    let vm_config_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(
        vm_config_payload["network-interfaces"].is_null()
            || vm_config_payload["network-interfaces"]
                .as_array()
                .map(|interfaces| interfaces.is_empty())
                .unwrap_or(false)
    );
}

#[tokio::test]
async fn network_put_rejects_pinned_preboot_conflicts() {
    let (router, firecracker) = app_router();

    let create_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/network-interfaces/eth0")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"iface_id":"eth0","host_dev_name":"tap0","guest_mac":"06:00:ac:10:00:02"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::NO_CONTENT);

    let duplicate_mac_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/network-interfaces/eth1")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"iface_id":"eth1","host_dev_name":"tap1","guest_mac":"06:00:ac:10:00:02"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(duplicate_mac_response.status(), StatusCode::BAD_REQUEST);
    let duplicate_mac_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(duplicate_mac_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(duplicate_mac_payload["code"], "InvalidRequest");
    assert_eq!(
        duplicate_mac_payload["fault_message"],
        "Network config error: The MAC address is already in use: 06:00:ac:10:00:02"
    );
    assert_eq!(
        duplicate_mac_payload["baseline_firecracker_revision"]
            .as_str()
            .unwrap(),
        firecracker
    );

    let duplicate_tap_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/network-interfaces/eth1")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"iface_id":"eth1","host_dev_name":"tap0","guest_mac":"06:00:ac:10:00:03"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(duplicate_tap_response.status(), StatusCode::BAD_REQUEST);
    let duplicate_tap_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(duplicate_tap_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(duplicate_tap_payload["code"], "InvalidRequest");
    assert_eq!(
        duplicate_tap_payload["fault_message"],
        "Network config error: Could not create the network device: Open tap device failed: Error while creating ifreq structure: Device or resource busy (os error 16). Invalid TUN/TAP Backend provided by tap0. Check our documentation on setting up the network devices."
    );
    assert_eq!(
        duplicate_tap_payload["baseline_firecracker_revision"]
            .as_str()
            .unwrap(),
        firecracker
    );

    let vm_config_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/vm/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vm_config_response.status(), StatusCode::OK);
    let vm_config_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        vm_config_payload["network-interfaces"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        vm_config_payload["network-interfaces"][0]["iface_id"],
        "eth0"
    );
}

#[tokio::test]
async fn network_patch_rejects_preboot_updates_on_full_router() {
    let (router, firecracker) = app_router();

    let response = router
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/network-interfaces/eth0")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"iface_id":"eth0","rx_rate_limiter":{"bandwidth":{"size":1024,"refill_time":1000}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["code"], "InvalidState");
    assert_eq!(
        payload["fault_message"],
        "The requested operation is not supported before starting the microVM."
    );
    assert_eq!(payload["baseline_firecracker_revision"], firecracker);
}

#[tokio::test]
async fn network_patch_matches_pinned_runtime_behavior() {
    let (router, firecracker) = app_router();

    let create_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/network-interfaces/eth0")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"iface_id":"eth0","host_dev_name":"tap0"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::NO_CONTENT);

    boot_instance(router.clone()).await;

    let mismatch_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/network-interfaces/eth0")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"iface_id":"eth1","rx_rate_limiter":{"bandwidth":{"size":1024,"refill_time":1000}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(mismatch_response.status(), StatusCode::BAD_REQUEST);
    let mismatch_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(mismatch_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(mismatch_payload["code"], "InvalidRequest");
    assert_eq!(
        mismatch_payload["fault_message"],
        "The id from the path [eth0] does not match the id from the body [eth1]!"
    );
    assert_eq!(
        mismatch_payload["baseline_firecracker_revision"]
            .as_str()
            .unwrap(),
        firecracker
    );

    let missing_interface_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/network-interfaces/eth9")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"iface_id":"eth9","rx_rate_limiter":{"bandwidth":{"size":1024,"refill_time":1000}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing_interface_response.status(), StatusCode::BAD_REQUEST);
    let missing_interface_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(missing_interface_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(missing_interface_payload["code"], "InvalidRequest");
    assert_eq!(
        missing_interface_payload["fault_message"],
        "Network config error: Unable to update the net device: Failed perform action on device: Device not found"
    );
    assert_eq!(
        missing_interface_payload["baseline_firecracker_revision"]
            .as_str()
            .unwrap(),
        firecracker
    );

    let update_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/network-interfaces/eth0")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"iface_id":"eth0","rx_rate_limiter":{"bandwidth":{"size":1024,"refill_time":1000}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(update_response.status(), StatusCode::NO_CONTENT);

    let vm_config_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/vm/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vm_config_response.status(), StatusCode::OK);
    let vm_config_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        vm_config_payload["network-interfaces"][0]["rx_rate_limiter"]["bandwidth"]["size"],
        1024
    );
}

#[tokio::test]
async fn snapshot_load_stub_matches_pinned_memory_source_parser_faults_on_full_router() {
    let (router, firecracker) = app_router();

    for (body, expected_message) in [
        (
            r#"{"snapshot_path":"state.snap"}"#,
            "An error occurred when deserializing the json body of a request: missing field: either `mem_backend` or `mem_file_path` is required.",
        ),
        (
            r#"{"snapshot_path":"state.snap","mem_file_path":"mem.snap","mem_backend":{"backend_type":"File","backend_path":"mem.snap"}}"#,
            "An error occurred when deserializing the json body of a request: too many fields: either `mem_backend` or `mem_file_path` exclusively is required.",
        ),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/snapshot/load")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let payload: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert_eq!(payload["code"], "InvalidRequest");
        assert_eq!(payload["fault_message"], expected_message);
        assert_eq!(
            payload["baseline_firecracker_revision"].as_str().unwrap(),
            firecracker
        );
    }
}

#[tokio::test]
async fn pmem_put_matches_pinned_root_device_behavior() {
    let (router, firecracker) = app_router();
    let root_backing_file = NamedTempFile::new().unwrap();
    root_backing_file.as_file().set_len(0x1000).unwrap();
    let replacement_backing_file = NamedTempFile::new().unwrap();
    replacement_backing_file.as_file().set_len(0x1000).unwrap();
    let drive_root_backing_file = NamedTempFile::new().unwrap();

    let mismatch_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/pmem/rootfs")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"id":"data","path_on_host":"/tmp/rootfs.img"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(mismatch_response.status(), StatusCode::BAD_REQUEST);
    let mismatch_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(mismatch_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(mismatch_payload["code"], "InvalidRequest");
    assert_eq!(
        mismatch_payload["fault_message"],
        "The id from the path does not match the id from the body!"
    );
    assert_eq!(
        mismatch_payload["baseline_firecracker_revision"]
            .as_str()
            .unwrap(),
        firecracker
    );

    let drive_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/drives/rootfs")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"drive_id":"rootfs","is_root_device":true,"path_on_host":"{}"}}"#,
                    drive_root_backing_file.path().display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(drive_response.status(), StatusCode::NO_CONTENT);

    let block_root_conflict_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/pmem/pmem0")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"id":"pmem0","path_on_host":"{}","root_device":true}}"#,
                    root_backing_file.path().display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        block_root_conflict_response.status(),
        StatusCode::BAD_REQUEST
    );
    let block_root_conflict_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(block_root_conflict_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(block_root_conflict_payload["code"], "InvalidRequest");
    assert_eq!(
        block_root_conflict_payload["fault_message"],
        "Pmem device error: Attempt to add pmem as a root device while the root device defined as a block device"
    );
    assert_eq!(
        block_root_conflict_payload["baseline_firecracker_revision"]
            .as_str()
            .unwrap(),
        firecracker
    );

    let (second_router, _) = app_router();
    let first_root_backing_file = NamedTempFile::new().unwrap();
    first_root_backing_file.as_file().set_len(0x1000).unwrap();
    let second_root_backing_file = NamedTempFile::new().unwrap();
    second_root_backing_file.as_file().set_len(0x1000).unwrap();
    let first_root_response = second_router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/pmem/pmem0")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"id":"pmem0","path_on_host":"{}","root_device":true}}"#,
                    first_root_backing_file.path().display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(first_root_response.status(), StatusCode::NO_CONTENT);

    let second_root_response = second_router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/pmem/pmem1")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"id":"pmem1","path_on_host":"{}","root_device":true}}"#,
                    second_root_backing_file.path().display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(second_root_response.status(), StatusCode::BAD_REQUEST);
    let second_root_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(second_root_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(second_root_payload["code"], "InvalidRequest");
    assert_eq!(
        second_root_payload["fault_message"],
        "Pmem device error: A root pmem device already exist"
    );

    let default_flags_response = second_router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/pmem/pmem0")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"id":"pmem0","path_on_host":"{}"}}"#,
                    replacement_backing_file.path().display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(default_flags_response.status(), StatusCode::NO_CONTENT);

    let vm_config_response = second_router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/vm/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vm_config_response.status(), StatusCode::OK);
    let vm_config_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(vm_config_payload["pmem"][0]["root_device"], false);
    assert_eq!(vm_config_payload["pmem"][0]["read_only"], false);
}

#[tokio::test]
async fn pmem_put_rejects_invalid_backing_files() {
    let (router, firecracker) = app_router();
    let missing_dir = tempdir().unwrap();
    let missing_path = missing_dir.path().join("missing.img");
    let zero_sized_backing_file = NamedTempFile::new().unwrap();

    let missing_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/pmem/pmem0")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"id":"pmem0","path_on_host":"{}"}}"#,
                    missing_path.display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing_response.status(), StatusCode::BAD_REQUEST);
    let missing_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(missing_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(missing_payload["code"], "InvalidRequest");
    assert_eq!(
        missing_payload["fault_message"],
        "Pmem device error: Unable to create the virtio-pmem device: Error accessing backing file: No such file or directory (os error 2)"
    );
    assert_eq!(
        missing_payload["baseline_firecracker_revision"]
            .as_str()
            .unwrap(),
        firecracker
    );

    let zero_sized_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/pmem/pmem0")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"id":"pmem0","path_on_host":"{}"}}"#,
                    zero_sized_backing_file.path().display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(zero_sized_response.status(), StatusCode::BAD_REQUEST);
    let zero_sized_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(zero_sized_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(zero_sized_payload["code"], "InvalidRequest");
    assert_eq!(
        zero_sized_payload["fault_message"],
        "Pmem device error: Unable to create the virtio-pmem device: Error backing file size is 0"
    );
    assert_eq!(
        zero_sized_payload["baseline_firecracker_revision"]
            .as_str()
            .unwrap(),
        firecracker
    );

    let vm_config_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/vm/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vm_config_response.status(), StatusCode::OK);
    let vm_config_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(vm_config_payload["pmem"].is_null());
}

#[tokio::test]
async fn vm_config_pmem_export_remains_available_after_boot_on_full_router() {
    let (router, _) = app_router();
    let backing_file = NamedTempFile::new().unwrap();
    backing_file.as_file().set_len(0x1000).unwrap();

    let pmem_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/pmem/pmem0")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"id":"pmem0","path_on_host":"{}"}}"#,
                    backing_file.path().display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(pmem_response.status(), StatusCode::NO_CONTENT);

    boot_instance(router.clone()).await;

    let vm_config_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/vm/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vm_config_response.status(), StatusCode::OK);
    let vm_config_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(vm_config_payload["pmem"][0]["root_device"], false);
    assert_eq!(vm_config_payload["pmem"][0]["read_only"], false);
}

#[tokio::test]
async fn memory_hotplug_get_returns_initial_runtime_status_on_full_router() {
    let (router, _) = app_router();

    let put_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/hotplug/memory")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"total_size_mib":1024}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_response.status(), StatusCode::NO_CONTENT);

    let vm_config_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/vm/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vm_config_response.status(), StatusCode::OK);
    let vm_config_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(vm_config_payload["memory-hotplug"]["block_size_mib"], 2);
    assert_eq!(vm_config_payload["memory-hotplug"]["slot_size_mib"], 128);
    assert_eq!(vm_config_payload["memory-hotplug"]["total_size_mib"], 1024);

    boot_instance(router.clone()).await;

    let get_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/hotplug/memory")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get_response.status(), StatusCode::OK);
    let get_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(get_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(get_payload["block_size_mib"], 2);
    assert_eq!(get_payload["slot_size_mib"], 128);
    assert_eq!(get_payload["total_size_mib"], 1024);
    assert_eq!(get_payload["plugged_size_mib"], 0);
    assert_eq!(get_payload["requested_size_mib"], 0);
}

#[tokio::test]
async fn memory_hotplug_get_returns_updated_requested_size_after_patch_on_full_router() {
    let (router, _) = app_router();

    let put_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/hotplug/memory")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"total_size_mib":1024}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_response.status(), StatusCode::NO_CONTENT);

    boot_instance(router.clone()).await;

    let patch_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/hotplug/memory")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"requested_size_mib":512}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(patch_response.status(), StatusCode::NO_CONTENT);

    let updated_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/hotplug/memory")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(updated_response.status(), StatusCode::OK);
    let updated_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(updated_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(updated_payload["block_size_mib"], 2);
    assert_eq!(updated_payload["slot_size_mib"], 128);
    assert_eq!(updated_payload["total_size_mib"], 1024);
    assert_eq!(updated_payload["plugged_size_mib"], 0);
    assert_eq!(updated_payload["requested_size_mib"], 512);
}

#[tokio::test]
async fn vm_config_memory_hotplug_export_remains_available_after_boot_on_full_router() {
    let (router, _) = app_router();

    let put_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/hotplug/memory")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"total_size_mib":1024}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_response.status(), StatusCode::NO_CONTENT);

    boot_instance(router.clone()).await;

    let vm_config_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/vm/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vm_config_response.status(), StatusCode::OK);
    let vm_config_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(vm_config_payload["memory-hotplug"]["block_size_mib"], 2);
    assert_eq!(vm_config_payload["memory-hotplug"]["slot_size_mib"], 128);
    assert_eq!(vm_config_payload["memory-hotplug"]["total_size_mib"], 1024);
}

#[tokio::test]
async fn vm_config_mmds_config_export_remains_available_after_boot_on_full_router() {
    let (router, _) = app_router();

    let put_network_interface_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/network-interfaces/eth0")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"iface_id":"eth0","host_dev_name":"tap0"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        put_network_interface_response.status(),
        StatusCode::NO_CONTENT
    );

    let put_config_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/mmds/config")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"network_interfaces":["eth0"]}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_config_response.status(), StatusCode::NO_CONTENT);

    boot_instance(router.clone()).await;

    let vm_config_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/vm/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vm_config_response.status(), StatusCode::OK);
    let vm_config_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        vm_config_payload["mmds-config"]["network_interfaces"][0],
        "eth0"
    );
    assert_eq!(vm_config_payload["mmds-config"]["version"], "V1");
    assert_eq!(
        vm_config_payload["mmds-config"]["ipv4_address"],
        "169.254.169.254"
    );
    assert_eq!(vm_config_payload["mmds-config"]["imds_compat"], false);
}

#[tokio::test]
async fn memory_hotplug_get_rejects_preboot_reads_on_full_router() {
    let (router, firecracker) = app_router();

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/hotplug/memory")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["code"], "InvalidState");
    assert_eq!(
        payload["fault_message"],
        "The requested operation is not supported before starting the microVM."
    );
    assert_eq!(payload["baseline_firecracker_revision"], firecracker);
}

#[tokio::test]
async fn memory_hotplug_patch_rejects_preboot_updates_on_full_router() {
    let (router, firecracker) = app_router();

    let response = router
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/hotplug/memory")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"requested_size_mib":512}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["code"], "InvalidState");
    assert_eq!(
        payload["fault_message"],
        "The requested operation is not supported before starting the microVM."
    );
    assert_eq!(payload["baseline_firecracker_revision"], firecracker);
}

#[tokio::test]
async fn memory_hotplug_put_rejects_postboot_updates_on_full_router() {
    let (router, firecracker) = app_router();
    let firecracker = firecracker.to_string();

    boot_instance(router.clone()).await;

    let response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/hotplug/memory")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"total_size_mib":1024}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["code"], "InvalidState");
    assert_eq!(
        payload["fault_message"],
        "The requested operation is not supported after starting the microVM."
    );
    assert_eq!(
        payload["baseline_firecracker_revision"].as_str().unwrap(),
        firecracker
    );
}

#[tokio::test]
async fn memory_hotplug_put_rejects_invalid_total_size_preboot_on_full_router() {
    let (router, firecracker) = app_router();

    let response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/hotplug/memory")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"total_size_mib":130}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["code"], "InvalidRequest");
    assert_eq!(
        payload["fault_message"],
        "The total size of the hotpluggable memory device must be at least the slot size and a multiple of the slot size."
    );
    assert_eq!(
        payload["baseline_firecracker_revision"].as_str().unwrap(),
        firecracker
    );
}

#[tokio::test]
async fn memory_hotplug_put_rejects_invalid_block_size_preboot_on_full_router() {
    let (router, firecracker) = app_router();

    let response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/hotplug/memory")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"block_size_mib":3,"total_size_mib":1024}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["code"], "InvalidRequest");
    assert_eq!(
        payload["fault_message"],
        "The block size of the hotpluggable memory device must be at least 2 MiB and a power of 2."
    );
    assert_eq!(
        payload["baseline_firecracker_revision"].as_str().unwrap(),
        firecracker
    );
}

#[tokio::test]
async fn memory_hotplug_patch_rejects_invalid_requested_size_on_full_router() {
    let (router, firecracker) = app_router();

    let put_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/hotplug/memory")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"total_size_mib":1024}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_response.status(), StatusCode::NO_CONTENT);

    boot_instance(router.clone()).await;

    let response = router
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/hotplug/memory")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"requested_size_mib":513}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["code"], "InvalidRequest");
    assert_eq!(
        payload["fault_message"],
        "The requested size of the hotpluggable memory device must be a multiple of the block size and no larger than the total size."
    );
    assert_eq!(
        payload["baseline_firecracker_revision"].as_str().unwrap(),
        firecracker
    );
}

#[tokio::test]
async fn mmds_flow_matches_pinned_baseline() {
    let (router, _) = app_router();

    let initial_get_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/mmds")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(initial_get_response.status(), StatusCode::OK);
    let initial_get_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(initial_get_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(initial_get_payload, serde_json::json!({}));

    let put_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/mmds")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"meta":{"name":"vesper"},"remove":"later"}"#))
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
                .uri("/mmds")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"meta":{"version":"1"},"remove":null}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(patch_response.status(), StatusCode::NO_CONTENT);

    let put_network_interface_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/network-interfaces/eth0")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"iface_id":"eth0","host_dev_name":"tap0"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        put_network_interface_response.status(),
        StatusCode::NO_CONTENT
    );

    let put_config_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/mmds/config")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"network_interfaces":["eth0"]}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_config_response.status(), StatusCode::NO_CONTENT);

    let vm_config_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/vm/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vm_config_response.status(), StatusCode::OK);
    let vm_config_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        vm_config_payload["mmds-config"]["network_interfaces"][0],
        "eth0"
    );
    assert_eq!(vm_config_payload["mmds-config"]["version"], "V1");
    assert_eq!(
        vm_config_payload["mmds-config"]["ipv4_address"],
        "169.254.169.254"
    );
    assert_eq!(vm_config_payload["mmds-config"]["imds_compat"], false);

    boot_instance(router.clone()).await;

    let runtime_get_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/mmds")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(runtime_get_response.status(), StatusCode::OK);
    let runtime_get_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(runtime_get_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(runtime_get_payload["meta"]["name"], "vesper");
    assert_eq!(runtime_get_payload["meta"]["version"], "1");
    assert!(runtime_get_payload.get("remove").is_none());

    let runtime_patch_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/mmds")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"meta":{"build":"2"}}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(runtime_patch_response.status(), StatusCode::NO_CONTENT);

    let updated_get_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/mmds")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(updated_get_response.status(), StatusCode::OK);
    let updated_get_payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(updated_get_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(updated_get_payload["meta"]["name"], "vesper");
    assert_eq!(updated_get_payload["meta"]["version"], "1");
    assert_eq!(updated_get_payload["meta"]["build"], "2");
}

#[tokio::test]
async fn mmds_get_returns_preboot_empty_store_on_full_router() {
    let (router, _) = app_router();

    let get_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/mmds")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get_response.status(), StatusCode::OK);

    let payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(get_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(payload, serde_json::json!({}));
}

#[tokio::test]
async fn mmds_get_returns_preboot_initialized_store_on_full_router() {
    let (router, _) = app_router();

    let put_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/mmds")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"meta":{"name":"vesper"},"remove":"later"}"#))
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
                .uri("/mmds")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"meta":{"version":"1"},"remove":null}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(patch_response.status(), StatusCode::NO_CONTENT);

    let get_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/mmds")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get_response.status(), StatusCode::OK);

    let payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(get_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(payload["meta"]["name"], "vesper");
    assert_eq!(payload["meta"]["version"], "1");
    assert!(payload.get("remove").is_none());
}

#[tokio::test]
async fn mmds_get_returns_runtime_configured_store_on_full_router() {
    let (router, _) = app_router();

    let put_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/mmds")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"meta":{"name":"vesper"},"remove":"later"}"#))
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
                .uri("/mmds")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"meta":{"version":"1"},"remove":null}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(patch_response.status(), StatusCode::NO_CONTENT);

    let put_network_interface_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/network-interfaces/eth0")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"iface_id":"eth0","host_dev_name":"tap0"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        put_network_interface_response.status(),
        StatusCode::NO_CONTENT
    );

    let put_config_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/mmds/config")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"network_interfaces":["eth0"]}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_config_response.status(), StatusCode::NO_CONTENT);

    boot_instance(router.clone()).await;

    let runtime_get_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/mmds")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(runtime_get_response.status(), StatusCode::OK);

    let payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(runtime_get_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(payload["meta"]["name"], "vesper");
    assert_eq!(payload["meta"]["version"], "1");
    assert!(payload.get("remove").is_none());
}

#[tokio::test]
async fn mmds_runtime_requires_config_with_baseline_revision() {
    let (router, firecracker) = app_router();

    let put_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/mmds")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"meta":{"name":"vesper"}}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_response.status(), StatusCode::NO_CONTENT);

    boot_instance(router.clone()).await;

    let get_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/mmds")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get_response.status(), StatusCode::BAD_REQUEST);
    let payload: serde_json::Value = serde_json::from_slice(
        &to_bytes(get_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(payload["code"], "InvalidRequest");
    assert_eq!(
        payload["fault_message"],
        "The MMDS data store is not initialized."
    );
    assert_eq!(
        payload["baseline_firecracker_revision"].as_str().unwrap(),
        firecracker
    );
}

#[tokio::test]
async fn mmds_put_rejects_oversized_store_on_full_router() {
    let (router, firecracker) = app_router();
    let oversized_payload = format!(r#"{{"blob":"{}"}}"#, "a".repeat(51_200));

    let response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/mmds")
                .header("content-type", "application/json")
                .body(Body::from(oversized_payload))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);

    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["code"], "PayloadTooLarge");
    assert_eq!(
        payload["fault_message"],
        "The MMDS patch request doesn't fit."
    );
    assert_eq!(
        payload["baseline_firecracker_revision"].as_str().unwrap(),
        firecracker
    );
}

#[tokio::test]
async fn mmds_put_requires_runtime_config_on_full_router() {
    let (router, firecracker) = app_router();

    let put_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/mmds")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"meta":{"name":"vesper"}}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_response.status(), StatusCode::NO_CONTENT);

    boot_instance(router.clone()).await;

    let response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/mmds")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"meta":{"build":"2"}}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["code"], "InvalidRequest");
    assert_eq!(
        payload["fault_message"],
        "The MMDS data store is not initialized."
    );
    assert_eq!(
        payload["baseline_firecracker_revision"].as_str().unwrap(),
        firecracker
    );
}

#[tokio::test]
async fn mmds_patch_requires_initialized_store_on_full_router() {
    let (router, firecracker) = app_router();

    let response = router
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/mmds")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"meta":{"name":"vesper"}}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["code"], "InvalidRequest");
    assert_eq!(
        payload["fault_message"],
        "The MMDS data store is not initialized."
    );
    assert_eq!(
        payload["baseline_firecracker_revision"].as_str().unwrap(),
        firecracker
    );
}

#[tokio::test]
async fn mmds_patch_requires_runtime_config_on_full_router() {
    let (router, firecracker) = app_router();

    let put_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/mmds")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"meta":{"name":"vesper"}}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_response.status(), StatusCode::NO_CONTENT);

    boot_instance(router.clone()).await;

    let response = router
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/mmds")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"meta":{"build":"2"}}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["code"], "InvalidRequest");
    assert_eq!(
        payload["fault_message"],
        "The MMDS data store is not initialized."
    );
    assert_eq!(
        payload["baseline_firecracker_revision"].as_str().unwrap(),
        firecracker
    );
}

#[tokio::test]
async fn mmds_patch_rejects_oversized_merge_on_full_router() {
    let (router, firecracker) = app_router();
    let oversized_patch = format!(r#"{{"blob":"{}"}}"#, "a".repeat(51_200));

    let put_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/mmds")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"meta":{"name":"vesper"}}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_response.status(), StatusCode::NO_CONTENT);

    let response = router
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/mmds")
                .header("content-type", "application/json")
                .body(Body::from(oversized_patch))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);

    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["code"], "PayloadTooLarge");
    assert_eq!(
        payload["fault_message"],
        "The MMDS patch request doesn't fit."
    );
    assert_eq!(
        payload["baseline_firecracker_revision"].as_str().unwrap(),
        firecracker
    );
}

#[tokio::test]
async fn mmds_config_put_rejects_empty_network_interface_list_on_full_router() {
    let (router, firecracker) = app_router();

    let response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/mmds/config")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"network_interfaces":[]}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["code"], "InvalidRequest");
    assert_eq!(
        payload["fault_message"],
        "The list of network interface IDs that allow forwarding MMDS requests is empty."
    );
    assert_eq!(
        payload["baseline_firecracker_revision"].as_str().unwrap(),
        firecracker
    );
}

#[tokio::test]
async fn mmds_config_put_rejects_unknown_network_interface_on_full_router() {
    let (router, firecracker) = app_router();

    let put_network_interface_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/network-interfaces/eth0")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"iface_id":"eth0","host_dev_name":"tap0"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        put_network_interface_response.status(),
        StatusCode::NO_CONTENT
    );

    let response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/mmds/config")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"network_interfaces":["missing"]}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["code"], "InvalidRequest");
    assert_eq!(
        payload["fault_message"],
        "The list of network interface IDs provided contains at least one ID that does not correspond to any existing network interface."
    );
    assert_eq!(
        payload["baseline_firecracker_revision"].as_str().unwrap(),
        firecracker
    );
}

#[tokio::test]
async fn mmds_config_put_rejects_non_link_local_ipv4_on_full_router() {
    let (router, firecracker) = app_router();

    let put_network_interface_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/network-interfaces/eth0")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"iface_id":"eth0","host_dev_name":"tap0"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        put_network_interface_response.status(),
        StatusCode::NO_CONTENT
    );

    let response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/mmds/config")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"network_interfaces":["eth0"],"ipv4_address":"10.0.0.1"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["code"], "InvalidRequest");
    assert_eq!(
        payload["fault_message"],
        "The MMDS IPv4 address is not link local."
    );
    assert_eq!(
        payload["baseline_firecracker_revision"].as_str().unwrap(),
        firecracker
    );
}

#[tokio::test]
async fn mmds_config_put_rejects_runtime_updates_on_full_router() {
    let (router, firecracker) = app_router();

    let put_network_interface_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/network-interfaces/eth0")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"iface_id":"eth0","host_dev_name":"tap0"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        put_network_interface_response.status(),
        StatusCode::NO_CONTENT
    );

    boot_instance(router.clone()).await;

    let response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/mmds/config")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"network_interfaces":["eth0"]}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["code"], "InvalidState");
    assert_eq!(
        payload["fault_message"],
        "The requested operation is not supported after starting the microVM."
    );
    assert_eq!(
        payload["baseline_firecracker_revision"].as_str().unwrap(),
        firecracker
    );
}
