//! Redis-backed sliding-window rate limiting middleware.
//!
//! Uses a fixed-window approach with Redis INCR + EXPIRE for simplicity and
//! correctness under horizontal scaling. Each window is keyed by
//! `(tier, identifier, minute-aligned timestamp)`.
//!
//! When Redis is unavailable the middleware is permissive — requests are
//! allowed through so that a Redis outage does not cause a full service
//! disruption. An error metric should be emitted so operators can react.

use axum::{
    body::Body,
    extract::{ConnectInfo, Request},
    http::{header::AUTHORIZATION, HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use fred::{clients::Client as RedisClient, interfaces::KeysInterface, types::ExpireOptions};
use std::net::SocketAddr;

use crate::config::RateLimitConfig;

/// Rate-limit tier applied to different endpoint groups.
#[derive(Debug, Clone, Copy)]
pub enum RateLimitTier {
    /// Authentication endpoints (register, login) — strictest limits.
    Auth,
    /// Write/mutation endpoints (publish, create, update, delete).
    Write,
    /// General read endpoints (list, get, search).
    Read,
    /// Native protocol adapter reads (npm, PyPI, Cargo, NuGet downloads).
    Protocol,
}

impl RateLimitTier {
    fn prefix(&self) -> &'static str {
        match self {
            Self::Auth => "rl:auth",
            Self::Write => "rl:write",
            Self::Read => "rl:read",
            Self::Protocol => "rl:proto",
        }
    }

    fn max_requests(&self, cfg: &RateLimitConfig) -> u64 {
        match self {
            Self::Auth => cfg.auth_requests_per_minute,
            Self::Write => cfg.write_requests_per_minute,
            Self::Read => cfg.read_requests_per_minute,
            Self::Protocol => cfg.protocol_requests_per_minute,
        }
    }
}

/// Classify a request into a rate-limit tier based on method + path.
pub fn classify_request(method: &axum::http::Method, path: &str) -> RateLimitTier {
    use axum::http::Method;

    // Auth endpoints — strictest
    if path.starts_with("/v1/auth/") {
        return RateLimitTier::Auth;
    }

    // Protocol adapter reads
    if matches!(method, &Method::GET | &Method::HEAD)
        && (path.starts_with("/npm/")
            || path.starts_with("/pypi/")
            || path.starts_with("/composer/")
            || path.starts_with("/rubygems/")
            || path.starts_with("/maven/")
            || path.starts_with("/cargo/")
            || path.starts_with("/nuget/")
            || path.starts_with("/oci/"))
    {
        return RateLimitTier::Protocol;
    }

    // Write operations
    if matches!(
        method,
        &Method::POST | &Method::PUT | &Method::PATCH | &Method::DELETE
    ) {
        return RateLimitTier::Write;
    }

    RateLimitTier::Read
}

/// Extract a rate-limit key from the request.
/// Prefers the authenticated identity (token hash prefix) over IP address
/// so that multiple users behind the same NAT are not unfairly limited.
fn extract_key(headers: &HeaderMap, peer_addr: Option<&SocketAddr>) -> String {
    if let Some(auth) = headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok()) {
        // Use a stable prefix of the credential to avoid storing full tokens
        let hash = {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(auth.as_bytes());
            hex::encode(&hasher.finalize()[..8])
        };
        return format!("tok:{hash}");
    }

    if let Some(addr) = peer_addr {
        return format!("ip:{}", addr.ip());
    }

    "ip:unknown".to_owned()
}

/// Axum middleware function for rate limiting.
///
/// Injected via `axum::middleware::from_fn_with_state`.
pub async fn rate_limit_middleware(
    axum::extract::State(state): axum::extract::State<crate::state::AppState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let config = &state.config.rate_limit;

    // Bypass when rate limiting is disabled or Redis is unavailable.
    if !config.enabled {
        return next.run(request).await;
    }

    let redis = match state.redis.as_ref() {
        Some(redis) => redis,
        None => return next.run(request).await,
    };

    let tier = classify_request(request.method(), request.uri().path());
    let max_requests = tier.max_requests(config);

    let peer_addr = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| &ci.0);
    let key = extract_key(request.headers(), peer_addr);

    // Fixed-window key: tier:identity:minute_timestamp
    let window = chrono::Utc::now().timestamp() / 60;
    let redis_key = format!("{}:{}:{}", tier.prefix(), key, window);

    match check_rate_limit(redis, &redis_key, max_requests).await {
        RateLimitResult::Allowed { current, limit } => {
            let mut response = next.run(request).await;
            let headers = response.headers_mut();
            headers.insert("x-ratelimit-limit", limit.into());
            headers.insert(
                "x-ratelimit-remaining",
                (limit.saturating_sub(current)).into(),
            );
            response
        }
        RateLimitResult::Exceeded { limit, retry_after } => {
            tracing::warn!(
                tier = ?tier,
                key = %key,
                limit = limit,
                "Rate limit exceeded"
            );
            too_many_requests_response(limit, retry_after)
        }
        RateLimitResult::RedisError => {
            // Fail open — allow the request through when Redis is down.
            tracing::warn!("Rate limit check failed (Redis error), allowing request");
            next.run(request).await
        }
    }
}

