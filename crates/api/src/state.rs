use anyhow::Result;
use fred::{
    clients::Client as RedisClient,
    interfaces::ClientLike,
    types::{config::Config as RedisConfig, Builder as RedisBuilder},
};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::sync::Arc;

use publaryn_search::index::MeilisearchIndex;

use crate::config::Config;
use crate::storage::{ArtifactStore, S3ArtifactStore};

/// Shared application state injected into every handler.
#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<Config>,
    pub search: Arc<MeilisearchIndex>,
    pub artifact_store: Arc<dyn ArtifactStore>,
    pub redis: Option<Arc<RedisClient>>,
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
        let artifact_store = Arc::new(S3ArtifactStore::new(&cfg.storage).await?);

        // Connect to Redis when configured.
        let redis = match connect_redis(&cfg.redis.url).await {
            Ok(client) => {
                tracing::info!("Redis connected at {}", &cfg.redis.url);
                Some(Arc::new(client))
            }
            Err(err) => {
                tracing::warn!("Redis unavailable, rate limiting and caching disabled: {err}");
                None
            }
        };

        Ok(Self {
            db,
            config: Arc::new(cfg.clone()),
            search,
            artifact_store,
            redis,
        })
    }

    /// Build an `AppState` from an already-provisioned database pool (for tests).
    /// Uses an in-memory artifact store and a real (but test-pointed) Meilisearch client.
    pub fn new_with_pool(db: PgPool, config: Config) -> Self {
        let search = Arc::new(MeilisearchIndex::new(
            &config.search.url,
            config.search.api_key.as_deref(),
        ));
        Self {
            db,
            config: Arc::new(config),
            search,
            artifact_store: Arc::new(crate::storage::MemoryArtifactStore::new()),
            redis: None,
        }
    }
}

async fn connect_redis(url: &str) -> Result<RedisClient> {
    let config = RedisConfig::from_url(url)?;
    let client = RedisBuilder::from_config(config).build()?;
    client.init().await?;
    Ok(client)
}
