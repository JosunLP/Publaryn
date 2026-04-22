use axum::{http::StatusCode, routing::get, Json, Router};
use fred::interfaces::ClientLike;
use serde::Serialize;
use utoipa::ToSchema;

use crate::state::AppState;

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct HealthResponse {
    pub status: String,
    pub service: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct ReadinessResponse {
    pub status: String,
    pub database: String,
    pub redis: String,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health_handler))
        .route("/readiness", get(readiness_handler))
}

/// GET /health — liveness probe
#[utoipa::path(
    get,
    path = "/health",
    tag = "health",
    responses(
        (status = 200, description = "Service is alive", body = HealthResponse),
    )
)]
#[allow(dead_code)]
pub async fn health_handler_doc() {}

/// GET /readiness — readiness probe (checks DB and Redis availability)
#[utoipa::path(
    get,
    path = "/readiness",
    tag = "health",
    responses(
        (status = 200, description = "Service is ready", body = ReadinessResponse),
        (status = 503, description = "One or more required dependencies are not ready", body = ReadinessResponse),
    )
)]
#[allow(dead_code)]
pub async fn readiness_handler_doc() {}

async fn health_handler() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_owned(),
        service: "publaryn".to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
    })
}

async fn readiness_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> (StatusCode, Json<ReadinessResponse>) {
    let db_ok = sqlx::query("SELECT 1").fetch_one(&state.db).await.is_ok();

    let redis_ok = match state.redis.as_ref() {
        Some(redis) => {
            let result: Result<String, _> = redis.ping(None).await;
            result.is_ok()
        }
        None => true, // Redis is optional; if not configured, don't block readiness
    };

    readiness_response(db_ok, redis_ok)
}

fn readiness_response(
    database_ready: bool,
    redis_ready: bool,
) -> (StatusCode, Json<ReadinessResponse>) {
    let all_ready = database_ready && redis_ready;
    let status = if all_ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (
        status,
        Json(ReadinessResponse {
            status: if all_ready { "ready" } else { "not_ready" }.to_owned(),
            database: if database_ready { "ok" } else { "error" }.to_owned(),
            redis: if redis_ready { "ok" } else { "error" }.to_owned(),
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::readiness_response;
    use axum::{http::StatusCode, Json};

    #[test]
    fn readiness_response_is_ok_when_dependencies_are_ready() {
        let (status, Json(body)) = readiness_response(true, true);

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ready");
        assert_eq!(body.database, "ok");
        assert_eq!(body.redis, "ok");
    }

    #[test]
    fn readiness_response_is_unavailable_when_database_fails() {
        let (status, Json(body)) = readiness_response(false, true);

        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body.status, "not_ready");
        assert_eq!(body.database, "error");
    }

    #[test]
    fn readiness_response_is_unavailable_when_redis_fails() {
        let (status, Json(body)) = readiness_response(true, false);

        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body.status, "not_ready");
        assert_eq!(body.redis, "error");
    }
}
