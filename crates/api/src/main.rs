use anyhow::Result;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use publaryn_api::{config, job_handlers::ReindexSearchHandler, router, state};
use publaryn_workers::queue::JobKind;
use publaryn_workers::scanners::{PolicyScanner, ScanArtifactHandler, SecretsScanner};
use publaryn_workers::worker::{Worker, WorkerConfig};

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
    let app = router::build_router(app_state.clone())?;

    // Spawn the background worker alongside the API server.
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let worker_db = app_state.db.clone();
    let worker_artifact_store = app_state.artifact_store.clone();
    let worker_handle = tokio::spawn(async move {
        let worker_config = WorkerConfig::default();
        let mut worker = Worker::new(worker_db.clone(), worker_config);

        // Build the artifact scanning pipeline.
        let store_reader = std::sync::Arc::new(
            publaryn_api::storage::ArtifactStoreReaderAdapter::new(worker_artifact_store),
        );
        let scanners: Vec<Box<dyn publaryn_workers::scanners::ArtifactScanner>> = vec![
            Box::new(PolicyScanner {
                max_artifact_bytes: 500 * 1024 * 1024, // 500 MiB
            }),
            Box::new(SecretsScanner::new()),
        ];
        let scan_handler = std::sync::Arc::new(ScanArtifactHandler {
            db: worker_db,
            artifact_store: store_reader,
            scanners,
        });
        worker.register_handler(JobKind::ScanArtifact, scan_handler);

        let reindex_handler = std::sync::Arc::new(ReindexSearchHandler {
            db: app_state.db.clone(),
            search: app_state.search.clone(),
        });
        worker.register_handler(JobKind::ReindexSearch, reindex_handler);

        worker.run(shutdown_rx).await;
    });

    let listener = tokio::net::TcpListener::bind(&cfg.server.bind_address).await?;
    tracing::info!("Listening on {}", cfg.server.bind_address);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    // Signal the background worker to shut down and wait for it.
    let _ = shutdown_tx.send(true);
    let _ = tokio::time::timeout(std::time::Duration::from_secs(10), worker_handle).await;

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
