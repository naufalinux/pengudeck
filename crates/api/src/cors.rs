use axum::{
    extract::Request,
    http::{header, HeaderValue, Method},
    middleware::Next,
    response::Response,
};

const UI_ORIGIN: &str = "http://127.0.0.1:8080";

pub(crate) async fn local_ui(request: Request, next: Next) -> Response {
    let path = request.uri().path();
    let applies = request.method() == Method::GET && (path == "/api" || path.starts_with("/api/"));
    let mut origins = request.headers().get_all(header::ORIGIN).iter();
    let allowed =
        origins.next().is_some_and(|value| value == UI_ORIGIN) && origins.next().is_none();
    let mut response = next.run(request).await;
    if applies {
        response
            .headers_mut()
            .append(header::VARY, HeaderValue::from_static("Origin"));
        if allowed {
            response.headers_mut().insert(
                header::ACCESS_CONTROL_ALLOW_ORIGIN,
                HeaderValue::from_static(UI_ORIGIN),
            );
        }
    }
    response
}
