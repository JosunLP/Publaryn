use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub auth: AuthConfig,
    pub storage: StorageConfig,
    pub search: SearchConfig,
    pub redis: RedisConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    #[serde(default = "default_bind")]
    pub bind_address: String,
    #[serde(default = "default_base_url")]
    pub base_url: String,
}

fn default_bind() -> String {
    "0.0.0.0:3000".to_owned()
}

fn default_base_url() -> String {
    "http://localhost:3000".to_owned()
}

#[derive(Debug, Deserialize, Clone)]
pub struct DatabaseConfig {
    pub url: String,
    #[serde(default = "default_pool_max")]
    pub max_connections: u32,
}

fn default_pool_max() -> u32 {
    20
}

#[derive(Debug, Deserialize, Clone)]
pub struct AuthConfig {
    pub jwt_secret: String,
    #[serde(default = "default_jwt_ttl")]
    pub jwt_ttl_seconds: i64,
    #[serde(default = "default_session_ttl")]
    pub session_ttl_seconds: i64,
    pub issuer: String,
}

fn default_jwt_ttl() -> i64 {
    3600
}

fn default_session_ttl() -> i64 {
    86400
}

#[derive(Debug, Deserialize, Clone)]
pub struct StorageConfig {
    pub endpoint: String,
    pub bucket: String,
    pub access_key: String,
    pub secret_key: String,
    #[serde(default = "default_region")]
    pub region: String,
}

fn default_region() -> String {
    "us-east-1".to_owned()
}

#[derive(Debug, Deserialize, Clone)]
pub struct SearchConfig {
    pub url: String,
    pub api_key: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RedisConfig {
    pub url: String,
}

impl Config {
    pub fn load() -> Result<Self> {
        let cfg = config::Config::builder()
            .add_source(
                config::Environment::default()
                    .separator("__")
                    .try_parsing(true),
            )
            .set_default("server.bind_address", "0.0.0.0:3000")?
            .set_default("server.base_url", "http://localhost:3000")?
            .set_default("database.max_connections", 20)?
            .set_default("auth.jwt_ttl_seconds", 3600)?
            .set_default("auth.session_ttl_seconds", 86400)?
            .set_default("storage.region", "us-east-1")?
            .set_default("search.url", "http://localhost:7700")?
            .set_default("redis.url", "redis://localhost:6379")?
            .build()
            .context("Failed to build configuration")?;

        cfg.try_deserialize().context("Failed to parse configuration")
    }
}
