//! Instance start, boot source, machine config, actions, VM pause and resume, unknown routes.

use super::*;

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
    assert!(vm_config["drives"].as_array().is_some_and(Vec::is_empty));
    assert!(vm_config["network-interfaces"].as_array().is_some_and(Vec::is_empty));
    assert!(vm_config["pmem"].as_array().is_some_and(Vec::is_empty));
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
