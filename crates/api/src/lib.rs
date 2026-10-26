pub mod boards;
mod cors;

/// Fixed production listener; deliberately not configurable by arguments or environment.
pub const BIND_ADDR: std::net::SocketAddr = std::net::SocketAddr::V4(std::net::SocketAddrV4::new(
    std::net::Ipv4Addr::LOCALHOST,
    8787,
));

use axum::{extract::State, http::StatusCode, routing::get, Json, Router};
use boards::BoardSource;
use hermes_dash_core::source::TaskSource;
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Clone)]
struct Sources {
    boards: Arc<dyn BoardSource + Send + Sync>,
    tasks: Arc<dyn TaskSource + Send + Sync>,
}
type ApiError = (StatusCode, Json<Value>);
fn unavailable() -> ApiError {
    (
        StatusCode::BAD_GATEWAY,
        Json(json!({"error":"source_unavailable"})),
    )
}

pub fn router(
    boards: Arc<dyn BoardSource + Send + Sync>,
    tasks: Arc<dyn TaskSource + Send + Sync>,
) -> Router {
    Router::new()
        .route("/api/health", get(|| async { Json(json!({"ok": true})) }))
        .route("/api/boards", get(list_boards))
        .route("/api/boards/{slug}/tasks", get(list_tasks))
        .with_state(Sources { boards, tasks })
        // Router-wide layer also covers unknown API paths and error responses.
        .layer(axum::middleware::from_fn(cors::local_ui))
}

async fn list_tasks(
    State(sources): State<Sources>,
    path: Result<axum::extract::Path<String>, axum::extract::rejection::PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let axum::extract::Path(slug) = path.map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"invalid_board"})),
        )
    })?;
    if !boards::valid_slug(&slug) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"invalid_board"})),
        ));
    }
    let board_source = sources.boards;
    let boards = tokio::task::spawn_blocking(move || board_source.boards())
        .await
        .map_err(|_| unavailable())?
        .map_err(|_| unavailable())?;
    if !boards.contains(&slug) {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error":"board_not_found"})),
        ));
    }
    let tasks = tokio::task::spawn_blocking(move || sources.tasks.list(&slug))
        .await
        .map_err(|_| unavailable())?
        .map_err(|_| unavailable())?;
    Ok(Json(
        json!({"columns": hermes_dash_core::model::group_by_column(&tasks)}),
    ))
}

async fn list_boards(State(sources): State<Sources>) -> Result<Json<Vec<String>>, ApiError> {
    tokio::task::spawn_blocking(move || sources.boards.boards())
        .await
        .map_err(|_| unavailable())?
        .map(Json)
        .map_err(|_| unavailable())
}
