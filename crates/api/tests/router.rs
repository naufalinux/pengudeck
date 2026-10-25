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
async fn fixture_groups_into_exact_six_columns() {
    let (status, body) = request(FakeSource::default(), "GET", "/api/boards/default/tasks").await;
    assert_eq!(status, StatusCode::OK);
    let tasks = parse_list(include_str!("../../core/tests/fixtures/list_sample.json")).unwrap();
    let expected = json!({"columns":[
        {"status":"todo", "tasks":[tasks[0], tasks[2]]},
        {"status":"ready", "tasks":[]},
        {"status":"running", "tasks":[tasks[1]]},
        {"status":"blocked", "tasks":[]},
        {"status":"review", "tasks":[]},
        {"status":"done", "tasks":[tasks[3]]}
    ]});
    assert_eq!(body, expected);
    assert!(body["columns"][2]["tasks"][0]["assignee"].is_null());
    let counts: Vec<_> = body["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["tasks"].as_array().unwrap().len())
        .collect();
    assert_eq!(counts, [2, 0, 1, 0, 0, 1]);
}

#[tokio::test]
async fn invalid_slugs_are_rejected_before_sources() {
    for slug in ["bad.slug", "bad%20slug", "%2F", "%C3%A9", "%00", "--flag!"] {
        let source = FakeSource {
            boards: Err(SourceError::Unavailable),
            tasks: Err(SourceError::Unavailable),
        };
        assert_eq!(
            request(source, "GET", &format!("/api/boards/{slug}/tasks")).await,
            (StatusCode::BAD_REQUEST, json!({"error":"invalid_board"}))
        );
    }
}
#[tokio::test]
async fn unknown_valid_board_is_not_found() {
    assert_eq!(
        request(
            FakeSource::default(),
            "GET",
            "/api/boards/Absent_-123/tasks"
        )
        .await,
        (StatusCode::NOT_FOUND, json!({"error":"board_not_found"}))
    );
}
#[tokio::test]
async fn board_failure_prevents_task_lookup() {
    let source = FakeSource {
        boards: Err(SourceError::InvalidData),
        ..Default::default()
    };
    assert_eq!(
        request(source, "GET", "/api/boards/absent/tasks").await,
        (
            StatusCode::BAD_GATEWAY,
            json!({"error":"source_unavailable"})
        )
    );
}
#[tokio::test]
async fn task_source_failures_are_sanitized() {
    for error in [SourceError::Unavailable, SourceError::InvalidData] {
        let source = FakeSource {
            tasks: Err(error),
            ..Default::default()
        };
        assert_eq!(
            request(source, "GET", "/api/boards/default/tasks").await,
            (
                StatusCode::BAD_GATEWAY,
                json!({"error":"source_unavailable"})
            )
        );
    }
}
#[tokio::test]
async fn empty_board_keeps_all_six_columns() {
    let source = FakeSource {
        tasks: Ok(vec![]),
        ..Default::default()
    };
    let expected = json!({"columns":[{"status":"todo","tasks":[]},{"status":"ready","tasks":[]},{"status":"running","tasks":[]},{"status":"blocked","tasks":[]},{"status":"review","tasks":[]},{"status":"done","tasks":[]}]});
    assert_eq!(
        request(source, "GET", "/api/boards/default/tasks").await,
        (StatusCode::OK, expected)
    );
}
#[tokio::test]
async fn post_is_not_allowed_on_all_read_routes() {
    for uri in ["/api/health", "/api/boards", "/api/boards/default/tasks"] {
        assert_eq!(
            request(FakeSource::default(), "POST", uri).await.0,
            StatusCode::METHOD_NOT_ALLOWED
        );
    }
}
#[tokio::test]
async fn unknown_route_is_not_found() {
    assert_eq!(
        request(FakeSource::default(), "GET", "/api/unknown")
            .await
            .0,
        StatusCode::NOT_FOUND
    );
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
