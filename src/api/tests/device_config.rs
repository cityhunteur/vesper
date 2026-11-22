//! CPU config, entropy, serial, logger and metrics.

use super::*;

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
    assert!(initial_vm_config_payload["cpu-config"].is_null());

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
    assert!(vm_config_payload["cpu-config"].is_null());
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
    assert!(vm_config_payload["logger"].is_null());
    assert!(vm_config_payload["metrics"].is_null());
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
async fn refuses_serial_after_start() {
    // Firecracker v1.16.0 refuses ConfigureSerial after boot (D-026, Backend A replay).
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
    assert_eq!(serial_response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(serial_response.into_body(), usize::MAX).await.unwrap();
    let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload.code, "InvalidState");
    assert_eq!(payload.fault_message, ACTION_NOT_SUPPORTED_POSTBOOT_MESSAGE);
}

#[tokio::test]
async fn refuses_logger_and_metrics_after_start() {
    // Firecracker v1.16.0 refuses ConfigureLogger and ConfigureMetrics after boot (D-026).
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
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload.fault_message, ACTION_NOT_SUPPORTED_POSTBOOT_MESSAGE);
    }
}
