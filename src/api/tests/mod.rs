//! Handler tests for `src/api.rs`, one file for each route family (TD-015). The shared helpers are here.

use super::*;
use axum::body::to_bytes;
use axum::http::Request;
use tempfile::NamedTempFile;
use tower::ServiceExt;

fn test_state() -> AppState {
    AppState::new(
        &[
            RouteSpec {
                method: "PUT".to_string(),
                path: "/actions".to_string(),
            },
            RouteSpec {
                method: "PUT".to_string(),
                path: "/boot-source".to_string(),
            },
            RouteSpec {
                method: "PUT".to_string(),
                path: "/balloon".to_string(),
            },
            RouteSpec {
                method: "PATCH".to_string(),
                path: "/balloon".to_string(),
            },
            RouteSpec {
                method: "GET".to_string(),
                path: "/balloon/statistics".to_string(),
            },
            RouteSpec {
                method: "PATCH".to_string(),
                path: "/balloon/statistics".to_string(),
            },
            RouteSpec {
                method: "GET".to_string(),
                path: "/balloon/hinting/status".to_string(),
            },
            RouteSpec {
                method: "PATCH".to_string(),
                path: "/balloon/hinting/start".to_string(),
            },
            RouteSpec {
                method: "PATCH".to_string(),
                path: "/balloon/hinting/stop".to_string(),
            },
            RouteSpec {
                method: "GET".to_string(),
                path: "/".to_string(),
            },
            RouteSpec {
                method: "GET".to_string(),
                path: "/balloon".to_string(),
            },
            RouteSpec {
                method: "GET".to_string(),
                path: "/version".to_string(),
            },
            RouteSpec {
                method: "GET".to_string(),
                path: "/machine-config".to_string(),
            },
            RouteSpec {
                method: "PUT".to_string(),
                path: "/cpu-config".to_string(),
            },
            RouteSpec {
                method: "PUT".to_string(),
                path: "/logger".to_string(),
            },
            RouteSpec {
                method: "PUT".to_string(),
                path: "/machine-config".to_string(),
            },
            RouteSpec {
                method: "PUT".to_string(),
                path: "/metrics".to_string(),
            },
            RouteSpec {
                method: "GET".to_string(),
                path: "/mmds".to_string(),
            },
            RouteSpec {
                method: "PATCH".to_string(),
                path: "/mmds".to_string(),
            },
            RouteSpec {
                method: "PUT".to_string(),
                path: "/mmds".to_string(),
            },
            RouteSpec {
                method: "PUT".to_string(),
                path: "/mmds/config".to_string(),
            },
            RouteSpec {
                method: "PUT".to_string(),
                path: "/serial".to_string(),
            },
            RouteSpec {
                method: "PUT".to_string(),
                path: "/entropy".to_string(),
            },
            RouteSpec {
                method: "PUT".to_string(),
                path: "/vsock".to_string(),
            },
            RouteSpec {
                method: "PATCH".to_string(),
                path: "/machine-config".to_string(),
            },
            RouteSpec {
                method: "PUT".to_string(),
                path: "/drives/{drive_id}".to_string(),
            },
            RouteSpec {
                method: "PATCH".to_string(),
                path: "/drives/{drive_id}".to_string(),
            },
            RouteSpec {
                method: "PUT".to_string(),
                path: "/network-interfaces/{iface_id}".to_string(),
            },
            RouteSpec {
                method: "PATCH".to_string(),
                path: "/network-interfaces/{iface_id}".to_string(),
            },
            RouteSpec {
                method: "GET".to_string(),
                path: "/hotplug/memory".to_string(),
            },
            RouteSpec {
                method: "PATCH".to_string(),
                path: "/hotplug/memory".to_string(),
            },
            RouteSpec {
                method: "PUT".to_string(),
                path: "/hotplug/memory".to_string(),
            },
            RouteSpec {
                method: "PUT".to_string(),
                path: "/pmem/{id}".to_string(),
            },
            RouteSpec {
                method: "PATCH".to_string(),
                path: "/pmem/{id}".to_string(),
            },
            RouteSpec {
                method: "PUT".to_string(),
                path: "/snapshot/create".to_string(),
            },
            RouteSpec {
                method: "PUT".to_string(),
                path: "/snapshot/load".to_string(),
            },
            RouteSpec {
                method: "GET".to_string(),
                path: "/vm/config".to_string(),
            },
            RouteSpec {
                method: "PATCH".to_string(),
                path: "/vm".to_string(),
            },
        ],
        "abc123".to_string(),
        "1.16.0-dev".to_string(),
    )
    .unwrap()
}

fn put_boot_source_request(
    kernel_path: &str,
    initrd_path: Option<&str>,
) -> Request<axum::body::Body> {
    let body = match initrd_path {
        Some(initrd_path) => {
            format!(r#"{{"kernel_image_path":"{kernel_path}","initrd_path":"{initrd_path}"}}"#)
        }
        None => format!(r#"{{"kernel_image_path":"{kernel_path}"}}"#),
    };

    Request::builder()
        .method("PUT")
        .uri("/boot-source")
        .header("content-type", "application/json")
        .body(axum::body::Body::from(body))
        .unwrap()
}

async fn send_json(
    router: &Router,
    method: &str,
    uri: &str,
    body: &str,
) -> (StatusCode, serde_json::Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .body(axum::body::Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, value)
}

async fn get_json(router: &Router, uri: &str) -> serde_json::Value {
    let (status, value) = send_json(router, "GET", uri, "").await;
    assert_eq!(status, StatusCode::OK);
    value
}

async fn put_kernel(router: &Router, kernel: &NamedTempFile) {
    let response = router
        .clone()
        .oneshot(put_boot_source_request(
            kernel.path().to_str().unwrap(),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

async fn instance_start(router: &Router) -> (StatusCode, serde_json::Value) {
    send_json(
        router,
        "PUT",
        "/actions",
        r#"{"action_type":"InstanceStart"}"#,
    )
    .await
}

fn pmem_body(id: &str, path: &std::path::Path, extra: &str) -> String {
    format!(
        r#"{{"id":"{id}","path_on_host":"{}"{extra}}}"#,
        path.display()
    )
}

const RATE_LIMITER_JSON: &str = r#"{"bandwidth":{"size":1048576,"refill_time":100},"ops":{"size":10,"one_time_burst":5,"refill_time":1000}}"#;

mod balloon;
mod balloon_reporting;
mod device_config;
mod drives;
mod lifecycle;
mod memory_hotplug;
mod mmds;
mod network;
mod pmem;
mod snapshot;
mod vsock;
