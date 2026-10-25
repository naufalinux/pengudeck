pub mod boards;

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
}

async fn list_tasks(
    State(sources): State<Sources>,
    axum::extract::Path(slug): axum::extract::Path<String>,
) -> Result<Json<Value>, ApiError> {
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
