//! MMDS config and data store.

use super::*;

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
