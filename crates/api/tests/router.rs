use axum::{
    body::{to_bytes, Body},
    handler::Handler,
    http::{Request, StatusCode},
    routing::any_service,
};
use hermes_dash_api::{boards::BoardSource, router};
use hermes_dash_core::{
    model::Task,
    source::{parse_list, SourceError, TaskSource},
};
use serde_json::{json, Value};
use std::sync::Arc;

struct FakeSource {
    boards: Result<Vec<String>, SourceError>,
    tasks: Result<Vec<Task>, SourceError>,
}
impl Default for FakeSource {
    fn default() -> Self {
        Self {
            boards: Ok(vec!["default".into(), "zaha".into()]),
            tasks: Ok(
                parse_list(include_str!("../../core/tests/fixtures/list_sample.json")).unwrap(),
            ),
        }
    }
}
impl BoardSource for FakeSource {
    fn boards(&self) -> Result<Vec<String>, SourceError> {
        self.boards.clone()
    }
}
impl TaskSource for FakeSource {
    fn list(&self, board: &str) -> Result<Vec<Task>, SourceError> {
        assert_eq!(board, "default");
        self.tasks.clone()
    }
}
async fn request(source: FakeSource, method: &str, uri: &str) -> (StatusCode, Value) {
    let source = Arc::new(source);
    // Public Handler API drives the actual Router without tower or sockets.
    let response = any_service(router(source.clone(), source))
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
    let content_type = response.headers().get("content-type").cloned();
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    if status == StatusCode::METHOD_NOT_ALLOWED || body.is_empty() {
        return (status, Value::Null);
    }
    assert_eq!(content_type.unwrap(), "application/json");
    (status, serde_json::from_slice(&body).unwrap())
}
#[tokio::test]
async fn health_is_exact_liveness_json() {
    let source = FakeSource {
        boards: Err(SourceError::Unavailable),
        tasks: Err(SourceError::Unavailable),
    };
    assert_eq!(
        request(source, "GET", "/api/health").await,
        (StatusCode::OK, json!({"ok":true}))
    );
}
#[tokio::test]
async fn boards_are_source_slugs_without_wrapper() {
    assert_eq!(
        request(FakeSource::default(), "GET", "/api/boards").await,
        (StatusCode::OK, json!(["default", "zaha"]))
    );
}
#[tokio::test]
async fn board_source_failure_is_sanitized() {
    let source = FakeSource {
        boards: Err(SourceError::Unavailable),
        ..Default::default()
    };
    assert_eq!(
        request(source, "GET", "/api/boards").await,
        (
            StatusCode::BAD_GATEWAY,
            json!({"error":"source_unavailable"})
        )
    );
}
