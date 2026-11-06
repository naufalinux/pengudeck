use hermes_dash_api::{boards::CliBoardSource, cleanup, retention, router, BIND_ADDR};
use hermes_dash_core::source::CliSource;
use std::{path::PathBuf, sync::Arc};

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let hermes_bin = PathBuf::from("hermes");
    let app = router(
        Arc::new(CliBoardSource {
            hermes_bin: hermes_bin.clone(),
        }),
        Arc::new(CliSource { hermes_bin }),
    )
    .merge(retention::router(retention::shared_home()))
    .merge(cleanup::router(retention::shared_home()));
    let listener = tokio::net::TcpListener::bind(BIND_ADDR).await?;
    let scheduler = tokio::spawn(retention::scheduler(retention::shared_home()));
    let result = axum::serve(listener, app).await;
    scheduler.abort();
    result
}
