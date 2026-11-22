//! pmem devices.

use super::*;

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
    let (status, _) = send_json(
        &router,
        "PUT",
        "/pmem/pmem0",
        &pmem_body("pmem0", &missing_path, ""),
    )
    .await;
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
    assert_eq!(
        payload["fault_message"],
        PMEM_BACKING_FILE_ZERO_SIZE_MESSAGE
    );
    assert_eq!(get_json(&router, "/").await["state"], "Not started");

    let (status, _) = send_json(
        &router,
        "PUT",
        "/pmem/pmem0",
        &pmem_body("pmem0", backing_file.path(), ""),
    )
    .await;
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
        &pmem_body(
            "pmem0",
            backing_file.path(),
            &format!(r#","rate_limiter":{RATE_LIMITER_JSON}"#),
        ),
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
        &pmem_body(
            "pmem1",
            backing_file.path(),
            r#","rate_limiter":{"bandwidth":{"size":1}}"#,
        ),
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
    let (status, _) = send_json(
        &router,
        "PUT",
        "/pmem/pmem0",
        &pmem_body("pmem0", backing_file.path(), ""),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let patch_body = format!(r#"{{"id":"pmem0","rate_limiter":{RATE_LIMITER_JSON}}}"#);

    let (status, payload) = send_json(&router, "PATCH", "/pmem/pmem0", &patch_body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(payload["code"], "InvalidState");
    assert_eq!(
        payload["fault_message"],
        ACTION_NOT_SUPPORTED_PREBOOT_MESSAGE
    );

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
        assert_eq!(
            payload["fault_message"], "invalid JSON payload",
            "{bad_body}"
        );
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
