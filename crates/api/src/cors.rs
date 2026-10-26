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
        let varies_on_origin = response
            .headers()
            .get_all(header::VARY)
            .iter()
            .any(|value| {
                value.to_str().is_ok_and(|value| {
                    value.split(',').any(|token| {
                        let token = token.trim();
                        token == "*" || token.eq_ignore_ascii_case("origin")
                    })
                })
            });
        if !varies_on_origin {
            response
                .headers_mut()
                .append(header::VARY, HeaderValue::from_static("Origin"));
        }
        if allowed {
            response.headers_mut().insert(
                header::ACCESS_CONTROL_ALLOW_ORIGIN,
                HeaderValue::from_static(UI_ORIGIN),
            );
        }
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        handler::Handler,
        http::Request,
        routing::{any_service, get},
        Router,
    };

    #[tokio::test]
    async fn vary_merge_preserves_existing_values_without_duplicate_origin() {
        for values in [
            vec!["Accept-Encoding", "Accept-Language"],
            vec!["Accept-Encoding, oRiGiN"],
            vec!["*"],
        ] {
            let original = values.clone();
            let app = Router::new()
                .route(
                    "/api/test",
                    get(move || async move {
                        let mut response = Response::new(Body::empty());
                        for value in values {
                            response
                                .headers_mut()
                                .append(header::VARY, HeaderValue::from_static(value));
                        }
                        response
                    }),
                )
                .layer(axum::middleware::from_fn(local_ui));
            let response = any_service(app)
                .call(
                    Request::builder()
                        .uri("/api/test")
                        .body(Body::empty())
                        .unwrap(),
                    (),
                )
                .await;
            let values: Vec<_> = response
                .headers()
                .get_all(header::VARY)
                .iter()
                .map(|value| value.to_str().unwrap())
                .collect();
            let expected = if original == ["Accept-Encoding", "Accept-Language"] {
                vec!["Accept-Encoding", "Accept-Language", "Origin"]
            } else {
                original
            };
            assert_eq!(values, expected);
        }
    }
}
