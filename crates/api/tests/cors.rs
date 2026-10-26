use axum::{
    body::Body,
    handler::Handler,
    http::{header, Request, StatusCode},
    response::Response,
    routing::any_service,
};
use hermes_dash_api::{boards::BoardSource, router};
use hermes_dash_core::{
    model::Task,
    source::{SourceError, TaskSource},
};
use std::sync::Arc;

const ORIGIN: &str = "http://127.0.0.1:8080";

struct Source;
impl BoardSource for Source {
    fn boards(&self) -> Result<Vec<String>, SourceError> {
        Ok(vec!["default".into()])
    }
}
impl TaskSource for Source {
    fn list(&self, _: &str) -> Result<Vec<Task>, SourceError> {
        Err(SourceError::Unavailable)
    }
}
async fn request(method: &str, path: &str, origins: &[&str]) -> Response {
    let source = Arc::new(Source);
    let mut request = Request::builder().method(method).uri(path);
    for origin in origins {
        request = request.header(header::ORIGIN, *origin);
    }
    any_service(router(source.clone(), source))
        .call(request.body(Body::empty()).unwrap(), ())
        .await
}

fn assert_no_extra_cors(response: &Response) {
    for name in response.headers().keys() {
        assert!(
            !name.as_str().starts_with("access-control-")
                || name == header::ACCESS_CONTROL_ALLOW_ORIGIN,
            "unexpected header: {name}"
        );
    }
}

#[tokio::test]
async fn negative_origins_never_receive_allow_origin() {
    for origins in [
        vec![],
        vec!["http://localhost:8080"],
        vec!["http://127.0.0.1:8081"],
        vec!["https://127.0.0.1:8080"],
        vec!["null"],
        vec!["https://example.com"],
        vec!["http://127.0.0.1:8080.evil"],
        vec!["evilhttp://127.0.0.1:8080"],
        vec!["http://127.0.0.1:8080/"],
        vec![""],
        vec!["malformed"],
        vec!["http://127.0.0.1:8080, http://127.0.0.1:8080"],
        vec!["http://127.0.0.1:8080 http://example.com"],
        vec![ORIGIN, ORIGIN],
        vec![ORIGIN, "null"],
        vec!["null", ORIGIN],
    ] {
        let response = request("GET", "/api/health", &origins).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            !response
                .headers()
                .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN),
            "{origins:?}"
        );
        assert_eq!(response.headers()[header::VARY], "Origin");
        assert_no_extra_cors(&response);
    }
}

#[tokio::test]
async fn cors_covers_api_success_errors_and_fallback() {
    for (path, status) in [
        ("/api/health", StatusCode::OK),
        ("/api/boards", StatusCode::OK),
        ("/api/boards/bad.slug/tasks", StatusCode::BAD_REQUEST),
        ("/api/boards/absent/tasks", StatusCode::NOT_FOUND),
        ("/api/boards/default/tasks", StatusCode::BAD_GATEWAY),
        ("/api/unknown", StatusCode::NOT_FOUND),
        ("/api", StatusCode::NOT_FOUND),
        ("/api/", StatusCode::NOT_FOUND),
        ("/api/health?x=1", StatusCode::OK),
    ] {
        let response = request("GET", path, &[ORIGIN]).await;
        assert_eq!(response.status(), status, "{path}");
        assert_eq!(
            response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
            ORIGIN
        );
        assert_eq!(response.headers()[header::VARY], "Origin");
        assert_no_extra_cors(&response);
        if status == StatusCode::BAD_GATEWAY {
            let body = axum::body::to_bytes(response.into_body(), 1024)
                .await
                .unwrap();
            assert_eq!(body.as_ref(), br#"{"error":"source_unavailable"}"#);
        }
    }
}

#[tokio::test]
async fn non_get_methods_keep_router_behavior_without_cors_or_preflight() {
    for path in [
        "/api/health",
        "/api/boards",
        "/api/boards/default/tasks",
        "/api/unknown",
    ] {
        for method in ["HEAD", "POST", "OPTIONS", "PUT", "DELETE", "PATCH"] {
            let response = request(method, path, &[ORIGIN]).await;
            let expected = if path == "/api/unknown" {
                StatusCode::NOT_FOUND
            } else if method == "HEAD" {
                if path.ends_with("/tasks") {
                    StatusCode::BAD_GATEWAY
                } else {
                    StatusCode::OK
                }
            } else {
                StatusCode::METHOD_NOT_ALLOWED
            };
            assert_eq!(response.status(), expected, "{method} {path}");
            assert!(!response.headers().contains_key(header::VARY));
            assert!(!response
                .headers()
                .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN));
            assert_no_extra_cors(&response);
        }
    }
}

#[tokio::test]
async fn non_api_paths_receive_no_cors_headers() {
    for path in ["/", "/apix", "/api-other", "/API/health"] {
        let response = request("GET", path, &[ORIGIN]).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert!(!response.headers().contains_key(header::VARY));
        assert!(!response
            .headers()
            .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN));
    }
}

#[tokio::test]
async fn exact_ui_origin_can_read_health() {
    let response = request("GET", "/api/health", &[ORIGIN]).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
        ORIGIN
    );
    assert_eq!(response.headers()[header::VARY], "Origin");
}
