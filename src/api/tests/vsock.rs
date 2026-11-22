//! The vsock device.

use super::*;

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
async fn accepts_low_vsock_guest_cid_as_firecracker_does() {
    // Firecracker v1.16.0 checks no minimum guest CID (D-026, Backend A replay).
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
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
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
