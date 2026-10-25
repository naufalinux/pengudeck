pub mod boards;

use axum::{routing::get, Json, Router};
use serde_json::{json, Value};

pub fn router() -> Router {
    Router::new().route(
        "/api/health",
        get(|| async { Json::<Value>(json!({"ok": true})) }),
    )
}
