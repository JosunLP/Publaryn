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
    #[serde(default)]
    pub rate_limit: RateLimitConfig,
    /// Enable artifact scanning pipeline. When false, releases go directly to
    /// `published` status without enqueueing a scan job.
    #[serde(default = "default_scanning_enabled")]
    pub scanning_enabled: bool,
}

fn default_scanning_enabled() -> bool {
    true
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    #[serde(default = "default_bind")]
    pub bind_address: String,
    #[serde(default = "default_base_url")]
    pub base_url: String,
    #[serde(default)]
    pub cors_allowed_origins: Vec<String>,
    #[serde(default)]
    pub static_dir: Option<String>,
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

#[derive(Debug, Deserialize, Clone)]
pub struct RateLimitConfig {
    /// Enable or disable rate limiting globally.
    #[serde(default = "default_rate_limit_enabled")]
    pub enabled: bool,
    /// Maximum requests per window for authentication endpoints (register, login).
    #[serde(default = "default_auth_rpm")]
    pub auth_requests_per_minute: u64,
    /// Maximum requests per window for write/mutation endpoints.
    #[serde(default = "default_write_rpm")]
    pub write_requests_per_minute: u64,
    /// Maximum requests per window for general read endpoints.
    #[serde(default = "default_read_rpm")]
    pub read_requests_per_minute: u64,
    /// Maximum requests per window for native protocol adapter reads.
    #[serde(default = "default_protocol_rpm")]
    pub protocol_requests_per_minute: u64,
}

fn default_rate_limit_enabled() -> bool {
    true
}
fn default_auth_rpm() -> u64 {
    10
}
fn default_write_rpm() -> u64 {
    60
}
fn default_read_rpm() -> u64 {
    300
}
fn default_protocol_rpm() -> u64 {
    1000
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            enabled: default_rate_limit_enabled(),
            auth_requests_per_minute: default_auth_rpm(),
            write_requests_per_minute: default_write_rpm(),
            read_requests_per_minute: default_read_rpm(),
            protocol_requests_per_minute: default_protocol_rpm(),
        }
    }
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
            .set_default("rate_limit.enabled", true)?
            .set_default("rate_limit.auth_requests_per_minute", 10)?
            .set_default("rate_limit.write_requests_per_minute", 60)?
            .set_default("rate_limit.read_requests_per_minute", 300)?
            .set_default("rate_limit.protocol_requests_per_minute", 1000)?
            .set_default("scanning_enabled", true)?
            .build()
            .context("Failed to build configuration")?;

        cfg.try_deserialize()
            .context("Failed to parse configuration")
    }

    /// Build a configuration suitable for integration tests.
    /// Uses the provided database URL and sensible defaults for everything else.
    pub fn test_config(database_url: &str) -> Self {
        Self {
            server: ServerConfig {
                bind_address: "127.0.0.1:0".to_owned(),
                base_url: "http://localhost:3000".to_owned(),
                cors_allowed_origins: vec![],
                static_dir: None,
            },
            database: DatabaseConfig {
                url: database_url.to_owned(),
                max_connections: 5,
            },
            auth: AuthConfig {
                jwt_secret: "test-secret-that-is-long-enough-for-hmac-256!".to_owned(),
                jwt_ttl_seconds: 3600,
                session_ttl_seconds: 86400,
                issuer: "https://test.publaryn.dev".to_owned(),
            },
            storage: StorageConfig {
                endpoint: "http://localhost:9000".to_owned(),
                bucket: "test-artifacts".to_owned(),
                access_key: "minioadmin".to_owned(),
                secret_key: "minioadmin".to_owned(),
                region: "us-east-1".to_owned(),
            },
            search: SearchConfig {
                url: "http://localhost:7700".to_owned(),
                api_key: None,
            },
            redis: RedisConfig {
                url: "redis://localhost:6379".to_owned(),
            },
            rate_limit: RateLimitConfig {
                enabled: false,
                auth_requests_per_minute: 100,
                write_requests_per_minute: 200,
                read_requests_per_minute: 1000,
                protocol_requests_per_minute: 5000,
            },
            scanning_enabled: false,
        }
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
            static_dir: None,
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
