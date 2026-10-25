pub mod boards;

use axum::{extract::State, http::StatusCode, routing::get, Json, Router};
use boards::BoardSource;
use hermes_dash_core::source::TaskSource;
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Clone)]
struct Sources {
    boards: Arc<dyn BoardSource + Send + Sync>,
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
    _tasks: Arc<dyn TaskSource + Send + Sync>,
) -> Router {
    Router::new()
        .route("/api/health", get(|| async { Json(json!({"ok": true})) }))
        .route("/api/boards", get(list_boards))
        .with_state(Sources { boards })
}

async fn list_boards(State(sources): State<Sources>) -> Result<Json<Vec<String>>, ApiError> {
    tokio::task::spawn_blocking(move || sources.boards.boards())
        .await
        .map_err(|_| unavailable())?
        .map(Json)
        .map_err(|_| unavailable())
}
