//! The balloon device: statistics and free-page hinting.

use super::*;

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
