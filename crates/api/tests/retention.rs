use axum::{
    body::{to_bytes, Body},
    handler::Handler,
    http::{Request, StatusCode},
    routing::any_service,
};
use hermes_dash_api::retention;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Home(PathBuf);
impl Home {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "pengudeck-retention-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
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
    let response = any_service(retention::router(home.0.clone()))
        .call(builder.body(Body::from(body.to_string())).unwrap(), ())
        .await;
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
#[tokio::test]
async fn settings_default_is_disabled_and_read_has_no_filesystem_writes() {
    let home = Home::new();
    let (status, body) = request(
        &home,
        "GET",
        "/api/settings/retention",
        None,
        false,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["policy"],
        json!({"enabled":false,"archive_days":30,"purge_days":180,"boards":["default"]})
    );
    assert_eq!(std::fs::read_dir(&home.0).unwrap().count(), 0);
}
#[tokio::test]
async fn settings_writes_reject_missing_or_untrusted_origin_and_marker() {
    let home = Home::new();
    for (origin, marker) in [
        (None, true),
        (Some("https://evil.invalid"), true),
        (Some("null"), true),
        (Some("http://127.0.0.1:8081"), false),
    ] {
        for path in [
            "/api/settings/retention/preview",
            "/api/settings/retention/track",
            "/api/settings/retention/apply",
        ] {
            let (status, _) = request(&home, "POST", path, origin, marker, json!({})).await;
            assert_eq!(status, StatusCode::FORBIDDEN);
        }
        assert_eq!(
            request(
                &home,
                "PUT",
                "/api/settings/retention",
                origin,
                marker,
                json!({})
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(std::fs::read_dir(&home.0).unwrap().count(), 0);
}
#[tokio::test]
async fn retention_preflight_allows_only_fixed_origin_and_host() {
    let home = Home::new();
    for (origin, status) in [
        (Some("http://127.0.0.1:8081"), StatusCode::NO_CONTENT),
        (Some("https://evil.invalid"), StatusCode::FORBIDDEN),
        (None, StatusCode::FORBIDDEN),
    ] {
        assert_eq!(
            request(
                &home,
                "OPTIONS",
                "/api/settings/retention/track",
                origin,
                false,
                Value::Null
            )
            .await
            .0,
            status
        );
    }
    let response = any_service(retention::router(home.0.clone()))
        .call(
            Request::builder()
                .uri("/api/settings/retention")
                .header("host", "evil.invalid:8788")
                .body(Body::empty())
                .unwrap(),
            (),
        )
        .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(std::fs::read_dir(&home.0).unwrap().count(), 0);
}

#[tokio::test]
async fn accepted_origin_invalid_policy_is_sanitized() {
    let home = Home::new();
    let (status, body) = request(
        &home,
        "PUT",
        "/api/settings/retention",
        Some("http://127.0.0.1:8081"),
        true,
        json!({"policy":{"secret":"SENTINEL"}}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(!body.to_string().contains("SENTINEL"));
}
