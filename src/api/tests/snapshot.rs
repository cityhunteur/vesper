//! Snapshot create and load.

use super::*;

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
async fn snapshot_load_allowed_after_cpu_config() {
    // `set_custom_cpu_template` does not set Firecracker's `boot_path` (D-026).
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
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
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
