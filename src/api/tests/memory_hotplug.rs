//! Memory hotplug config and status.

use super::*;

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
    assert!(initial_vm_config_payload["memory-hotplug"].is_null());

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
        "Memory hotplug config error: Total size must be a multiple of slot size (128 MiB)"
    );
}
