//! Errors outside the handlers: the body limit, a panic (TD-019) and a blocking host path (TD-024).

use super::*;

// TD-019: the body limit, a body that cannot be read and a handler panic all answer with an error envelope.
async fn envelope_of(response: axum::response::Response) -> (StatusCode, serde_json::Value) {
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).expect("an error envelope"),
    )
}

#[tokio::test]
async fn an_oversized_body_gets_an_error_envelope() {
    let big = vec![b'x'; 2 * 1024 * 1024 + 1];
    let declared = Request::builder()
        .method("PUT")
        .uri("/mmds")
        .header("content-type", "application/json")
        .header("content-length", big.len().to_string())
        .body(axum::body::Body::from(big.clone()))
        .unwrap();
    let (status, body) = envelope_of(test_state().router().oneshot(declared).await.unwrap()).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(body["code"], "PayloadTooLarge");
    assert!(body["baseline_firecracker_revision"].is_string());
    // No Content-Length header: the limit is met while the body is read.
    let no_header = Request::builder()
        .method("PUT")
        .uri("/mmds")
        .body(axum::body::Body::from(big))
        .unwrap();
    let (status, body) = envelope_of(test_state().router().oneshot(no_header).await.unwrap()).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(body["path"], "/mmds");
}

#[tokio::test]
async fn a_poisoned_lock_gets_a_500_envelope() {
    let state = test_state();
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = state.machine_config.write().unwrap();
        panic!("poison the machine config lock");
    }));
    let request = Request::builder()
        .method("GET")
        .uri("/machine-config")
        .body(axum::body::Body::empty())
        .unwrap();
    let (status, body) = envelope_of(state.router().oneshot(request).await.unwrap()).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["code"], "InternalError");
    assert_eq!(body["method"], "GET");
}

// TD-024: a caller-given path that blocks (a FIFO with no writer) does not stall other requests.
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn a_blocking_host_path_does_not_stall_other_requests() {
    let dir = tempfile::tempdir().unwrap();
    let fifo = dir.path().join("kernel.fifo");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let router = test_state().router();
    let body = serde_json::json!({ "kernel_image_path": fifo.to_str().unwrap() }).to_string();
    let put = Request::builder()
        .method("PUT")
        .uri("/boot-source")
        .header("content-type", "application/json")
        .body(axum::body::Body::from(body))
        .unwrap();
    let blocked = tokio::spawn(router.clone().oneshot(put));
    std::thread::sleep(std::time::Duration::from_millis(200)); // not tokio's timer, which a blocked worker stops
    let get = Request::builder()
        .method("GET")
        .uri("/")
        .body(axum::body::Body::empty())
        .unwrap();
    // Spawned, so it runs on the runtime's one worker, as a client's connection does. The wait is a std channel on a
    // blocking thread: a blocked worker would also stop tokio's timers.
    let (tx, rx) = std::sync::mpsc::channel();
    let r = router.clone();
    tokio::spawn(async move {
        let _ = tx.send(r.oneshot(get).await.map(|resp| resp.status()));
    });
    let got =
        tokio::task::spawn_blocking(move || rx.recv_timeout(std::time::Duration::from_secs(5)))
            .await
            .unwrap();
    // Release the blocked open first, so a failure below leaves no worker blocked.
    let _w = std::fs::OpenOptions::new().write(true).open(&fifo).unwrap();
    let _ = tokio::task::spawn_blocking(move || drop(blocked)).await;
    assert!(
        matches!(got, Ok(Ok(StatusCode::OK))),
        "GET / stalled behind an open of a FIFO: {got:?}"
    );
}
