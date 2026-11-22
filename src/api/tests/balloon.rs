//! The balloon device: config and the runtime target.

use super::*;

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
        balloon_update_too_large_message(129)
    );
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
