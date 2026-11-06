//! Narrow opt-in retention write surface. The Python implementation is embedded
//! so a deployed binary never depends on a compile-time source checkout path.
use axum::{
    extract::{DefaultBodyLimit, Request, State},
    http::{header, HeaderValue, Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const SCRIPT: &str = include_str!("../../../scripts/kanban_retention.py");
pub(crate) const UI_ORIGIN: &str = "http://127.0.0.1:8081";
type ApiError = (StatusCode, Json<Value>);

/// Same shared Kanban root as Hermes, NOT the active profile's HERMES_HOME.
pub fn shared_home() -> PathBuf {
    std::env::var_os("HERMES_KANBAN_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".hermes")
        })
}

pub fn router(home: PathBuf) -> Router {
    Router::new()
        .route("/api/settings/retention", get(settings).put(save))
        .route("/api/settings/retention/preview", post(preview))
        .route("/api/settings/retention/track", post(track))
        .route("/api/settings/retention/apply", post(apply))
        .with_state(home)
        .layer(DefaultBodyLimit::max(16 * 1024))
        .layer(middleware::from_fn(protect))
}

fn error(status: StatusCode, code: &'static str) -> ApiError {
    (status, Json(json!({"error": code})))
}

fn exactly(headers: &axum::http::HeaderMap, name: &str, value: &str) -> bool {
    let mut values = headers.get_all(name).iter();
    values.next().is_some_and(|v| v == value) && values.next().is_none()
}

pub(crate) async fn protect(request: Request, next: Next) -> Response {
    let origin_allowed = exactly(request.headers(), "origin", UI_ORIGIN);
    // Host validation also rejects DNS rebinding; do not relax for public deployment.
    if !exactly(request.headers(), "host", "127.0.0.1:8788") {
        return error(StatusCode::FORBIDDEN, "invalid_host").into_response();
    }
    let mutation = request.method() != Method::GET && request.method() != Method::OPTIONS;
    if mutation
        && (!origin_allowed || !exactly(request.headers(), "x-pengudeck-request", "retention-v1"))
    {
        return error(StatusCode::FORBIDDEN, "untrusted_request").into_response();
    }
    let mut response = if request.method() == Method::OPTIONS {
        if !origin_allowed {
            return error(StatusCode::FORBIDDEN, "untrusted_request").into_response();
        }
        StatusCode::NO_CONTENT.into_response()
    } else {
        next.run(request).await
    };
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.append(header::VARY, HeaderValue::from_static("Origin"));
    if origin_allowed {
        headers.insert(
            header::ACCESS_CONTROL_ALLOW_ORIGIN,
            HeaderValue::from_static(UI_ORIGIN),
        );
        headers.insert(
            header::ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static("GET, PUT, POST, OPTIONS"),
        );
        headers.insert(
            header::ACCESS_CONTROL_ALLOW_HEADERS,
            HeaderValue::from_static("Content-Type, X-Pengudeck-Request"),
        );
    }
    response
}

/// Bounded subprocess I/O and wall time. No user-controlled executable, code or paths.
pub async fn execute(
    home: PathBuf,
    operation: &'static str,
    payload: Value,
) -> Result<Value, ApiError> {
    execute_program(home, SCRIPT, false, operation, payload).await
}

/// Same bounded runner for another embedded program. `with_engine` hands the
/// original retention script to it through the environment, never a path.
pub(crate) async fn execute_program(
    home: PathBuf,
    code: &'static str,
    with_engine: bool,
    operation: &'static str,
    payload: Value,
) -> Result<Value, ApiError> {
    tokio::task::spawn_blocking(move || {
        execute_blocking(home, code, with_engine, operation, payload)
    })
    .await
    .map_err(|_| error(StatusCode::SERVICE_UNAVAILABLE, "retention_unavailable"))?
}

fn execute_blocking(
    home: PathBuf,
    code: &'static str,
    with_engine: bool,
    operation: &'static str,
    payload: Value,
) -> Result<Value, ApiError> {
    let failed = || error(StatusCode::SERVICE_UNAVAILABLE, "retention_unavailable");
    let mut command = Command::new("python3");
    if with_engine {
        command.env("PENGUDECK_RETENTION_SCRIPT", SCRIPT);
    }
    let mut child = command
        .arg("-c")
        .arg(code)
        .arg(operation)
        .arg("--home")
        .arg(home)
        .arg("--input-json")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| failed())?;
    let mut stdin = child.stdin.take().ok_or_else(failed)?;
    let stdout = child.stdout.take().ok_or_else(failed)?;
    let input = serde_json::to_vec(&payload).map_err(|_| failed())?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let work = (|| {
        stdin.write_all(&input).map_err(|_| failed())?;
        drop(stdin);
        let started = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().map_err(|_| failed())? {
                break status;
            }
            if started.elapsed() > Duration::from_secs(65) {
                return Err(failed());
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        if !status.success() {
            return Err(error(StatusCode::BAD_REQUEST, "retention_request_failed"));
        }
        Ok(())
    })();
    // Always reap on every path, including timeout and malformed output.
    let _ = child.kill();
    let _ = child.wait();
    let bytes = reader.join().map_err(|_| failed())?.map_err(|_| failed())?;
    work?;
    if bytes.len() > 1024 * 1024 {
        return Err(failed());
    }
    serde_json::from_slice(&bytes).map_err(|_| failed())
}

async fn settings(State(home): State<PathBuf>) -> Result<Json<Value>, ApiError> {
    execute(home, "get", json!({})).await.map(Json)
}
async fn save(
    State(home): State<PathBuf>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    execute(home, "save", payload).await.map(Json)
}
async fn preview(State(home): State<PathBuf>) -> Result<Json<Value>, ApiError> {
    execute(home, "preview", json!({})).await.map(Json)
}
async fn apply(
    State(home): State<PathBuf>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    execute(home, "apply", payload).await.map(Json)
}

async fn track(
    State(home): State<PathBuf>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    execute(home, "track", payload).await.map(Json)
}

/// Lifecycle-bound scheduler: no host cron/unit, no action while policy is disabled.
/// The script holds a cross-process lock and enforces at least 24 hours between attempts.
pub async fn scheduler(home: PathBuf) {
    loop {
        if execute(home.clone(), "tick", json!({})).await.is_err() {
            eprintln!("retention tick failed; inspect Settings before retrying");
        }
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
}
