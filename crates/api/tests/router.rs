use axum::{
    body::{to_bytes, Body},
    handler::Handler,
    http::{Request, StatusCode},
    routing::any_service,
};
use hermes_dash_api::router;
use serde_json::{json, Value};

async fn request(method: &str, uri: &str) -> (StatusCode, Value) {
    // MethodRouter's public Handler API drives the real Router without tower or sockets.
    let response = any_service(router())
        .call(
            Request::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
            (),
        )
        .await;
    let status = response.status();
    assert_eq!(response.headers()["content-type"], "application/json");
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap())
}

#[tokio::test]
async fn health_is_exact_liveness_json() {
    assert_eq!(
        request("GET", "/api/health").await,
        (StatusCode::OK, json!({"ok":true}))
    );
}