enum RateLimitResult {
    Allowed { current: u64, limit: u64 },
    Exceeded { limit: u64, retry_after: u64 },
    RedisError,
}

async fn check_rate_limit(redis: &RedisClient, key: &str, max_requests: u64) -> RateLimitResult {
    // INCR atomically increments and returns the new count.
    // If the key is new, Redis creates it with value 1.
    let count: u64 = match redis.incr::<u64, _>(key).await {
        Ok(count) => count,
        Err(_) => return RateLimitResult::RedisError,
    };

    // Set expiry on first request in the window (count == 1).
    if count == 1 {
        let _: Result<(), _> = redis.expire::<(), _>(key, 60, None::<ExpireOptions>).await;
    }

    if count > max_requests {
        // Compute seconds remaining in the current window.
        let ttl: i64 = redis.ttl::<i64, _>(key).await.unwrap_or(60);
        let retry_after = if ttl > 0 { ttl as u64 } else { 60 };
        RateLimitResult::Exceeded {
            limit: max_requests,
            retry_after,
        }
    } else {
        RateLimitResult::Allowed {
            current: count,
            limit: max_requests,
        }
    }
}

fn too_many_requests_response(limit: u64, retry_after: u64) -> Response {
    let body = serde_json::json!({
        "error": "Too many requests",
        "retry_after_seconds": retry_after,
    });

    (
        StatusCode::TOO_MANY_REQUESTS,
        [
            ("retry-after", retry_after.to_string()),
            ("x-ratelimit-limit", limit.to_string()),
            ("x-ratelimit-remaining", "0".to_string()),
            ("content-type", "application/json".to_string()),
        ],
        serde_json::to_string(&body).unwrap_or_default(),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Method;

    #[test]
    fn classify_auth_endpoints() {
        assert!(matches!(
            classify_request(&Method::POST, "/v1/auth/login"),
            RateLimitTier::Auth
        ));
        assert!(matches!(
            classify_request(&Method::POST, "/v1/auth/register"),
            RateLimitTier::Auth
        ));
    }

    #[test]
    fn classify_protocol_reads() {
        assert!(matches!(
            classify_request(&Method::GET, "/npm/@scope/package"),
            RateLimitTier::Protocol
        ));
        assert!(matches!(
            classify_request(&Method::GET, "/pypi/simple/requests/"),
            RateLimitTier::Protocol
        ));
        assert!(matches!(
            classify_request(&Method::GET, "/cargo/index/config.json"),
            RateLimitTier::Protocol
        ));
        assert!(matches!(
            classify_request(&Method::GET, "/nuget/v3/index.json"),
            RateLimitTier::Protocol
        ));
        assert!(matches!(
            classify_request(&Method::GET, "/composer/packages.json"),
            RateLimitTier::Protocol
        ));
        assert!(matches!(
            classify_request(&Method::GET, "/rubygems/info/rails"),
            RateLimitTier::Protocol
        ));
        assert!(matches!(
            classify_request(&Method::GET, "/maven/com/example/app/maven-metadata.xml"),
            RateLimitTier::Protocol
        ));
        assert!(matches!(
            classify_request(&Method::HEAD, "/oci/v2/example/app/manifests/latest"),
            RateLimitTier::Protocol
        ));
    }

    #[test]
    fn classify_write_endpoints() {
        assert!(matches!(
            classify_request(&Method::POST, "/v1/packages"),
            RateLimitTier::Write
        ));
        assert!(matches!(
            classify_request(&Method::PUT, "/npm/@scope/package"),
            RateLimitTier::Write
        ));
        assert!(matches!(
            classify_request(&Method::DELETE, "/v1/tokens/some-id"),
            RateLimitTier::Write
        ));
        assert!(matches!(
            classify_request(&Method::PUT, "/oci/v2/example/app/manifests/latest"),
            RateLimitTier::Write
        ));
    }

    #[test]
    fn classify_read_endpoints() {
        assert!(matches!(
            classify_request(&Method::GET, "/v1/packages/npm/express"),
            RateLimitTier::Read
        ));
        assert!(matches!(
            classify_request(&Method::GET, "/v1/search?q=test"),
            RateLimitTier::Read
        ));
    }
}
