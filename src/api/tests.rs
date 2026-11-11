
use super::pmem::{
    PMEM_BACKING_FILE_ZERO_SIZE_MESSAGE, PMEM_ID_PATH_MISMATCH_MESSAGE,
    PMEM_ROOT_ALREADY_EXISTS_MESSAGE, PMEM_ROOT_BLOCK_CONFLICT_MESSAGE,
    PMEM_UPDATE_NOT_FOUND_MESSAGE,
};
use super::*;
use axum::body::to_bytes;
use axum::http::Request;
use tempfile::NamedTempFile;
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
                method: "PUT".to_string(),
                path: "/balloon".to_string(),
            },
            RouteSpec {
                method: "PATCH".to_string(),
                path: "/balloon".to_string(),
            },
            RouteSpec {
                method: "GET".to_string(),
                path: "/balloon/statistics".to_string(),
            },
            RouteSpec {
                method: "PATCH".to_string(),
                path: "/balloon/statistics".to_string(),
            },
            RouteSpec {
                method: "GET".to_string(),
                path: "/balloon/hinting/status".to_string(),
            },
            RouteSpec {
                method: "PATCH".to_string(),
                path: "/balloon/hinting/start".to_string(),
            },
            RouteSpec {
                method: "PATCH".to_string(),
                path: "/balloon/hinting/stop".to_string(),
            },
            RouteSpec {
                method: "GET".to_string(),
                path: "/".to_string(),
            },
            RouteSpec {
                method: "GET".to_string(),
                path: "/balloon".to_string(),
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
                path: "/cpu-config".to_string(),
            },
            RouteSpec {
                method: "PUT".to_string(),
                path: "/logger".to_string(),
            },
            RouteSpec {
                method: "PUT".to_string(),
                path: "/machine-config".to_string(),
            },
            RouteSpec {
                method: "PUT".to_string(),
                path: "/metrics".to_string(),
            },
            RouteSpec {
                method: "GET".to_string(),
                path: "/mmds".to_string(),
            },
            RouteSpec {
                method: "PATCH".to_string(),
                path: "/mmds".to_string(),
            },
            RouteSpec {
                method: "PUT".to_string(),
                path: "/mmds".to_string(),
            },
            RouteSpec {
                method: "PUT".to_string(),
                path: "/mmds/config".to_string(),
            },
            RouteSpec {
                method: "PUT".to_string(),
                path: "/serial".to_string(),
            },
            RouteSpec {
                method: "PUT".to_string(),
                path: "/entropy".to_string(),
            },
            RouteSpec {
                method: "PUT".to_string(),
                path: "/vsock".to_string(),
            },
            RouteSpec {
                method: "PATCH".to_string(),
                path: "/machine-config".to_string(),
            },
            RouteSpec {
                method: "PUT".to_string(),
                path: "/drives/{drive_id}".to_string(),
            },
            RouteSpec {
                method: "PATCH".to_string(),
                path: "/drives/{drive_id}".to_string(),
            },
            RouteSpec {
                method: "PUT".to_string(),
                path: "/network-interfaces/{iface_id}".to_string(),
            },
            RouteSpec {
                method: "PATCH".to_string(),
                path: "/network-interfaces/{iface_id}".to_string(),
            },
            RouteSpec {
                method: "GET".to_string(),
                path: "/hotplug/memory".to_string(),
            },
            RouteSpec {
                method: "PATCH".to_string(),
                path: "/hotplug/memory".to_string(),
            },
            RouteSpec {
                method: "PUT".to_string(),
                path: "/hotplug/memory".to_string(),
            },
            RouteSpec {
                method: "PUT".to_string(),
                path: "/pmem/{id}".to_string(),
            },
            RouteSpec {
                method: "PATCH".to_string(),
                path: "/pmem/{id}".to_string(),
            },
            RouteSpec {
                method: "PUT".to_string(),
                path: "/snapshot/create".to_string(),
            },
            RouteSpec {
                method: "PUT".to_string(),
                path: "/snapshot/load".to_string(),
            },
            RouteSpec {
                method: "GET".to_string(),
                path: "/vm/config".to_string(),
            },
            RouteSpec {
                method: "PATCH".to_string(),
                path: "/vm".to_string(),
            },
        ],
        "abc123".to_string(),
        "1.16.0-dev".to_string(),
    )
    .unwrap()
}

