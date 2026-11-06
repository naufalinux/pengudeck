//! Simple cleanup write surface. The embedded facade orchestrates the original,
//! unchanged retention engine; the browser never needs the script's workflow.
use crate::retention::{execute_program, protect};
use axum::{
    extract::{DefaultBodyLimit, State},
    http::StatusCode,
    middleware,
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::path::PathBuf;

const FACADE: &str = include_str!("../../../scripts/kanban_cleanup.py");
type ApiError = (StatusCode, Json<Value>);

pub fn router(home: PathBuf) -> Router {
    Router::new()
        .route("/api/settings/cleanup", get(status).put(save))
        .route("/api/settings/cleanup/review", post(review))
        .route("/api/settings/cleanup/run", post(run))
        .with_state(home)
        .layer(DefaultBodyLimit::max(16 * 1024))
        .layer(middleware::from_fn(protect))
}

/// The facade reports fixed lowercase categories; anything else stays generic.
async fn call(
    home: PathBuf,
    operation: &'static str,
    payload: Value,
) -> Result<Json<Value>, ApiError> {
    let value = execute_program(home, FACADE, true, operation, payload).await?;
    if let Some(code) = value.get("error") {
        let code = code
            .as_str()
            .filter(|c| {
                (1..=48).contains(&c.len())
                    && c.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
            })
            .unwrap_or("cleanup_failed");
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": code}))));
    }
    Ok(Json(value))
}

async fn status(State(home): State<PathBuf>) -> Result<Json<Value>, ApiError> {
    call(home, "status", json!({})).await
}
async fn review(
    State(home): State<PathBuf>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    call(home, "review", payload).await
}
async fn save(
    State(home): State<PathBuf>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    call(home, "save", payload).await
}
async fn run(
    State(home): State<PathBuf>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    call(home, "run", payload).await
}
