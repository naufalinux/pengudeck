use axum::{
    body::{to_bytes, Body},
    handler::Handler,
    http::{Request, StatusCode},
    routing::any_service,
};
use hermes_dash_api::cleanup;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
const ORIGIN: &str = "http://127.0.0.1:8081";
struct Home(PathBuf);
impl Home {
    fn new(with_board: bool) -> Self {
        let path = std::env::temp_dir().join(format!(
            "pengudeck-cleanup-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        if with_board {
            let status = Command::new("python3")
                .arg("-c")
                .arg("import sqlite3,sys; d=sqlite3.connect(sys.argv[1]+'/kanban.db'); d.executescript('CREATE TABLE tasks(id TEXT PRIMARY KEY,status TEXT,completed_at INTEGER);CREATE TABLE task_events(task_id TEXT,kind TEXT,created_at INTEGER);CREATE TABLE task_comments(task_id TEXT,body TEXT);CREATE TABLE task_links(parent_id TEXT,child_id TEXT);'); d.execute(\"INSERT INTO tasks VALUES ('t_1','done',1)\"); d.commit()")
                .arg(&path)
                .status()
                .unwrap();
            assert!(status.success());
        }
        Self(path)
    }
    fn entries(&self) -> usize {
        std::fs::read_dir(&self.0).unwrap().count()
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
async fn request(
    home: &Home,
    method: &str,
    path: &str,
    origin: Option<&str>,
    marker: bool,
    body: Value,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "127.0.0.1:8788")
        .header("content-type", "application/json");
    if let Some(origin) = origin {
        builder = builder.header("origin", origin);
    }
    if marker {
        builder = builder.header("x-pengudeck-request", "retention-v1");
    }
    let response = any_service(cleanup::router(home.0.clone()))
        .call(builder.body(Body::from(body.to_string())).unwrap(), ())
        .await;
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
async fn trusted(home: &Home, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
    request(home, method, path, Some(ORIGIN), true, body).await
}
fn policy(enabled: bool) -> Value {
    json!({"enabled":enabled,"archive_days":30,"purge_days":180,"boards":["default"]})
}

#[tokio::test]
async fn status_is_read_only_and_defaults_to_disabled() {
    let home = Home::new(true);
    let before = home.entries();
    let (status, body) = request(
        &home,
        "GET",
        "/api/settings/cleanup",
        None,
        false,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["policy"], policy(false));
    assert_eq!(
        body["summary"],
        json!({"archive":0,"purge":0,"untracked":["default"]})
    );
    assert_eq!(home.entries(), before);
}
#[tokio::test]
async fn writes_reject_missing_or_untrusted_origin_and_marker() {
    let home = Home::new(true);
    let before = home.entries();
    for (origin, marker) in [
        (None, true),
        (Some("https://evil.invalid"), true),
        (Some("null"), true),
        (Some(ORIGIN), false),
    ] {
        for (method, path) in [
            ("PUT", "/api/settings/cleanup"),
            ("POST", "/api/settings/cleanup/review"),
            ("POST", "/api/settings/cleanup/run"),
        ] {
            let body = json!({"policy":policy(true),"confirmed":true});
            let (status, _) = request(&home, method, path, origin, marker, body).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path} {origin:?}");
        }
    }
    assert_eq!(home.entries(), before);
}
#[tokio::test]
async fn preflight_allows_only_fixed_origin_and_host() {
    let home = Home::new(false);
    for (origin, status) in [
        (Some(ORIGIN), StatusCode::NO_CONTENT),
        (Some("https://evil.invalid"), StatusCode::FORBIDDEN),
        (None, StatusCode::FORBIDDEN),
    ] {
        let (code, _) = request(
            &home,
            "OPTIONS",
            "/api/settings/cleanup",
            origin,
            false,
            Value::Null,
        )
        .await;
        assert_eq!(code, status);
    }
    let response = any_service(cleanup::router(home.0.clone()))
        .call(
            Request::builder()
                .uri("/api/settings/cleanup")
                .header("host", "evil.invalid:8788")
                .body(Body::empty())
                .unwrap(),
            (),
        )
        .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}
#[tokio::test]
async fn enabling_without_explicit_confirmation_changes_nothing() {
    let home = Home::new(true);
    let before = home.entries();
    for body in [
        json!({"policy":policy(true)}),
        json!({"policy":policy(true),"confirmed":"true"}),
    ] {
        let (status, body) = trusted(&home, "PUT", "/api/settings/cleanup", body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body, json!({"error":"confirmation_required"}));
    }
    assert_eq!(home.entries(), before);
}
#[tokio::test]
async fn invalid_input_returns_only_a_fixed_error_code() {
    let home = Home::new(true);
    let (status, body) = trusted(
        &home,
        "PUT",
        "/api/settings/cleanup",
        json!({"policy":{"secret":"SENTINEL"}}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body, json!({"error":"invalid_policy"}));
    assert!(!body.to_string().contains("SENTINEL"));
    let (status, body) = trusted(
        &home,
        "POST",
        "/api/settings/cleanup/run",
        json!({"confirmed":true,"fingerprint":"nope"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body, json!({"error":"stale_preview"}));
}
#[tokio::test]
async fn one_confirmed_save_tracks_and_enables_and_review_is_read_only() {
    let home = Home::new(true);
    let (status, body) = trusted(
        &home,
        "POST",
        "/api/settings/cleanup/review",
        json!({"policy":policy(true)}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["summary"]["untracked"], json!(["default"]));
    assert!(!home.0.join("kanban/retention").exists());
    let (status, body) = trusted(
        &home,
        "PUT",
        "/api/settings/cleanup",
        json!({"policy":policy(true),"confirmed":true}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["policy"], policy(true));
    let (_, status_body) = request(
        &home,
        "GET",
        "/api/settings/cleanup",
        None,
        false,
        Value::Null,
    )
    .await;
    assert_eq!(status_body["policy"], policy(true));
    assert_eq!(status_body["summary"]["untracked"], json!([]));
    assert!(
        std::fs::read_dir(home.0.join("kanban/retention/backups"))
            .unwrap()
            .count()
            == 1
    );
}
