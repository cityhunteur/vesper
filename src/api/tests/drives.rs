//! Block drives.

use super::*;

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
    assert!(vm_config_payload["drives"].as_array().is_some_and(Vec::is_empty));
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
