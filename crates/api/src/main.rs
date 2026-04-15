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

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    tracing::info!("Publaryn shutdown complete");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C signal handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install terminate signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("Received Ctrl+C shutdown signal");
        }
        _ = terminate => {
            tracing::info!("Received terminate shutdown signal");
        }
    }
}
