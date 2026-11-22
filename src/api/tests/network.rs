//! Network interfaces.

use super::*;

#[tokio::test]
async fn network_interface_patch_stub_validates_path_body_id_match() {
    let router = test_state().router();
    let kernel = NamedTempFile::new().unwrap();

    let create_response = router
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

    let mismatch_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/network-interfaces/eth0")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"iface_id":"eth1","rx_rate_limiter":{"bandwidth":{"size":1024,"refill_time":1000}}}"#,
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
        network_id_path_mismatch_message("eth0", "eth1")
    );

    let missing_interface_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/network-interfaces/eth9")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"iface_id":"eth9","rx_rate_limiter":{"bandwidth":{"size":1024,"refill_time":1000}}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
    assert_eq!(missing_interface_response.status(), StatusCode::BAD_REQUEST);
    let missing_interface_body = to_bytes(missing_interface_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let missing_interface_payload: ErrorEnvelope =
        serde_json::from_slice(&missing_interface_body).unwrap();
    assert_eq!(missing_interface_payload.code, "InvalidRequest");
    assert_eq!(
        missing_interface_payload.fault_message,
        NETWORK_UPDATE_NOT_FOUND_MESSAGE
    );

    let valid_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/network-interfaces/eth0")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"iface_id":"eth0","rx_rate_limiter":{"bandwidth":{"size":1024,"refill_time":1000}}}"#,
                    ))
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
    assert_eq!(
        vm_config_payload["network-interfaces"][0]["iface_id"],
        "eth0"
    );
    assert_eq!(
        vm_config_payload["network-interfaces"][0]["rx_rate_limiter"]["bandwidth"]["size"],
        1024
    );
}

#[tokio::test]
async fn network_put_validates_mtu_bounds() {
    let router = test_state().router();

    let (status, payload) = send_json(
        &router,
        "PUT",
        "/network-interfaces/eth0",
        r#"{"iface_id":"eth0","host_dev_name":"tap0","mtu":67}"#,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(payload["code"], "InvalidRequest");
    assert_eq!(
        payload["fault_message"],
        "Network config error: Could not create the network device: MTU 67 is out of range [68, 65535]"
    );

    let (status, payload) = send_json(
        &router,
        "PUT",
        "/network-interfaces/eth0",
        r#"{"iface_id":"eth0","host_dev_name":"tap0","mtu":65536}"#,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(payload["fault_message"], "invalid JSON payload");
    assert!(get_json(&router, "/vm/config").await["network-interfaces"].as_array().is_some_and(Vec::is_empty));

    for (iface, tap, mtu) in [("eth0", "tap0", 68), ("eth1", "tap1", 65535)] {
        let (status, _) = send_json(
            &router,
            "PUT",
            &format!("/network-interfaces/{iface}"),
            &format!(r#"{{"iface_id":"{iface}","host_dev_name":"{tap}","mtu":{mtu}}}"#),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }
    let (status, _) = send_json(
        &router,
        "PUT",
        "/network-interfaces/eth2",
        r#"{"iface_id":"eth2","host_dev_name":"tap2"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let interfaces = get_json(&router, "/vm/config").await["network-interfaces"].clone();
    assert_eq!(interfaces[0]["mtu"], 68);
    assert_eq!(interfaces[1]["mtu"], 65535);
    assert!(interfaces[2]["mtu"].is_null());
}
