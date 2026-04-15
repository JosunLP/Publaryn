use anyhow::Result;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

mod config;
mod error;
mod npm_bridge;
mod request_auth;
mod router;
mod routes;
mod scopes;
mod state;
mod storage;

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env if present
    dotenvy::dotenv().ok();

    // Initialize tracing
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,publaryn=debug".into()))
        .with(tracing_subscriber::fmt::layer().json())
        .init();

    let cfg = config::Config::load()?;
    tracing::info!("Starting Publaryn v{}", env!("CARGO_PKG_VERSION"));

    let app_state = state::AppState::new(&cfg).await?;
    let app = router::build_router(app_state)?;

    let listener = tokio::net::TcpListener::bind(&cfg.server.bind_address).await?;
    tracing::info!("Listening on {}", cfg.server.bind_address);

    axum::serve(listener, app).await?;
    Ok(())
}
