use anyhow::{bail, Context, Result};
use axum::http::HeaderValue;
use serde::Deserialize;
use url::Url;

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
    #[serde(default)]
    pub cors_allowed_origins: Vec<String>,
}

fn default_bind() -> String {
    "0.0.0.0:3000".to_owned()
}

fn default_base_url() -> String {
    "http://localhost:3000".to_owned()
}

impl ServerConfig {
    pub fn cors_allowed_origins(&self) -> Result<Vec<HeaderValue>> {
        self.cors_allowed_origins
            .iter()
            .map(|origin| normalize_cors_allowed_origin(origin))
            .collect()
    }
}

fn normalize_cors_allowed_origin(origin: &str) -> Result<HeaderValue> {
    let trimmed_origin = origin.trim();

    if trimmed_origin.is_empty() {
        bail!("CORS allowed origins must not contain empty values");
    }

    if trimmed_origin == "*" {
        bail!("Wildcard CORS origins are not allowed; configure explicit origins");
    }

    let parsed_origin = Url::parse(trimmed_origin)
        .with_context(|| format!("Invalid CORS origin '{trimmed_origin}'"))?;

    if !matches!(parsed_origin.scheme(), "http" | "https") {
        bail!("CORS origin '{trimmed_origin}' must use http or https");
    }

    if parsed_origin.host_str().is_none() {
        bail!("CORS origin '{trimmed_origin}' must include a host");
    }

    if !parsed_origin.username().is_empty() || parsed_origin.password().is_some() {
        bail!("CORS origin '{trimmed_origin}' must not include user credentials");
    }

    if parsed_origin.path() != "/"
        || parsed_origin.query().is_some()
        || parsed_origin.fragment().is_some()
    {
        bail!("CORS origin '{trimmed_origin}' must not include a path, query string, or fragment");
    }

    let normalized_origin = parsed_origin.origin().ascii_serialization();

    HeaderValue::from_str(&normalized_origin)
        .with_context(|| format!("Invalid CORS origin '{trimmed_origin}'"))
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
                    .list_separator(",")
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

        cfg.try_deserialize()
            .context("Failed to parse configuration")
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize_cors_allowed_origin, ServerConfig};

    #[test]
    fn normalizes_cors_origin_with_trailing_slash() {
        let origin = normalize_cors_allowed_origin("https://packages.example.com/")
            .expect("origin with trailing slash should be accepted");

        assert_eq!(
            origin.to_str().expect("header value should be ascii"),
            "https://packages.example.com"
        );
    }

    #[test]
    fn rejects_wildcard_cors_origin() {
        let error =
            normalize_cors_allowed_origin("*").expect_err("wildcard origin should be rejected");

        assert_eq!(
            error.to_string(),
            "Wildcard CORS origins are not allowed; configure explicit origins"
        );
    }

    #[test]
    fn rejects_cors_origin_with_path() {
        let error = normalize_cors_allowed_origin("https://packages.example.com/app")
            .expect_err("origins with paths should be rejected");

        assert_eq!(
            error.to_string(),
            "CORS origin 'https://packages.example.com/app' must not include a path, query string, or fragment"
        );
    }

    #[test]
    fn collects_valid_cors_origin_list() {
        let server = ServerConfig {
            bind_address: "0.0.0.0:3000".into(),
            base_url: "http://localhost:3000".into(),
            cors_allowed_origins: vec![
                "http://localhost:5173/".into(),
                "https://packages.example.com".into(),
            ],
        };

        let origins = server
            .cors_allowed_origins()
            .expect("valid origins should be collected");

        let rendered = origins
            .iter()
            .map(|origin| {
                origin
                    .to_str()
                    .expect("header value should be ascii")
                    .to_owned()
            })
            .collect::<Vec<_>>();

        assert_eq!(
            rendered,
            vec![
                "http://localhost:5173".to_owned(),
                "https://packages.example.com".to_owned(),
            ]
        );
    }
}
