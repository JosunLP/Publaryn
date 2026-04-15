use anyhow::Result;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::sync::Arc;

use publaryn_search::index::MeilisearchIndex;

use crate::config::Config;

/// Shared application state injected into every handler.
#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<Config>,
    pub search: Arc<MeilisearchIndex>,
}

impl AppState {
    pub async fn new(cfg: &Config) -> Result<Self> {
        let db = PgPoolOptions::new()
            .max_connections(cfg.database.max_connections)
            .connect(&cfg.database.url)
            .await?;

        // Run pending migrations automatically on startup.
        sqlx::migrate!("../../migrations").run(&db).await?;

        let search = Arc::new(MeilisearchIndex::new(
            &cfg.search.url,
            cfg.search.api_key.as_deref(),
        ));

        Ok(Self {
            db,
            config: Arc::new(cfg.clone()),
            search,
        })
    }
}
