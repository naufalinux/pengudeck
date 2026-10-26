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
