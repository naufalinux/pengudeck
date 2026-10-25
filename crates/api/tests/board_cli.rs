#![cfg(unix)]
use axum::{
    body::{to_bytes, Body},
    handler::Handler,
    http::{Request, StatusCode},
    routing::any_service,
};
use hermes_dash_api::{
    boards::{BoardSource, CliBoardSource},
    router,
};
use hermes_dash_core::source::CliSource;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct FakeCli {
    dir: PathBuf,
    bin: PathBuf,
}
impl FakeCli {
    fn new(body: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "hermes-dash-t_ae8c3d44-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        let bin = dir.join("fake hermes ; literal");
        fs::write(&bin, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o700)).unwrap();
        Self { dir, bin }
    }
    fn source(&self) -> CliBoardSource {
        CliBoardSource {
            hermes_bin: self.bin.clone(),
        }
    }
}
impl Drop for FakeCli {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.dir).unwrap();
    }
}
#[test]
fn board_cli_passes_exact_read_only_argv() {
    let fake = FakeCli::new(
        r#"[ "$#" -eq 3 ] && [ "$1" = kanban ] && [ "$2" = boards ] && [ "$3" = list ] || exit 12
printf 'SLUG NAME COUNTS\nzaha Zaha Team done=2\n'
printf 'STDERR_SECRET' >&2"#,
    );
    assert_eq!(fake.source().boards().unwrap(), ["zaha"]);
}
#[tokio::test]
async fn cli_failures_never_leak_sentinel_into_http_responses() {
    for script in [
        "printf 'STDERR_SECRET /private/path' >&2; exit 7",
        "printf 'STDERR_SECRET malformed stdout'",
        "printf '\\377'",
    ] {
        let fake = FakeCli::new(script);
        let app = router(
            Arc::new(fake.source()),
            Arc::new(CliSource {
                hermes_bin: fake.bin.clone(),
            }),
        );
        let response = any_service(app)
            .call(
                Request::builder()
                    .uri("/api/boards")
                    .body(Body::empty())
                    .unwrap(),
                (),
            )
            .await;
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(response.headers()["content-type"], "application/json");
        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        assert_eq!(&body[..], br#"{"error":"source_unavailable"}"#);
    }
}
#[test]
fn board_cli_spawn_failure_is_sanitized() {
    let fake = FakeCli::new("exit 0");
    let source = CliBoardSource {
        hermes_bin: fake.dir.join("MISSING_SECRET_PATH"),
    };
    let error = source.boards().unwrap_err();
    assert!(!format!("{error} {error:?}").contains("MISSING_SECRET_PATH"));
}