fn put_boot_source_request(
    kernel_path: &str,
    initrd_path: Option<&str>,
) -> Request<axum::body::Body> {
    let body = match initrd_path {
        Some(initrd_path) => {
            format!(r#"{{"kernel_image_path":"{kernel_path}","initrd_path":"{initrd_path}"}}"#)
        }
        None => format!(r#"{{"kernel_image_path":"{kernel_path}"}}"#),
    };

    Request::builder()
        .method("PUT")
        .uri("/boot-source")
        .header("content-type", "application/json")
        .body(axum::body::Body::from(body))
        .unwrap()
}

#[tokio::test]
async fn transitions_to_running_on_instance_start() {
    let router = test_state().router();
    let kernel = NamedTempFile::new().unwrap();
    let initrd = NamedTempFile::new().unwrap();

    let boot_source_response = router
        .clone()
        .oneshot(put_boot_source_request(
            kernel.path().to_str().unwrap(),
            Some(initrd.path().to_str().unwrap()),
        ))
        .await
        .unwrap();
    assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

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
    let kernel = NamedTempFile::new().unwrap();

    let boot_source_response = router
        .clone()
        .oneshot(put_boot_source_request(
            kernel.path().to_str().unwrap(),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

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
    assert_eq!(payload.fault_message, ACTION_NOT_SUPPORTED_POSTBOOT_MESSAGE);
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
    let kernel = NamedTempFile::new().unwrap();

    let boot_source_response = router
        .clone()
        .oneshot(put_boot_source_request(
            kernel.path().to_str().unwrap(),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

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
    assert_eq!(
        vm_config_payload["boot-source"]["kernel_image_path"],
        kernel.path().to_str().unwrap()
    );
    assert_eq!(vm_config_payload["machine-config"]["mem_size_mib"], 512);
    assert_eq!(vm_config_payload["machine-config"]["vcpu_count"], 2);
}

#[tokio::test]
async fn stores_balloon_config_and_exports_vm_config() {
    let router = test_state().router();

    let initial_get_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/balloon")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(initial_get_response.status(), StatusCode::BAD_REQUEST);
    let initial_get_body = to_bytes(initial_get_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let initial_get_payload: ErrorEnvelope = serde_json::from_slice(&initial_get_body).unwrap();
    assert_eq!(initial_get_payload.code, "InvalidRequest");
    assert_eq!(
        initial_get_payload.fault_message,
        BALLOON_NOT_CONFIGURED_MESSAGE
    );

    let put_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/balloon")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"amount_mib":1,"deflate_on_oom":true}"#,
                ))
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
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get_response.status(), StatusCode::OK);
    let get_body = to_bytes(get_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let get_payload: serde_json::Value = serde_json::from_slice(&get_body).unwrap();
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
    assert_eq!(vm_config_payload["balloon"]["amount_mib"], 1);
    assert_eq!(vm_config_payload["balloon"]["deflate_on_oom"], true);
    assert_eq!(vm_config_payload["balloon"]["stats_polling_interval_s"], 0);
    assert_eq!(vm_config_payload["balloon"]["free_page_hinting"], false);
    assert_eq!(vm_config_payload["balloon"]["free_page_reporting"], false);
}

#[tokio::test]
async fn rejects_balloon_config_larger_than_guest_memory() {
    let router = test_state().router();

    let response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/balloon")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"amount_mib":129,"deflate_on_oom":true}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload.code, "InvalidRequest");
    assert_eq!(
        payload.fault_message,
        BALLOON_TOO_MANY_PAGES_REQUESTED_MESSAGE
    );
}

#[tokio::test]
async fn rejects_machine_config_smaller_than_balloon_target() {
    let router = test_state().router();

    let put_balloon_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/balloon")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"amount_mib":64,"deflate_on_oom":true}"#,
                ))
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
            .body(axum::body::Body::from(r#"{"mem_size_mib":32}"#))
            .unwrap(),
        Request::builder()
            .method("PUT")
            .uri("/machine-config")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                r#"{"vcpu_count":1,"mem_size_mib":32}"#,
            ))
            .unwrap(),
    ] {
        let response = router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload.code, "InvalidRequest");
        assert_eq!(
            payload.fault_message,
            MACHINE_CONFIG_INCOMPATIBLE_BALLOON_SIZE_MESSAGE
        );
    }
}

#[tokio::test]
async fn runtime_balloon_read_and_update_require_configured_device() {
    let router = test_state().router();
    let kernel = NamedTempFile::new().unwrap();

    let boot_source_response = router
        .clone()
        .oneshot(put_boot_source_request(
            kernel.path().to_str().unwrap(),
            None,
        ))
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
                .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

    let get_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/balloon")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get_response.status(), StatusCode::BAD_REQUEST);
    let get_body = to_bytes(get_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let get_payload: ErrorEnvelope = serde_json::from_slice(&get_body).unwrap();
    assert_eq!(get_payload.code, "InvalidRequest");
    assert_eq!(
        get_payload.fault_message,
        BALLOON_RUNTIME_NOT_CONFIGURED_MESSAGE
    );

    let patch_response = router
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/balloon")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(r#"{"amount_mib":1}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(patch_response.status(), StatusCode::BAD_REQUEST);
    let patch_body = to_bytes(patch_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let patch_payload: ErrorEnvelope = serde_json::from_slice(&patch_body).unwrap();
    assert_eq!(patch_payload.code, "InvalidRequest");
    assert_eq!(
        patch_payload.fault_message,
        BALLOON_UPDATE_NOT_CONFIGURED_MESSAGE
    );
}

#[tokio::test]
async fn updates_balloon_target_after_boot_and_rejects_oversized_runtime_target() {
    let router = test_state().router();
    let kernel = NamedTempFile::new().unwrap();

    let put_balloon_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/balloon")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"amount_mib":1,"deflate_on_oom":true}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_balloon_response.status(), StatusCode::NO_CONTENT);

    let boot_source_response = router
        .clone()
        .oneshot(put_boot_source_request(
            kernel.path().to_str().unwrap(),
            None,
        ))
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
                .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

    let patch_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/balloon")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(r#"{"amount_mib":64}"#))
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
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get_response.status(), StatusCode::OK);
    let get_body = to_bytes(get_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let get_payload: serde_json::Value = serde_json::from_slice(&get_body).unwrap();
    assert_eq!(get_payload["amount_mib"], 64);
    assert_eq!(get_payload["deflate_on_oom"], true);

    let oversized_response = router
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/balloon")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(r#"{"amount_mib":129}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(oversized_response.status(), StatusCode::BAD_REQUEST);
    let oversized_body = to_bytes(oversized_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let oversized_payload: ErrorEnvelope = serde_json::from_slice(&oversized_body).unwrap();
    assert_eq!(oversized_payload.code, "InvalidRequest");
    assert_eq!(
        oversized_payload.fault_message,
        balloon_update_too_large_message(DEFAULT_MEM_SIZE_MIB)
    );
}

#[tokio::test]
async fn runtime_balloon_stats_require_configured_device_and_enabled_stats() {
    let router = test_state().router();
    let kernel = NamedTempFile::new().unwrap();

    let boot_source_response = router
        .clone()
        .oneshot(put_boot_source_request(
            kernel.path().to_str().unwrap(),
            None,
        ))
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
                .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

    let missing_get_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/balloon/statistics")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing_get_response.status(), StatusCode::BAD_REQUEST);
    let missing_get_body = to_bytes(missing_get_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let missing_get_payload: ErrorEnvelope = serde_json::from_slice(&missing_get_body).unwrap();
    assert_eq!(missing_get_payload.code, "InvalidRequest");
    assert_eq!(
        missing_get_payload.fault_message,
        BALLOON_RUNTIME_NOT_CONFIGURED_MESSAGE
    );

    let missing_patch_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/balloon/statistics")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(r#"{"stats_polling_interval_s":1}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing_patch_response.status(), StatusCode::BAD_REQUEST);
    let missing_patch_body = to_bytes(missing_patch_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let missing_patch_payload: ErrorEnvelope = serde_json::from_slice(&missing_patch_body).unwrap();
    assert_eq!(missing_patch_payload.code, "InvalidRequest");
    assert_eq!(
        missing_patch_payload.fault_message,
        BALLOON_UPDATE_NOT_CONFIGURED_MESSAGE
    );

    let router = test_state().router();
    let kernel = NamedTempFile::new().unwrap();

    let put_balloon_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/balloon")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"amount_mib":1,"deflate_on_oom":true}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_balloon_response.status(), StatusCode::NO_CONTENT);

    let boot_source_response = router
        .clone()
        .oneshot(put_boot_source_request(
            kernel.path().to_str().unwrap(),
            None,
        ))
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
                .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

    let disabled_get_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/balloon/statistics")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(disabled_get_response.status(), StatusCode::BAD_REQUEST);
    let disabled_get_body = to_bytes(disabled_get_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let disabled_get_payload: ErrorEnvelope = serde_json::from_slice(&disabled_get_body).unwrap();
    assert_eq!(disabled_get_payload.code, "InvalidRequest");
    assert_eq!(
        disabled_get_payload.fault_message,
        BALLOON_STATS_DISABLED_MESSAGE
    );

    let disabled_patch_response = router
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/balloon/statistics")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(r#"{"stats_polling_interval_s":1}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(disabled_patch_response.status(), StatusCode::BAD_REQUEST);
    let disabled_patch_body = to_bytes(disabled_patch_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let disabled_patch_payload: ErrorEnvelope =
        serde_json::from_slice(&disabled_patch_body).unwrap();
    assert_eq!(disabled_patch_payload.code, "InvalidRequest");
    assert_eq!(
        disabled_patch_payload.fault_message,
        BALLOON_STATS_STATE_CHANGE_MESSAGE
    );
}

#[tokio::test]
async fn runtime_balloon_stats_return_required_fields_and_allow_nonzero_interval_updates() {
    let router = test_state().router();
    let kernel = NamedTempFile::new().unwrap();

    let put_balloon_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/balloon")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"amount_mib":64,"deflate_on_oom":true,"stats_polling_interval_s":1}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_balloon_response.status(), StatusCode::NO_CONTENT);

    let boot_source_response = router
        .clone()
        .oneshot(put_boot_source_request(
            kernel.path().to_str().unwrap(),
            None,
        ))
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
                .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

    let get_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/balloon/statistics")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get_response.status(), StatusCode::OK);
    let get_body = to_bytes(get_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let get_payload: serde_json::Value = serde_json::from_slice(&get_body).unwrap();
    assert_eq!(get_payload["target_mib"], 64);
    assert_eq!(get_payload["actual_mib"], 64);
    assert_eq!(get_payload["target_pages"], 16384);
    assert_eq!(get_payload["actual_pages"], 16384);

    let patch_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/balloon/statistics")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(r#"{"stats_polling_interval_s":2}"#))
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
                .body(axum::body::Body::from(r#"{"stats_polling_interval_s":0}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(disable_response.status(), StatusCode::BAD_REQUEST);
    let disable_body = to_bytes(disable_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let disable_payload: ErrorEnvelope = serde_json::from_slice(&disable_body).unwrap();
    assert_eq!(disable_payload.code, "InvalidRequest");
    assert_eq!(
        disable_payload.fault_message,
        BALLOON_STATS_STATE_CHANGE_MESSAGE
    );
}

#[tokio::test]
async fn runtime_balloon_hinting_status_requires_configured_enabled_device() {
    let router = test_state().router();
    let kernel = NamedTempFile::new().unwrap();
    let firecracker = test_state().firecracker_revision;

    let boot_source_response = router
        .clone()
        .oneshot(put_boot_source_request(
            kernel.path().to_str().unwrap(),
            None,
        ))
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
                .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

    let unconfigured_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/balloon/hinting/status")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unconfigured_response.status(), StatusCode::BAD_REQUEST);
    let unconfigured_body = to_bytes(unconfigured_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let unconfigured_payload: ErrorEnvelope = serde_json::from_slice(&unconfigured_body).unwrap();
    assert_eq!(unconfigured_payload.code, "InvalidRequest");
    assert_eq!(
        unconfigured_payload.fault_message,
        BALLOON_UPDATE_NOT_CONFIGURED_MESSAGE
    );
    assert_eq!(
        unconfigured_payload.baseline_firecracker_revision,
        firecracker
    );

    let router = test_state().router();
    let kernel = NamedTempFile::new().unwrap();
    let firecracker = test_state().firecracker_revision;

    let put_balloon_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/balloon")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"amount_mib":64,"deflate_on_oom":true,"free_page_hinting":false}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_balloon_response.status(), StatusCode::NO_CONTENT);

    let boot_source_response = router
        .clone()
        .oneshot(put_boot_source_request(
            kernel.path().to_str().unwrap(),
            None,
        ))
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
                .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

    let disabled_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/balloon/hinting/status")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(disabled_response.status(), StatusCode::BAD_REQUEST);
    let disabled_body = to_bytes(disabled_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let disabled_payload: ErrorEnvelope = serde_json::from_slice(&disabled_body).unwrap();
    assert_eq!(disabled_payload.code, "InvalidRequest");
    assert_eq!(
        disabled_payload.fault_message,
        BALLOON_HINTING_NOT_ENABLED_MESSAGE
    );
    assert_eq!(disabled_payload.baseline_firecracker_revision, firecracker);
}

#[tokio::test]
async fn runtime_balloon_hinting_status_returns_idle_status_when_enabled() {
    let router = test_state().router();
    let kernel = NamedTempFile::new().unwrap();

    let put_balloon_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/balloon")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"amount_mib":64,"deflate_on_oom":true,"free_page_hinting":true}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_balloon_response.status(), StatusCode::NO_CONTENT);

    let boot_source_response = router
        .clone()
        .oneshot(put_boot_source_request(
            kernel.path().to_str().unwrap(),
            None,
        ))
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
                .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/balloon/hinting/status")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload["host_cmd"], 0);
    assert!(payload["guest_cmd"].is_null());
}

#[tokio::test]
async fn runtime_balloon_hinting_control_requires_configured_enabled_device() {
    let router = test_state().router();
    let kernel = NamedTempFile::new().unwrap();
    let firecracker = test_state().firecracker_revision;

    let boot_source_response = router
        .clone()
        .oneshot(put_boot_source_request(
            kernel.path().to_str().unwrap(),
            None,
        ))
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
                .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

    for (path, body) in [
        ("/balloon/hinting/start", axum::body::Body::empty()),
        ("/balloon/hinting/stop", axum::body::Body::from("{not-json")),
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
        let response_body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: ErrorEnvelope = serde_json::from_slice(&response_body).unwrap();
        assert_eq!(payload.code, "InvalidRequest");
        assert_eq!(payload.fault_message, BALLOON_UPDATE_NOT_CONFIGURED_MESSAGE);
        assert_eq!(payload.baseline_firecracker_revision, firecracker);
    }

    let router = test_state().router();
    let kernel = NamedTempFile::new().unwrap();
    let firecracker = test_state().firecracker_revision;

    let put_balloon_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/balloon")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"amount_mib":64,"deflate_on_oom":true,"free_page_hinting":false}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_balloon_response.status(), StatusCode::NO_CONTENT);

    let boot_source_response = router
        .clone()
        .oneshot(put_boot_source_request(
            kernel.path().to_str().unwrap(),
            None,
        ))
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
                .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

    for path in ["/balloon/hinting/start", "/balloon/hinting/stop"] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(path)
                    .header("content-type", "application/json")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let response_body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: ErrorEnvelope = serde_json::from_slice(&response_body).unwrap();
        assert_eq!(payload.code, "InvalidRequest");
        assert_eq!(payload.fault_message, BALLOON_HINTING_NOT_ENABLED_MESSAGE);
        assert_eq!(payload.baseline_firecracker_revision, firecracker);
    }
}

#[tokio::test]
async fn runtime_balloon_hinting_start_and_stop_follow_low_risk_control_flow() {
    let router = test_state().router();
    let kernel = NamedTempFile::new().unwrap();

    let put_balloon_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/balloon")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"amount_mib":64,"deflate_on_oom":true,"free_page_hinting":true}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_balloon_response.status(), StatusCode::NO_CONTENT);

    let boot_source_response = router
        .clone()
        .oneshot(put_boot_source_request(
            kernel.path().to_str().unwrap(),
            None,
        ))
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
                .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

    let first_start_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/balloon/hinting/start")
                .header("content-type", "application/json")
                .body(axum::body::Body::empty())
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
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(first_status_response.status(), StatusCode::OK);
    let first_status_body = to_bytes(first_status_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let first_status: serde_json::Value = serde_json::from_slice(&first_status_body).unwrap();
    assert_eq!(first_status["host_cmd"], BALLOON_HINTING_FIRST_RUN_COMMAND);
    assert!(first_status["guest_cmd"].is_null());

    let second_start_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/balloon/hinting/start")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(r#"{"acknowledge_on_stop":false}"#))
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
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(second_status_response.status(), StatusCode::OK);
    let second_status_body = to_bytes(second_status_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let second_status: serde_json::Value = serde_json::from_slice(&second_status_body).unwrap();
    assert_eq!(
        second_status["host_cmd"],
        BALLOON_HINTING_FIRST_RUN_COMMAND + 1
    );
    assert!(second_status["guest_cmd"].is_null());

    let stop_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/balloon/hinting/stop")
                .header("content-type", "application/json")
                .body(axum::body::Body::from("{not-json"))
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
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(stopped_status_response.status(), StatusCode::OK);
    let stopped_status_body = to_bytes(stopped_status_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let stopped_status: serde_json::Value = serde_json::from_slice(&stopped_status_body).unwrap();
    assert_eq!(stopped_status["host_cmd"], BALLOON_HINTING_DONE_COMMAND);
    assert!(stopped_status["guest_cmd"].is_null());
}

#[tokio::test]
async fn stores_memory_hotplug_config_and_exports_runtime_status() {
    let router = test_state().router();
    let kernel = NamedTempFile::new().unwrap();

    let initial_vm_config_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/vm/config")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(initial_vm_config_response.status(), StatusCode::OK);
    let initial_vm_config_body = to_bytes(initial_vm_config_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let initial_vm_config_payload: serde_json::Value =
        serde_json::from_slice(&initial_vm_config_body).unwrap();
    assert!(initial_vm_config_payload.get("memory-hotplug").is_none());

    let put_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/hotplug/memory")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(r#"{"total_size_mib":1024}"#))
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
    assert_eq!(vm_config_payload["memory-hotplug"]["block_size_mib"], 2);
    assert_eq!(vm_config_payload["memory-hotplug"]["slot_size_mib"], 128);
    assert_eq!(vm_config_payload["memory-hotplug"]["total_size_mib"], 1024);

    let boot_source_response = router
        .clone()
        .oneshot(put_boot_source_request(
            kernel.path().to_str().unwrap(),
            None,
        ))
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
                .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

    let status_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/hotplug/memory")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(status_response.status(), StatusCode::OK);
    let status_body = to_bytes(status_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let status_payload: serde_json::Value = serde_json::from_slice(&status_body).unwrap();
    assert_eq!(status_payload["block_size_mib"], 2);
    assert_eq!(status_payload["slot_size_mib"], 128);
    assert_eq!(status_payload["total_size_mib"], 1024);
    assert_eq!(status_payload["plugged_size_mib"], 0);
    assert_eq!(status_payload["requested_size_mib"], 0);

    let patch_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/hotplug/memory")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(r#"{"requested_size_mib":512}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(patch_response.status(), StatusCode::NO_CONTENT);

    let updated_status_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/hotplug/memory")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(updated_status_response.status(), StatusCode::OK);
    let updated_status_body = to_bytes(updated_status_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let updated_status_payload: serde_json::Value =
        serde_json::from_slice(&updated_status_body).unwrap();
    assert_eq!(updated_status_payload["requested_size_mib"], 512);
}

#[tokio::test]
async fn stores_mmds_config_and_preserves_runtime_datastore() {
    let router = test_state().router();
    let kernel = NamedTempFile::new().unwrap();

    let initial_get_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/mmds")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(initial_get_response.status(), StatusCode::OK);
    let initial_get_body = to_bytes(initial_get_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let initial_get_payload: serde_json::Value = serde_json::from_slice(&initial_get_body).unwrap();
    assert_eq!(initial_get_payload, serde_json::json!({}));

    let put_mmds_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/mmds")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"meta":{"name":"vesper"},"remove":"later"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_mmds_response.status(), StatusCode::NO_CONTENT);

    let patch_mmds_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/mmds")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"meta":{"version":"1"},"remove":null}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(patch_mmds_response.status(), StatusCode::NO_CONTENT);

    let get_mmds_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/mmds")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get_mmds_response.status(), StatusCode::OK);
    let get_mmds_body = to_bytes(get_mmds_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let get_mmds_payload: serde_json::Value = serde_json::from_slice(&get_mmds_body).unwrap();
    assert_eq!(get_mmds_payload["meta"]["name"], "vesper");
    assert_eq!(get_mmds_payload["meta"]["version"], "1");
    assert!(get_mmds_payload.get("remove").is_none());

    let put_network_interface_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/network-interfaces/eth0")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"iface_id":"eth0","host_dev_name":"tap0"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        put_network_interface_response.status(),
        StatusCode::NO_CONTENT
    );

    let put_mmds_config_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/mmds/config")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(r#"{"network_interfaces":["eth0"]}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_mmds_config_response.status(), StatusCode::NO_CONTENT);

    let vm_config_response = router
        .clone()
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

    let boot_source_response = router
        .clone()
        .oneshot(put_boot_source_request(
            kernel.path().to_str().unwrap(),
            None,
        ))
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
                .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

    let runtime_get_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/mmds")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(runtime_get_response.status(), StatusCode::OK);
    let runtime_get_body = to_bytes(runtime_get_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let runtime_get_payload: serde_json::Value = serde_json::from_slice(&runtime_get_body).unwrap();
    assert_eq!(runtime_get_payload["meta"]["name"], "vesper");
    assert_eq!(runtime_get_payload["meta"]["version"], "1");

    let runtime_patch_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/mmds")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(r#"{"meta":{"build":"2"}}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(runtime_patch_response.status(), StatusCode::NO_CONTENT);

    let updated_runtime_get_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/mmds")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(updated_runtime_get_response.status(), StatusCode::OK);
    let updated_runtime_get_body = to_bytes(updated_runtime_get_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let updated_runtime_get_payload: serde_json::Value =
        serde_json::from_slice(&updated_runtime_get_body).unwrap();
    assert_eq!(updated_runtime_get_payload["meta"]["name"], "vesper");
    assert_eq!(updated_runtime_get_payload["meta"]["version"], "1");
    assert_eq!(updated_runtime_get_payload["meta"]["build"], "2");
}

#[tokio::test]
async fn mmds_patch_requires_initialized_store_and_runtime_config() {
    let router = test_state().router();
    let kernel = NamedTempFile::new().unwrap();

    let preboot_patch_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/mmds")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(r#"{"meta":{"name":"vesper"}}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(preboot_patch_response.status(), StatusCode::BAD_REQUEST);
    let preboot_patch_body = to_bytes(preboot_patch_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let preboot_patch_payload: ErrorEnvelope = serde_json::from_slice(&preboot_patch_body).unwrap();
    assert_eq!(preboot_patch_payload.code, "InvalidRequest");
    assert_eq!(
        preboot_patch_payload.fault_message,
        MMDS_NOT_INITIALIZED_MESSAGE
    );

    let put_mmds_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/mmds")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(r#"{"meta":{"name":"vesper"}}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_mmds_response.status(), StatusCode::NO_CONTENT);

    let boot_source_response = router
        .clone()
        .oneshot(put_boot_source_request(
            kernel.path().to_str().unwrap(),
            None,
        ))
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
                .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

    let runtime_get_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/mmds")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(runtime_get_response.status(), StatusCode::BAD_REQUEST);
    let runtime_get_body = to_bytes(runtime_get_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let runtime_get_payload: ErrorEnvelope = serde_json::from_slice(&runtime_get_body).unwrap();
    assert_eq!(runtime_get_payload.code, "InvalidRequest");
    assert_eq!(
        runtime_get_payload.fault_message,
        MMDS_NOT_INITIALIZED_MESSAGE
    );
}

#[tokio::test]
async fn rejects_invalid_mmds_config_payloads_and_oversized_store() {
    let router = test_state().router();

    let empty_networks_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/mmds/config")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(r#"{"network_interfaces":[]}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(empty_networks_response.status(), StatusCode::BAD_REQUEST);
    let empty_networks_body = to_bytes(empty_networks_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let empty_networks_payload: ErrorEnvelope =
        serde_json::from_slice(&empty_networks_body).unwrap();
    assert_eq!(empty_networks_payload.code, "InvalidRequest");
    assert_eq!(
        empty_networks_payload.fault_message,
        MMDS_EMPTY_NETWORK_INTERFACE_LIST_MESSAGE
    );

    let put_network_interface_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/network-interfaces/eth0")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"iface_id":"eth0","host_dev_name":"tap0"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        put_network_interface_response.status(),
        StatusCode::NO_CONTENT
    );

    let unknown_interface_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/mmds/config")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"network_interfaces":["missing"]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unknown_interface_response.status(), StatusCode::BAD_REQUEST);
    let unknown_interface_body = to_bytes(unknown_interface_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let unknown_interface_payload: ErrorEnvelope =
        serde_json::from_slice(&unknown_interface_body).unwrap();
    assert_eq!(unknown_interface_payload.code, "InvalidRequest");
    assert_eq!(
        unknown_interface_payload.fault_message,
        MMDS_INVALID_NETWORK_INTERFACE_ID_MESSAGE
    );

    let invalid_ipv4_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/mmds/config")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"network_interfaces":["eth0"],"ipv4_address":"10.0.0.1"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(invalid_ipv4_response.status(), StatusCode::BAD_REQUEST);
    let invalid_ipv4_body = to_bytes(invalid_ipv4_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let invalid_ipv4_payload: ErrorEnvelope = serde_json::from_slice(&invalid_ipv4_body).unwrap();
    assert_eq!(invalid_ipv4_payload.code, "InvalidRequest");
    assert_eq!(
        invalid_ipv4_payload.fault_message,
        MMDS_INVALID_IPV4_ADDRESS_MESSAGE
    );

    let oversized_payload = format!(
        r#"{{"blob":"{}"}}"#,
        "a".repeat(MMDS_DATA_STORE_LIMIT_BYTES)
    );
    let oversized_response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/mmds")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(oversized_payload))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(oversized_response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let oversized_body = to_bytes(oversized_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let oversized_payload: ErrorEnvelope = serde_json::from_slice(&oversized_body).unwrap();
    assert_eq!(oversized_payload.code, "PayloadTooLarge");
    assert_eq!(
        oversized_payload.fault_message,
        MMDS_DATA_STORE_LIMIT_EXCEEDED_MESSAGE
    );
}

#[tokio::test]
async fn validates_and_exports_boot_source() {
    let router = test_state().router();
    let kernel = NamedTempFile::new().unwrap();
    let initrd = NamedTempFile::new().unwrap();

    let put_response = router
        .clone()
        .oneshot(put_boot_source_request(
            kernel.path().to_str().unwrap(),
            Some(initrd.path().to_str().unwrap()),
        ))
        .await
        .unwrap();
    assert_eq!(put_response.status(), StatusCode::NO_CONTENT);

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
    assert_eq!(
        vm_config_payload["boot-source"]["kernel_image_path"],
        kernel.path().to_str().unwrap()
    );
    assert_eq!(
        vm_config_payload["boot-source"]["initrd_path"],
        initrd.path().to_str().unwrap()
    );
}

#[tokio::test]
async fn stores_cpu_config_without_exporting_it() {
    let router = test_state().router();

    let initial_vm_config_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/vm/config")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(initial_vm_config_response.status(), StatusCode::OK);
    let initial_vm_config_body = to_bytes(initial_vm_config_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let initial_vm_config_payload: serde_json::Value =
        serde_json::from_slice(&initial_vm_config_body).unwrap();
    assert!(initial_vm_config_payload.get("cpu-config").is_none());

    let cpu_config_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/cpu-config")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
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
    assert!(vm_config_payload.get("cpu-config").is_none());
}

#[tokio::test]
async fn rejects_invalid_cpu_config_payloads() {
    let router = test_state().router();

    let response = router
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/cpu-config")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"cpuid_modifiers":[{"leaf":"0x1","subleaf":"0x0","flags":0,"modifiers":[{"register":"invalid","bitmap":"0b1"}]}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload.code, "InvalidRequest");
    assert_eq!(payload.path, "/cpu-config");
}

#[tokio::test]
async fn rejects_invalid_boot_source_payloads() {
    let router = test_state().router();

    let invalid_kernel_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/boot-source")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"kernel_image_path":"/definitely/missing-kernel"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(invalid_kernel_response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(invalid_kernel_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload.code, "InvalidRequest");
    assert!(
        payload
            .fault_message
            .starts_with("The kernel file cannot be opened:")
    );
}

#[tokio::test]
async fn rejects_invalid_memory_hotplug_payloads() {
    let router = test_state().router();

    let response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/hotplug/memory")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(r#"{"total_size_mib":130}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload.code, "InvalidRequest");
    assert_eq!(
        payload.fault_message,
        INVALID_MEMORY_HOTPLUG_TOTAL_SIZE_MESSAGE
    );
}

#[tokio::test]
async fn rejects_instance_start_without_boot_source() {
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

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload.code, "InvalidRequest");
    assert_eq!(payload.fault_message, MISSING_KERNEL_CONFIG_MESSAGE);
}

#[tokio::test]
async fn rejects_flush_metrics_before_start() {
    let router = test_state().router();
    let response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/actions")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(r#"{"action_type":"FlushMetrics"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload.code, "InvalidState");
    assert_eq!(payload.fault_message, ACTION_NOT_SUPPORTED_PREBOOT_MESSAGE);
}

#[tokio::test]
async fn flush_metrics_succeeds_after_start_without_metrics_init() {
    let router = test_state().router();
    let kernel = NamedTempFile::new().unwrap();

    let boot_source_response = router
        .clone()
        .oneshot(put_boot_source_request(
            kernel.path().to_str().unwrap(),
            None,
        ))
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
                .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

    let flush_response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/actions")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(r#"{"action_type":"FlushMetrics"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(flush_response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn rejects_instance_start_after_boot_with_firecracker_postboot_message() {
    let router = test_state().router();
    let kernel = NamedTempFile::new().unwrap();

    let boot_source_response = router
        .clone()
        .oneshot(put_boot_source_request(
            kernel.path().to_str().unwrap(),
            None,
        ))
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
                .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(first_start_response.status(), StatusCode::NO_CONTENT);

    let second_start_response = router
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
    assert_eq!(second_start_response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(second_start_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload.code, "InvalidState");
    assert_eq!(payload.fault_message, ACTION_NOT_SUPPORTED_POSTBOOT_MESSAGE);
}

#[tokio::test]
#[cfg(target_arch = "x86_64")]
async fn rejects_send_ctrl_alt_del_before_start() {
    let router = test_state().router();
    let response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/actions")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"action_type":"SendCtrlAltDel"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload.code, "InvalidState");
    assert_eq!(payload.fault_message, ACTION_NOT_SUPPORTED_PREBOOT_MESSAGE);
}

#[tokio::test]
#[cfg(target_arch = "x86_64")]
async fn send_ctrl_alt_del_succeeds_after_start() {
    let router = test_state().router();
    let kernel = NamedTempFile::new().unwrap();

    let boot_source_response = router
        .clone()
        .oneshot(put_boot_source_request(
            kernel.path().to_str().unwrap(),
            None,
        ))
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
                .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

    let ctrl_alt_del_response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/actions")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"action_type":"SendCtrlAltDel"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ctrl_alt_del_response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
#[cfg(target_arch = "aarch64")]
async fn rejects_send_ctrl_alt_del_on_aarch64() {
    let router = test_state().router();
    let response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/actions")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"action_type":"SendCtrlAltDel"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload.code, "InvalidRequest");
    assert_eq!(payload.fault_message, SEND_CTRL_ALT_DEL_AARCH64_MESSAGE);
}

#[tokio::test]
async fn merges_logger_updates_and_rejects_invalid_targets() {
    let state = Arc::new(test_state());

    let first_response = handle_put_logger(
        &state,
        &Method::PUT,
        "/logger",
        Bytes::from_static(br#"{"level":"warning"}"#),
    );
    assert_eq!(first_response.status(), StatusCode::NO_CONTENT);

    let second_response = handle_put_logger(
        &state,
        &Method::PUT,
        "/logger",
        Bytes::from_static(br#"{"show_level":true,"module":"api_server::request"}"#),
    );
    assert_eq!(second_response.status(), StatusCode::NO_CONTENT);

    let logger_config = state
        .logger_config
        .read()
        .expect("logger config lock poisoned while reading")
        .clone();
    assert_eq!(logger_config.level, LoggerLevel::Warn);
    assert!(logger_config.show_level);
    assert!(!logger_config.show_log_origin);
    assert_eq!(logger_config.module.as_deref(), Some("api_server::request"));

    let invalid_response = handle_put_logger(
        &state,
        &Method::PUT,
        "/logger",
        Bytes::from_static(br#"{"log_path":"/definitely/missing-dir/logger.log"}"#),
    );
    assert_eq!(invalid_response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(invalid_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload.code, "InvalidRequest");
    assert!(
        payload
            .fault_message
            .starts_with("Failed to open target file:")
    );
}

#[tokio::test]
async fn initializes_metrics_once_and_omits_runtime_export() {
    let router = test_state().router();
    let tempdir = tempfile::tempdir().unwrap();
    let metrics_path = tempdir.path().join("metrics.jsonl");
    let metrics_payload = format!(r#"{{"metrics_path":"{}"}}"#, metrics_path.display());

    let first_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/metrics")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(metrics_payload.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(first_response.status(), StatusCode::NO_CONTENT);

    let duplicate_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/metrics")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(metrics_payload))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(duplicate_response.status(), StatusCode::BAD_REQUEST);
    let duplicate_body = to_bytes(duplicate_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let duplicate_payload: ErrorEnvelope = serde_json::from_slice(&duplicate_body).unwrap();
    assert_eq!(duplicate_payload.code, "InvalidRequest");
    assert_eq!(
        duplicate_payload.fault_message,
        METRICS_ALREADY_INITIALIZED_MESSAGE
    );

    let vm_config_response = router
        .clone()
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
    assert!(vm_config_payload.get("logger").is_none());
    assert!(vm_config_payload.get("metrics").is_none());
}

#[tokio::test]
async fn stores_serial_config_and_omits_runtime_export() {
    let state = Arc::new(test_state());

    let serial_response = handle_put_serial(
        &state,
        &Method::PUT,
        "/serial",
        Bytes::from_static(br#"{"serial_out_path":"serial.log"}"#),
    );
    assert_eq!(serial_response.status(), StatusCode::NO_CONTENT);

    let serial_config = state
        .serial_config
        .read()
        .expect("serial config lock poisoned while reading")
        .clone();
    assert_eq!(serial_config.serial_out_path.as_deref(), Some("serial.log"));

    let vm_config_response = full_vm_config_response(&state);
    assert_eq!(vm_config_response.status(), StatusCode::OK);
    let vm_config_body = to_bytes(vm_config_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let vm_config_payload: serde_json::Value = serde_json::from_slice(&vm_config_body).unwrap();
    assert!(vm_config_payload.get("serial").is_none());
}

#[tokio::test]
async fn stores_entropy_config_and_exports_it() {
    let router = test_state().router();

    let entropy_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/entropy")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
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
async fn rejects_entropy_after_start() {
    let router = test_state().router();
    let kernel = NamedTempFile::new().unwrap();

    let boot_source_response = router
        .clone()
        .oneshot(put_boot_source_request(
            kernel.path().to_str().unwrap(),
            None,
        ))
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
                .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

    let entropy_response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/entropy")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(r#"{}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(entropy_response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(entropy_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload.code, "InvalidState");
}

#[tokio::test]
async fn rejects_cpu_config_after_start() {
    let router = test_state().router();
    let kernel = NamedTempFile::new().unwrap();

    let boot_source_response = router
        .clone()
        .oneshot(put_boot_source_request(
            kernel.path().to_str().unwrap(),
            None,
        ))
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
                .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

    let cpu_config_response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/cpu-config")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(r#"{"kvm_capabilities":["1"]}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(cpu_config_response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(cpu_config_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload.code, "InvalidState");
}

#[tokio::test]
async fn stores_vsock_config_and_omits_deprecated_export_field() {
    let router = test_state().router();

    let vsock_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/vsock")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
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
    assert_eq!(vm_config_payload["vsock"]["guest_cid"], 5);
    assert_eq!(
        vm_config_payload["vsock"]["uds_path"],
        "/tmp/vesper-vsock.sock"
    );
    assert!(vm_config_payload["vsock"].get("vsock_id").is_none());
}

#[tokio::test]
async fn rejects_invalid_vsock_guest_cid() {
    let router = test_state().router();

    let response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/vsock")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"guest_cid":2,"uds_path":"/tmp/vesper-vsock.sock"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload.code, "InvalidRequest");
    assert_eq!(payload.fault_message, INVALID_VSOCK_GUEST_CID_MESSAGE);
}

#[tokio::test]
async fn drive_stub_validates_path_body_id_match() {
    let router = test_state().router();
    let root_backing_file = NamedTempFile::new().unwrap();

    let mismatch_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/drives/rootfs")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"drive_id":"data","is_root_device":true,"path_on_host":"/tmp/rootfs.ext4"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
    assert_eq!(mismatch_response.status(), StatusCode::BAD_REQUEST);
    let mismatch_body = to_bytes(mismatch_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let mismatch_payload: ErrorEnvelope = serde_json::from_slice(&mismatch_body).unwrap();
    assert_eq!(mismatch_payload.code, "InvalidRequest");
    assert_eq!(
        mismatch_payload.fault_message,
        DRIVE_ID_PATH_MISMATCH_MESSAGE
    );

    let valid_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/drives/rootfs")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(format!(
                    r#"{{"drive_id":"rootfs","is_root_device":true,"path_on_host":"{}"}}"#,
                    root_backing_file.path().display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(valid_response.status(), StatusCode::NO_CONTENT);

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
    assert_eq!(vm_config_payload["drives"][0]["drive_id"], "rootfs");
}

#[tokio::test]
async fn drive_stub_enforces_root_device_conflicts() {
    let router = test_state().router();
    let pmem_root_backing_file = NamedTempFile::new().unwrap();
    pmem_root_backing_file.as_file().set_len(0x1000).unwrap();

    let pmem_root_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/pmem/pmem0")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(format!(
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
                    .body(axum::body::Body::from(
                        r#"{"drive_id":"rootfs","is_root_device":true,"path_on_host":"/tmp/rootfs.ext4"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
    assert_eq!(pmem_conflict_response.status(), StatusCode::BAD_REQUEST);
    let pmem_conflict_body = to_bytes(pmem_conflict_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let pmem_conflict_payload: ErrorEnvelope = serde_json::from_slice(&pmem_conflict_body).unwrap();
    assert_eq!(pmem_conflict_payload.code, "InvalidRequest");
    assert_eq!(
        pmem_conflict_payload.fault_message,
        DRIVE_ROOT_PMEM_CONFLICT_MESSAGE
    );

    let second_router = test_state().router();
    let first_root_backing_file = NamedTempFile::new().unwrap();
    let replacement_root_backing_file = NamedTempFile::new().unwrap();
    let first_root_response = second_router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/drives/rootfs")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(format!(
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
                .body(axum::body::Body::from(
                    r#"{"drive_id":"data","is_root_device":true,"path_on_host":"/tmp/data.ext4"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(second_root_response.status(), StatusCode::BAD_REQUEST);
    let second_root_body = to_bytes(second_root_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let second_root_payload: ErrorEnvelope = serde_json::from_slice(&second_root_body).unwrap();
    assert_eq!(second_root_payload.code, "InvalidRequest");
    assert_eq!(
        second_root_payload.fault_message,
        DRIVE_ROOT_ALREADY_EXISTS_MESSAGE
    );

    let update_existing_root_response = second_router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/drives/rootfs")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(format!(
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
    assert_eq!(vm_config_payload["drives"][0]["drive_id"], "rootfs");
    assert_eq!(
        vm_config_payload["drives"][0]["path_on_host"],
        replacement_root_backing_file.path().display().to_string()
    );
}

#[tokio::test]
async fn drive_stub_rejects_missing_backing_files() {
    let router = test_state().router();
    let missing_backing_file = "/definitely/missing-drive.ext4";

    let missing_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/drives/rootfs")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(format!(
                    r#"{{"drive_id":"rootfs","is_root_device":true,"path_on_host":"{}"}}"#,
                    missing_backing_file
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing_response.status(), StatusCode::BAD_REQUEST);
    let missing_body = to_bytes(missing_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let missing_payload: ErrorEnvelope = serde_json::from_slice(&missing_body).unwrap();
    assert_eq!(missing_payload.code, "InvalidRequest");
    assert_eq!(
        missing_payload.fault_message,
        format!(
            "Drive config error: Unable to create the virtio block device: Virtio backend error: Error manipulating the backing file: No such file or directory (os error 2) {}",
            missing_backing_file
        )
    );

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
    assert!(vm_config_payload["drives"].is_null());
}

#[tokio::test]
async fn drive_stub_defaults_cache_type_in_export() {
    let router = test_state().router();
    let root_backing_file = NamedTempFile::new().unwrap();

    let create_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/drives/rootfs")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(format!(
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
    assert_eq!(vm_config_payload["drives"][0]["cache_type"], "Unsafe");
}

#[tokio::test]
async fn drive_export_orders_root_device_first() {
    let router = test_state().router();
    let data_backing_file = NamedTempFile::new().unwrap();
    let root_backing_file = NamedTempFile::new().unwrap();

    let data_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/drives/data")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(format!(
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
                .body(axum::body::Body::from(format!(
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
    assert_eq!(vm_config_payload["drives"][0]["drive_id"], "rootfs");
    assert_eq!(vm_config_payload["drives"][1]["drive_id"], "data");
}

#[tokio::test]
async fn runtime_drive_patch_requires_existing_drive_and_updates_config() {
    let router = test_state().router();
    let kernel = NamedTempFile::new().unwrap();
    let root_backing_file = NamedTempFile::new().unwrap();

    let create_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/drives/rootfs")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(format!(
                    r#"{{"drive_id":"rootfs","is_root_device":true,"path_on_host":"{}"}}"#,
                    root_backing_file.path().display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::NO_CONTENT);

    let boot_source_response = router
        .clone()
        .oneshot(put_boot_source_request(
            kernel.path().to_str().unwrap(),
            None,
        ))
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
                .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

    let missing_drive_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/drives/data")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"drive_id":"data","path_on_host":"/tmp/data.ext4"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing_drive_response.status(), StatusCode::BAD_REQUEST);
    let missing_drive_body = to_bytes(missing_drive_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let missing_drive_payload: ErrorEnvelope = serde_json::from_slice(&missing_drive_body).unwrap();
    assert_eq!(missing_drive_payload.code, "InvalidRequest");
    assert_eq!(
        missing_drive_payload.fault_message,
        DRIVE_UPDATE_NOT_FOUND_MESSAGE
    );

    let update_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/drives/rootfs")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
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
    assert_eq!(
        vm_config_payload["drives"][0]["path_on_host"],
        "/tmp/rootfs-updated.ext4"
    );
}

#[tokio::test]
async fn network_interface_patch_stub_validates_path_body_id_match() {
    let router = test_state().router();
    let kernel = NamedTempFile::new().unwrap();

    let create_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/network-interfaces/eth0")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"iface_id":"eth0","host_dev_name":"tap0"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::NO_CONTENT);

    let boot_source_response = router
        .clone()
        .oneshot(put_boot_source_request(
            kernel.path().to_str().unwrap(),
            None,
        ))
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
                .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

    let mismatch_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/network-interfaces/eth0")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"iface_id":"eth1","rx_rate_limiter":{"bandwidth":{"size":1024,"refill_time":1000}}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
    assert_eq!(mismatch_response.status(), StatusCode::BAD_REQUEST);
    let mismatch_body = to_bytes(mismatch_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let mismatch_payload: ErrorEnvelope = serde_json::from_slice(&mismatch_body).unwrap();
    assert_eq!(mismatch_payload.code, "InvalidRequest");
    assert_eq!(
        mismatch_payload.fault_message,
        network_id_path_mismatch_message("eth0", "eth1")
    );

    let missing_interface_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/network-interfaces/eth9")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"iface_id":"eth9","rx_rate_limiter":{"bandwidth":{"size":1024,"refill_time":1000}}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
    assert_eq!(missing_interface_response.status(), StatusCode::BAD_REQUEST);
    let missing_interface_body = to_bytes(missing_interface_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let missing_interface_payload: ErrorEnvelope =
        serde_json::from_slice(&missing_interface_body).unwrap();
    assert_eq!(missing_interface_payload.code, "InvalidRequest");
    assert_eq!(
        missing_interface_payload.fault_message,
        NETWORK_UPDATE_NOT_FOUND_MESSAGE
    );

    let valid_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/network-interfaces/eth0")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"iface_id":"eth0","rx_rate_limiter":{"bandwidth":{"size":1024,"refill_time":1000}}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
    assert_eq!(valid_response.status(), StatusCode::NO_CONTENT);

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
    assert_eq!(
        vm_config_payload["network-interfaces"][0]["iface_id"],
        "eth0"
    );
    assert_eq!(
        vm_config_payload["network-interfaces"][0]["rx_rate_limiter"]["bandwidth"]["size"],
        1024
    );
}

#[tokio::test]
async fn pmem_stub_validates_path_body_id_match() {
    let router = test_state().router();
    let backing_file = NamedTempFile::new().unwrap();
    backing_file.as_file().set_len(0x1000).unwrap();

    let mismatch_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/pmem/rootfs")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"id":"data","path_on_host":"/tmp/rootfs.img"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(mismatch_response.status(), StatusCode::BAD_REQUEST);
    let mismatch_body = to_bytes(mismatch_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let mismatch_payload: ErrorEnvelope = serde_json::from_slice(&mismatch_body).unwrap();
    assert_eq!(mismatch_payload.code, "InvalidRequest");
    assert_eq!(
        mismatch_payload.fault_message,
        PMEM_ID_PATH_MISMATCH_MESSAGE
    );

    let valid_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/pmem/rootfs")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(format!(
                    r#"{{"id":"rootfs","path_on_host":"{}"}}"#,
                    backing_file.path().display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(valid_response.status(), StatusCode::NO_CONTENT);

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
    assert_eq!(vm_config_payload["pmem"][0]["id"], "rootfs");
    assert_eq!(vm_config_payload["pmem"][0]["root_device"], false);
    assert_eq!(vm_config_payload["pmem"][0]["read_only"], false);
}

async fn send_json(
    router: &Router,
    method: &str,
    uri: &str,
    body: &str,
) -> (StatusCode, serde_json::Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .body(axum::body::Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, value)
}

async fn get_json(router: &Router, uri: &str) -> serde_json::Value {
    let (status, value) = send_json(router, "GET", uri, "").await;
    assert_eq!(status, StatusCode::OK);
    value
}

async fn put_kernel(router: &Router, kernel: &NamedTempFile) {
    let response = router
        .clone()
        .oneshot(put_boot_source_request(kernel.path().to_str().unwrap(), None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

async fn instance_start(router: &Router) -> (StatusCode, serde_json::Value) {
    send_json(router, "PUT", "/actions", r#"{"action_type":"InstanceStart"}"#).await
}

fn pmem_body(id: &str, path: &std::path::Path, extra: &str) -> String {
    format!(r#"{{"id":"{id}","path_on_host":"{}"{extra}}}"#, path.display())
}

const RATE_LIMITER_JSON: &str =
    r#"{"bandwidth":{"size":1048576,"refill_time":100},"ops":{"size":10,"one_time_burst":5,"refill_time":1000}}"#;

#[tokio::test]
async fn pmem_backing_files_are_validated_at_instance_start() {
    let router = test_state().router();
    let kernel = NamedTempFile::new().unwrap();
    put_kernel(&router, &kernel).await;
    let missing_dir = tempfile::tempdir().unwrap();
    let missing_path = missing_dir.path().join("missing.img");
    let zero_sized_backing_file = NamedTempFile::new().unwrap();
    let backing_file = NamedTempFile::new().unwrap();
    backing_file.as_file().set_len(0x1000).unwrap();

    // Firecracker stores the pmem config without opening the backing file.
    let (status, _) = send_json(&router, "PUT", "/pmem/pmem0", &pmem_body("pmem0", &missing_path, "")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, payload) = instance_start(&router).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(payload["code"], "InvalidRequest");
    assert_eq!(
        payload["fault_message"],
        "Start microvm error: Cannot create pmem device: Error accessing backing file: No such file or directory (os error 2)"
    );
    assert_eq!(get_json(&router, "/").await["state"], "Not started");

    let (status, _) = send_json(
        &router,
        "PUT",
        "/pmem/pmem0",
        &pmem_body("pmem0", zero_sized_backing_file.path(), ""),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, payload) = instance_start(&router).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(payload["fault_message"], PMEM_BACKING_FILE_ZERO_SIZE_MESSAGE);
    assert_eq!(get_json(&router, "/").await["state"], "Not started");

    let (status, _) = send_json(&router, "PUT", "/pmem/pmem0", &pmem_body("pmem0", backing_file.path(), "")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = instance_start(&router).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(get_json(&router, "/").await["state"], "Running");
}

#[tokio::test]
async fn pmem_put_accepts_and_exports_rate_limiter() {
    let router = test_state().router();
    let backing_file = NamedTempFile::new().unwrap();
    backing_file.as_file().set_len(0x1000).unwrap();

    let (status, _) = send_json(
        &router,
        "PUT",
        "/pmem/pmem0",
        &pmem_body("pmem0", backing_file.path(), &format!(r#","rate_limiter":{RATE_LIMITER_JSON}"#)),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let vm_config = get_json(&router, "/vm/config").await;
    assert_eq!(
        vm_config["pmem"][0]["rate_limiter"],
        serde_json::from_str::<serde_json::Value>(RATE_LIMITER_JSON).unwrap()
    );

    let (status, payload) = send_json(
        &router,
        "PUT",
        "/pmem/pmem1",
        &pmem_body("pmem1", backing_file.path(), r#","rate_limiter":{"bandwidth":{"size":1}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(payload["fault_message"], "invalid JSON payload");
}

#[tokio::test]
async fn pmem_patch_requires_running_vm_and_matching_ids() {
    let router = test_state().router();
    let kernel = NamedTempFile::new().unwrap();
    put_kernel(&router, &kernel).await;
    let backing_file = NamedTempFile::new().unwrap();
    backing_file.as_file().set_len(0x1000).unwrap();
    let (status, _) = send_json(&router, "PUT", "/pmem/pmem0", &pmem_body("pmem0", backing_file.path(), "")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let patch_body = format!(r#"{{"id":"pmem0","rate_limiter":{RATE_LIMITER_JSON}}}"#);

    let (status, payload) = send_json(&router, "PATCH", "/pmem/pmem0", &patch_body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(payload["code"], "InvalidState");
    assert_eq!(payload["fault_message"], ACTION_NOT_SUPPORTED_PREBOOT_MESSAGE);

    let (status, _) = instance_start(&router).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, payload) = send_json(&router, "PATCH", "/pmem/pmem0", &patch_body).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{payload}");

    // Firecracker's GET /vm/config reports the PUT-time pmem config, not the patched limiter.
    let vm_config = get_json(&router, "/vm/config").await;
    assert!(vm_config["pmem"][0]["rate_limiter"].is_null());

    let (status, payload) = send_json(
        &router,
        "PATCH",
        "/pmem/pmem1",
        &format!(r#"{{"id":"pmem1","rate_limiter":{RATE_LIMITER_JSON}}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(payload["code"], "InvalidRequest");
    assert_eq!(payload["fault_message"], PMEM_UPDATE_NOT_FOUND_MESSAGE);

    // Without a rate limiter Firecracker does not look the device up.
    let (status, _) = send_json(&router, "PATCH", "/pmem/pmem1", r#"{"id":"pmem1"}"#).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, payload) = send_json(&router, "PATCH", "/pmem/pmem1", &patch_body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(payload["fault_message"], PMEM_ID_PATH_MISMATCH_MESSAGE);

    for bad_body in [
        "{not-json",
        r#"{"rate_limiter":{}}"#,
        r#"{"id":"pmem0","path_on_host":"/tmp/x"}"#,
        r#"{"id":"pmem0","rate_limiter":{"bandwidth":{"size":1}}}"#,
    ] {
        let (status, payload) = send_json(&router, "PATCH", "/pmem/pmem0", bad_body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad_body}");
        assert_eq!(payload["fault_message"], "invalid JSON payload", "{bad_body}");
    }
}

#[tokio::test]
async fn postboot_device_puts_report_pci_not_enabled() {
    let router = test_state().router();
    let kernel = NamedTempFile::new().unwrap();
    put_kernel(&router, &kernel).await;
    let (status, _) = instance_start(&router).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let backing_file = NamedTempFile::new().unwrap();
    backing_file.as_file().set_len(0x1000).unwrap();

    for (uri, body) in [
        (
            "/drives/data",
            format!(
                r#"{{"drive_id":"data","is_root_device":false,"path_on_host":"{}"}}"#,
                backing_file.path().display()
            ),
        ),
        (
            "/network-interfaces/eth0",
            r#"{"iface_id":"eth0","host_dev_name":"tap0"}"#.to_string(),
        ),
        ("/pmem/pmem0", pmem_body("pmem0", backing_file.path(), "")),
    ] {
        let (status, payload) = send_json(&router, "PUT", uri, &body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}");
        assert_eq!(payload["code"], "InvalidRequest", "{uri}");
        assert_eq!(payload["fault_message"], PCI_NOT_ENABLED_MESSAGE, "{uri}");
    }

    let (status, payload) = send_json(
        &router,
        "PUT",
        "/pmem/pmem0",
        &pmem_body("pmem1", backing_file.path(), ""),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(payload["fault_message"], PMEM_ID_PATH_MISMATCH_MESSAGE);

    let vm_config = get_json(&router, "/vm/config").await;
    assert!(vm_config["drives"].is_null());
    assert!(vm_config["network-interfaces"].is_null());
    assert!(vm_config["pmem"].is_null());
}

#[tokio::test]
async fn network_put_validates_mtu_bounds() {
    let router = test_state().router();

    let (status, payload) = send_json(
        &router,
        "PUT",
        "/network-interfaces/eth0",
        r#"{"iface_id":"eth0","host_dev_name":"tap0","mtu":67}"#,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(payload["code"], "InvalidRequest");
    assert_eq!(
        payload["fault_message"],
        "Network config error: Could not create the network device: MTU 67 is out of range [68, 65535]"
    );

    let (status, payload) = send_json(
        &router,
        "PUT",
        "/network-interfaces/eth0",
        r#"{"iface_id":"eth0","host_dev_name":"tap0","mtu":65536}"#,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(payload["fault_message"], "invalid JSON payload");
    assert!(get_json(&router, "/vm/config").await["network-interfaces"].is_null());

    for (iface, tap, mtu) in [("eth0", "tap0", 68), ("eth1", "tap1", 65535)] {
        let (status, _) = send_json(
            &router,
            "PUT",
            &format!("/network-interfaces/{iface}"),
            &format!(r#"{{"iface_id":"{iface}","host_dev_name":"{tap}","mtu":{mtu}}}"#),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }
    let (status, _) = send_json(
        &router,
        "PUT",
        "/network-interfaces/eth2",
        r#"{"iface_id":"eth2","host_dev_name":"tap2"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let interfaces = get_json(&router, "/vm/config").await["network-interfaces"].clone();
    assert_eq!(interfaces[0]["mtu"], 68);
    assert_eq!(interfaces[1]["mtu"], 65535);
    assert!(interfaces[2]["mtu"].is_null());
}

#[tokio::test]
async fn serial_put_accepts_token_bucket_rate_limiter() {
    let state = Arc::new(test_state());

    let response = handle_put_serial(
        &state,
        &Method::PUT,
        "/serial",
        Bytes::from_static(
            br#"{"serial_out_path":"serial.log","rate_limiter":{"size":4096,"one_time_burst":1024,"refill_time":100}}"#,
        ),
    );
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        *state
            .serial_config
            .read()
            .expect("serial config lock poisoned while reading"),
        SerialConfig {
            serial_out_path: Some("serial.log".to_string()),
            rate_limiter: Some(TokenBucket {
                one_time_burst: Some(1024),
                refill_time: 100,
                size: 4096,
            }),
        }
    );

    for bad_body in [
        r#"{"rate_limiter":{"size":4096}}"#,
        r#"{"rate_limiter":{"bandwidth":{"size":1,"refill_time":1}}}"#,
    ] {
        let response = handle_put_serial(
            &state,
            &Method::PUT,
            "/serial",
            Bytes::from(bad_body.to_string()),
        );
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{bad_body}");
    }
}

#[tokio::test]
async fn snapshot_load_accepts_vsock_override_and_clock_realtime() {
    for body in [
        r#"{"snapshot_path":"snap","mem_file_path":"mem","vsock_override":{"uds_path":"/tmp/v.sock"}}"#,
        r#"{"snapshot_path":"snap","mem_file_path":"mem","clock_realtime":false}"#,
    ] {
        let router = test_state().router();
        let (status, payload) = send_json(&router, "PUT", "/snapshot/load", body).await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}: {payload}");
        assert_eq!(get_json(&router, "/").await["state"], "Paused");
    }

    for body in [
        r#"{"snapshot_path":"snap","mem_file_path":"mem","vsock_override":{}}"#,
        r#"{"snapshot_path":"snap","mem_file_path":"mem","vsock_override":"/tmp/v.sock"}"#,
        r#"{"snapshot_path":"snap","mem_file_path":"mem","clock_realtime":"yes"}"#,
    ] {
        let router = test_state().router();
        let (status, payload) = send_json(&router, "PUT", "/snapshot/load", body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(payload["fault_message"], "invalid JSON payload");
        assert_eq!(get_json(&router, "/").await["state"], "Not started");
    }
}

#[tokio::test]
async fn snapshot_load_clock_realtime_follows_target_architecture() {
    let router = test_state().router();
    let (status, payload) = send_json(
        &router,
        "PUT",
        "/snapshot/load",
        r#"{"snapshot_path":"snap","mem_file_path":"mem","clock_realtime":true}"#,
    )
    .await;

    #[cfg(target_arch = "aarch64")]
    {
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(payload["code"], "InvalidRequest");
        assert_eq!(
            payload["fault_message"],
            SNAPSHOT_LOAD_CLOCK_REALTIME_AARCH64_MESSAGE
        );
        assert_eq!(get_json(&router, "/").await["state"], "Not started");
    }

    #[cfg(not(target_arch = "aarch64"))]
    {
        assert_eq!(status, StatusCode::NO_CONTENT, "{payload}");
        assert_eq!(get_json(&router, "/").await["state"], "Paused");
    }
}

#[tokio::test]
async fn pmem_stub_enforces_root_device_conflicts() {
    let router = test_state().router();
    let root_backing_file = NamedTempFile::new().unwrap();
    root_backing_file.as_file().set_len(0x1000).unwrap();
    let replacement_backing_file = NamedTempFile::new().unwrap();
    replacement_backing_file.as_file().set_len(0x1000).unwrap();
    let drive_root_backing_file = NamedTempFile::new().unwrap();

    let drive_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/drives/rootfs")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(format!(
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
                .body(axum::body::Body::from(format!(
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
    let block_root_conflict_body = to_bytes(block_root_conflict_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let block_root_conflict_payload: ErrorEnvelope =
        serde_json::from_slice(&block_root_conflict_body).unwrap();
    assert_eq!(block_root_conflict_payload.code, "InvalidRequest");
    assert_eq!(
        block_root_conflict_payload.fault_message,
        PMEM_ROOT_BLOCK_CONFLICT_MESSAGE
    );

    let second_router = test_state().router();
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
                .body(axum::body::Body::from(format!(
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
                .body(axum::body::Body::from(format!(
                    r#"{{"id":"pmem1","path_on_host":"{}","root_device":true}}"#,
                    second_root_backing_file.path().display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(second_root_response.status(), StatusCode::BAD_REQUEST);
    let second_root_body = to_bytes(second_root_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let second_root_payload: ErrorEnvelope = serde_json::from_slice(&second_root_body).unwrap();
    assert_eq!(second_root_payload.code, "InvalidRequest");
    assert_eq!(
        second_root_payload.fault_message,
        PMEM_ROOT_ALREADY_EXISTS_MESSAGE
    );

    let update_existing_root_response = second_router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/pmem/pmem0")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(format!(
                    r#"{{"id":"pmem0","path_on_host":"{}","root_device":true,"read_only":true}}"#,
                    replacement_backing_file.path().display()
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
    assert_eq!(vm_config_payload["pmem"][0]["id"], "pmem0");
    assert_eq!(vm_config_payload["pmem"][0]["root_device"], true);
    assert_eq!(vm_config_payload["pmem"][0]["read_only"], true);
}

#[tokio::test]
async fn snapshot_load_stub_requires_exactly_one_memory_source() {
    let router = test_state().router();

    for (body, expected_message) in [
        (
            r#"{"snapshot_path":"state.snap"}"#,
            SNAPSHOT_LOAD_MISSING_MEMORY_SOURCE_MESSAGE,
        ),
        (
            r#"{"snapshot_path":"state.snap","mem_file_path":"mem.snap","mem_backend":{"backend_type":"File","backend_path":"mem.snap"}}"#,
            SNAPSHOT_LOAD_TOO_MANY_MEMORY_SOURCES_MESSAGE,
        ),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/snapshot/load")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let response_body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: ErrorEnvelope = serde_json::from_slice(&response_body).unwrap();
        assert_eq!(payload.code, "InvalidRequest");
        assert_eq!(payload.fault_message, expected_message);
    }

    let valid_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/snapshot/load")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"snapshot_path":"state.snap","mem_backend":{"backend_type":"File","backend_path":"mem.snap"}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
    assert_eq!(valid_response.status(), StatusCode::NO_CONTENT);

    let instance_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(instance_response.status(), StatusCode::OK);
    let instance_body = to_bytes(instance_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let payload: serde_json::Value = serde_json::from_slice(&instance_body).unwrap();
    assert_eq!(payload["state"], "Paused");
}

#[tokio::test]
async fn snapshot_load_stub_rejects_when_non_logger_resources_are_configured() {
    let router = test_state().router();
    let kernel = NamedTempFile::new().unwrap();

    let boot_source_response = router
        .clone()
        .oneshot(put_boot_source_request(
            kernel.path().to_str().unwrap(),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

    let response = router
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/snapshot/load")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"snapshot_path":"state.snap","mem_backend":{"backend_type":"File","backend_path":"mem.snap"}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let response_body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload: ErrorEnvelope = serde_json::from_slice(&response_body).unwrap();
    assert_eq!(payload.code, "InvalidState");
    assert_eq!(payload.fault_message, SNAPSHOT_LOAD_RESOURCE_GUARD_MESSAGE);
}

#[tokio::test]
async fn snapshot_load_stub_allows_logger_metrics_and_serial() {
    let router = test_state().router();
    let tempdir = tempfile::tempdir().unwrap();
    let metrics_path = tempdir.path().join("snapshot-load-metrics.jsonl");

    let logger_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/logger")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(r#"{"level":"info"}"#))
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
                .body(axum::body::Body::from(format!(
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
                .body(axum::body::Body::from(
                    r#"{"serial_out_path":"serial.log"}"#,
                ))
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
                    .body(axum::body::Body::from(
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
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(instance_response.status(), StatusCode::OK);
    let instance_body = to_bytes(instance_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let payload: serde_json::Value = serde_json::from_slice(&instance_body).unwrap();
    assert_eq!(payload["state"], "Paused");
}

#[tokio::test]
async fn snapshot_create_stub_requires_paused_vm() {
    let router = test_state().router();
    let kernel = NamedTempFile::new().unwrap();

    let boot_source_response = router
        .clone()
        .oneshot(put_boot_source_request(
            kernel.path().to_str().unwrap(),
            None,
        ))
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
                .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

    let running_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/snapshot/create")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"mem_file_path":"mem.snap","snapshot_path":"state.snap"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(running_response.status(), StatusCode::BAD_REQUEST);
    let running_body = to_bytes(running_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let running_payload: ErrorEnvelope = serde_json::from_slice(&running_body).unwrap();
    assert_eq!(running_payload.code, "InvalidState");
    assert_eq!(
        running_payload.fault_message,
        SNAPSHOT_CREATE_REQUIRES_PAUSED_MESSAGE
    );

    let pause_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/vm")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(r#"{"state":"Paused"}"#))
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
                .body(axum::body::Body::from(
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
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(instance_response.status(), StatusCode::OK);
    let instance_body = to_bytes(instance_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let payload: serde_json::Value = serde_json::from_slice(&instance_body).unwrap();
    assert_eq!(payload["state"], "Paused");
}

#[tokio::test]
async fn snapshot_load_stub_resumes_vm_when_requested() {
    let router = test_state().router();

    let snapshot_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/snapshot/load")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
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
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(instance_response.status(), StatusCode::OK);
    let instance_body = to_bytes(instance_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let payload: serde_json::Value = serde_json::from_slice(&instance_body).unwrap();
    assert_eq!(payload["state"], "Running");
}

#[tokio::test]
async fn rejects_vsock_after_start() {
    let router = test_state().router();
    let kernel = NamedTempFile::new().unwrap();

    let boot_source_response = router
        .clone()
        .oneshot(put_boot_source_request(
            kernel.path().to_str().unwrap(),
            None,
        ))
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
                .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

    let vsock_response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/vsock")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"guest_cid":5,"uds_path":"/tmp/vesper-vsock.sock"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vsock_response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(vsock_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload.code, "InvalidState");
}

#[tokio::test]
async fn allows_serial_after_start() {
    let router = test_state().router();
    let kernel = NamedTempFile::new().unwrap();

    let boot_source_response = router
        .clone()
        .oneshot(put_boot_source_request(
            kernel.path().to_str().unwrap(),
            None,
        ))
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
                .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

    let serial_response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/serial")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"serial_out_path":"serial.log"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(serial_response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn allows_logger_and_metrics_after_start() {
    let router = test_state().router();
    let kernel = NamedTempFile::new().unwrap();
    let tempdir = tempfile::tempdir().unwrap();
    let metrics_path = tempdir.path().join("metrics-after-start.jsonl");

    let boot_source_response = router
        .clone()
        .oneshot(put_boot_source_request(
            kernel.path().to_str().unwrap(),
            None,
        ))
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
                .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

    for (path, body) in [
        ("/logger", r#"{"level":"info"}"#.to_string()),
        (
            "/metrics",
            format!(r#"{{"metrics_path":"{}"}}"#, metrics_path.display()),
        ),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(path)
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }
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
async fn rejects_vm_pause_before_start() {
    let router = test_state().router();
    let response = router
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/vm")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(r#"{"state":"Paused"}"#))
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
async fn pauses_and_resumes_vm_state() {
    let router = test_state().router();
    let kernel = NamedTempFile::new().unwrap();

    let boot_source_response = router
        .clone()
        .oneshot(put_boot_source_request(
            kernel.path().to_str().unwrap(),
            None,
        ))
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
                .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

    let pause_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/vm")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(r#"{"state":"Paused"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(pause_response.status(), StatusCode::NO_CONTENT);

    let paused_state = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(paused_state.status(), StatusCode::OK);
    let paused_body = to_bytes(paused_state.into_body(), usize::MAX)
        .await
        .unwrap();
    let paused_payload: serde_json::Value = serde_json::from_slice(&paused_body).unwrap();
    assert_eq!(paused_payload["state"], "Paused");

    let resume_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/vm")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(r#"{"state":"Resumed"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resume_response.status(), StatusCode::NO_CONTENT);

    let resumed_state = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resumed_state.status(), StatusCode::OK);
    let resumed_body = to_bytes(resumed_state.into_body(), usize::MAX)
        .await
        .unwrap();
    let resumed_payload: serde_json::Value = serde_json::from_slice(&resumed_body).unwrap();
    assert_eq!(resumed_payload["state"], "Running");
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

#[tokio::test]
async fn snapshot_load_rejects_memory_hotplug_configuration() {
    let router = test_state().router();

    let put_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/hotplug/memory")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(r#"{"total_size_mib":1024}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_response.status(), StatusCode::NO_CONTENT);

    let response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/snapshot/load")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"snapshot_path":"state.snap","mem_file_path":"state.mem"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload.code, "InvalidState");
    assert_eq!(payload.fault_message, SNAPSHOT_LOAD_RESOURCE_GUARD_MESSAGE);
}

#[tokio::test]
async fn snapshot_load_rejects_cpu_config_configuration() {
    let router = test_state().router();

    let put_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/cpu-config")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"kvm_capabilities":["1","!2"],"cpuid_modifiers":[{"leaf":"0x1","subleaf":"0x0","flags":0,"modifiers":[{"register":"eax","bitmap":"0bxxxxxxxxxxxxxxxxxxxxxxxxxxxx0001"}]}],"msr_modifiers":[{"addr":"0x10a","bitmap":"0b0000000000000000000000000000000000000000000000000000000000000001"}],"reg_modifiers":[{"addr":"0x0","bitmap":"0b1"}],"vcpu_features":[{"index":0,"bitmap":"0b00000000000000000000000001100000"}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
    assert_eq!(put_response.status(), StatusCode::NO_CONTENT);

    let response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/snapshot/load")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"snapshot_path":"state.snap","mem_file_path":"state.mem"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload.code, "InvalidState");
    assert_eq!(payload.fault_message, SNAPSHOT_LOAD_RESOURCE_GUARD_MESSAGE);
}

#[tokio::test]
async fn snapshot_load_rejects_entropy_configuration() {
    let router = test_state().router();

    let put_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/entropy")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"rate_limiter":{"bandwidth":{"size":4096,"refill_time":1000,"one_time_burst":512},"ops":{"size":32,"refill_time":100}}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
    assert_eq!(put_response.status(), StatusCode::NO_CONTENT);

    let response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/snapshot/load")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"snapshot_path":"state.snap","mem_file_path":"state.mem"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload.code, "InvalidState");
    assert_eq!(payload.fault_message, SNAPSHOT_LOAD_RESOURCE_GUARD_MESSAGE);
}

#[tokio::test]
async fn snapshot_load_rejects_vsock_configuration() {
    let router = test_state().router();

    let put_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/vsock")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"guest_cid":5,"uds_path":"/tmp/vesper-vsock.sock","vsock_id":"deprecated"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
    assert_eq!(put_response.status(), StatusCode::NO_CONTENT);

    let response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/snapshot/load")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"snapshot_path":"state.snap","mem_file_path":"state.mem"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload.code, "InvalidState");
    assert_eq!(payload.fault_message, SNAPSHOT_LOAD_RESOURCE_GUARD_MESSAGE);
}

#[tokio::test]
async fn snapshot_load_rejects_balloon_configuration() {
    let router = test_state().router();

    let put_balloon_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/balloon")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"amount_mib":64,"deflate_on_oom":true}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_balloon_response.status(), StatusCode::NO_CONTENT);

    let response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/snapshot/load")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"snapshot_path":"state.snap","mem_file_path":"state.mem"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload.code, "InvalidState");
    assert_eq!(payload.fault_message, SNAPSHOT_LOAD_RESOURCE_GUARD_MESSAGE);
}

#[tokio::test]
async fn snapshot_load_rejects_mmds_configuration() {
    let router = test_state().router();

    let put_network_interface_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/network-interfaces/eth0")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"iface_id":"eth0","host_dev_name":"tap0"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        put_network_interface_response.status(),
        StatusCode::NO_CONTENT
    );

    let put_mmds_config_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/mmds/config")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(r#"{"network_interfaces":["eth0"]}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_mmds_config_response.status(), StatusCode::NO_CONTENT);

    let response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/snapshot/load")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"snapshot_path":"state.snap","mem_file_path":"state.mem"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload.code, "InvalidState");
    assert_eq!(payload.fault_message, SNAPSHOT_LOAD_RESOURCE_GUARD_MESSAGE);
}
