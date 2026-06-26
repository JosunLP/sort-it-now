//! HTTP integration tests that exercise the full Axum router and handler stack.
//!
//! These tests drive [`sort_it_now::api::build_router`] in-process via `tower`'s `oneshot`,
//! avoiding any TCP binding while still covering routing, extraction, validation, and
//! serialization end to end.

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use sort_it_now::api::build_router;
use sort_it_now::config::{OptimizerConfig, RequestLimits};
use tower::ServiceExt; // for `oneshot`

/// Builds a router with default optimizer configuration and the given request limits.
fn router_with_limits(limits: RequestLimits) -> Router {
    build_router(OptimizerConfig::default(), limits)
}

/// Builds a router with default optimizer configuration and default request limits.
fn router() -> Router {
    router_with_limits(RequestLimits::default())
}

/// Sends a GET request and returns the status plus parsed JSON body.
async fn get_json(app: Router, uri: &str) -> (StatusCode, Value) {
    let response = app
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body collected");
    let value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    (status, value)
}

/// Sends a POST request with a raw body and returns the status plus parsed JSON body.
async fn post_json(app: Router, uri: &str, body: String) -> (StatusCode, Value) {
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body collected");
    let value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    (status, value)
}

#[tokio::test]
async fn health_endpoint_returns_ok() {
    let (status, body) = get_json(router(), "/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn version_endpoint_reports_build_metadata() {
    let (status, body) = get_json(router(), "/version").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], env!("CARGO_PKG_NAME"));
    assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
}

#[tokio::test]
async fn config_endpoint_exposes_defaults() {
    let (status, body) = get_json(router(), "/config").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["grid_step"].as_f64().unwrap() > 0.0);
    assert_eq!(
        body["max_objects"].as_u64().unwrap(),
        RequestLimits::DEFAULT_MAX_OBJECTS as u64
    );
    assert_eq!(body["allow_item_rotation"], false);
}

#[tokio::test]
async fn pack_endpoint_returns_utilization_metrics() {
    let payload = json!({
        "containers": [{"name": "Box", "dims": [10.0, 10.0, 10.0], "max_weight": 100.0}],
        "objects": [{"id": 1, "dims": [10.0, 10.0, 5.0], "weight": 40.0}]
    })
    .to_string();

    let (status, body) = post_json(router(), "/pack", payload).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["is_complete"], true);
    assert_eq!(body["results"].as_array().unwrap().len(), 1);

    let diagnostics = &body["results"][0]["diagnostics"];
    assert!((diagnostics["volume_utilization_percent"].as_f64().unwrap() - 50.0).abs() < 1e-6);
    assert!((diagnostics["weight_utilization_percent"].as_f64().unwrap() - 40.0).abs() < 1e-6);

    let summary = &body["diagnostics_summary"];
    assert!(
        (summary["average_volume_utilization_percent"]
            .as_f64()
            .unwrap()
            - 50.0)
            .abs()
            < 1e-6
    );
}

#[tokio::test]
async fn pack_endpoint_rejects_invalid_json() {
    let (status, body) = post_json(router(), "/pack", "not-json".to_string()).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["error"].is_string());
}

#[tokio::test]
async fn pack_endpoint_rejects_missing_containers() {
    let payload = json!({ "containers": [], "objects": [] }).to_string();
    let (status, body) = post_json(router(), "/pack", payload).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["details"].as_str().unwrap().contains("packaging type"));
}

#[tokio::test]
async fn pack_endpoint_enforces_object_limit() {
    let payload = json!({
        "containers": [{"dims": [10.0, 10.0, 10.0], "max_weight": 100.0}],
        "objects": [
            {"id": 1, "dims": [5.0, 5.0, 5.0], "weight": 1.0},
            {"id": 2, "dims": [5.0, 5.0, 5.0], "weight": 1.0}
        ]
    })
    .to_string();

    let app = router_with_limits(RequestLimits::with_limits(1, 10));
    let (status, body) = post_json(app, "/pack", payload).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        body["details"]
            .as_str()
            .unwrap()
            .contains("Too many objects")
    );
}

#[tokio::test]
async fn pack_endpoint_reports_unplaced_oversized_object() {
    let payload = json!({
        "containers": [{"dims": [10.0, 10.0, 10.0], "max_weight": 100.0}],
        "objects": [{"id": 1, "dims": [20.0, 20.0, 20.0], "weight": 1.0}]
    })
    .to_string();

    let (status, body) = post_json(router(), "/pack", payload).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["is_complete"], false);
    assert_eq!(body["unplaced"].as_array().unwrap().len(), 1);
    assert_eq!(
        body["unplaced"][0]["reason_code"],
        "dimensions_exceed_container"
    );
}

#[tokio::test]
async fn unknown_asset_returns_not_found() {
    let response = router()
        .oneshot(
            Request::builder()
                .uri("/does-not-exist.bin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn index_is_served() {
    let response = router()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let html = String::from_utf8_lossy(&body);
    assert!(html.contains("<!DOCTYPE html>"));
}
