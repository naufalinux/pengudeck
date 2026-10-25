use hermes_dash_api::{boards::CliBoardSource, router, BIND_ADDR};
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
    );
    let listener = tokio::net::TcpListener::bind(BIND_ADDR).await?;
    axum::serve(listener, app).await
}
