use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

fn concrete_path(template: &str) -> String {
    let mut out = String::new();
    let mut chars = template.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '{' {
            while let Some(next) = chars.next() {
                if next == '}' {
                    break;
                }
            }
            out.push_str("test");
            continue;
        }
        out.push(ch);
    }
    out
}

#[tokio::test]
async fn all_pinned_routes_are_reachable() {
    let lock = vesper::baseline::load("docs/specs/design-v0.baselines.lock.yaml").unwrap();
    let firecracker = lock.baselines.get("firecracker").unwrap().revision.clone();
    let firecracker_version =
        vesper::spec::load_info_version("third_party/firecracker/firecracker.yaml").unwrap();

    let routes = vesper::spec::load_routes("third_party/firecracker/firecracker.yaml")
        .unwrap()
        .into_iter()
        .map(|route| vesper::api::RouteSpec {
            method: route.method,
            path: route.path,
        })
        .collect::<Vec<_>>();

    let router = vesper::api::AppState::new(&routes, firecracker.clone(), firecracker_version)
        .unwrap()
        .router();

    for route in routes {
        let body = if route.method == "PUT" && route.path == "/actions" {
            Body::from(r#"{"action_type":"FlushMetrics"}"#)
        } else {
            Body::empty()
        };

        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(route.method.as_str())
                    .uri(concrete_path(&route.path))
                    .body(body)
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_ne!(response.status(), StatusCode::NOT_FOUND);

        if response.status().is_success() {
            continue;
        }

        let payload: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert_eq!(
            payload["baseline_firecracker_revision"].as_str().unwrap(),
            firecracker
        );
    }
}
