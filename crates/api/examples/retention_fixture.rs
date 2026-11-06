//! Disposable loopback-only server for the browser contract test; never production.
use hermes_dash_api::{boards::CliBoardSource, cleanup, retention, router};
use hermes_dash_core::source::CliSource;
use std::{io::Write, path::PathBuf, sync::Arc};

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let home = retention::shared_home();
    if !home.join(".pengudeck-test-fixture").is_file() {
        return Err(std::io::Error::other("private fixture marker required"));
    }
    let app = router(
        Arc::new(CliBoardSource {
            hermes_bin: PathBuf::from("hermes"),
        }),
        Arc::new(CliSource {
            hermes_bin: PathBuf::from("hermes"),
        }),
    )
    .merge(retention::router(home.clone()))
    .merge(cleanup::router(home));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    println!("{}", listener.local_addr()?);
    std::io::stdout().flush()?;
    axum::serve(listener, app).await
}
