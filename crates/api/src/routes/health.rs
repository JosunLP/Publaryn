use axum::{http::StatusCode, routing::get, Json, Router};
use fred::prelude::*;
use serde_json::{json, Value};
use utoipa;

use crate::state::AppState;

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
        (status = 200, description = "Service is alive"),
    )
)]
#[allow(dead_code)]
pub async fn health_handler_doc() {}

async fn health_handler() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "service": "publaryn",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

/// GET /readiness — readiness probe (checks DB and Redis availability)
async fn readiness_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> (StatusCode, Json<Value>) {
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

fn readiness_response(database_ready: bool, redis_ready: bool) -> (StatusCode, Json<Value>) {
    let all_ready = database_ready && redis_ready;
    let status = if all_ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (status, Json(json!({
        "status": if all_ready { "ready" } else { "not_ready" },
        "database": if database_ready { "ok" } else { "error" },
        "redis": if redis_ready { "ok" } else { "error" },
    })))
}

#[cfg(test)]
mod tests {
    use super::readiness_response;
    use axum::{http::StatusCode, Json};

    #[test]
    fn readiness_response_is_ok_when_dependencies_are_ready() {
        let (status, Json(body)) = readiness_response(true, true);

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "ready");
        assert_eq!(body["database"], "ok");
        assert_eq!(body["redis"], "ok");
    }

    #[test]
    fn readiness_response_is_unavailable_when_database_fails() {
        let (status, Json(body)) = readiness_response(false, true);

        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["status"], "not_ready");
        assert_eq!(body["database"], "error");
    }

    #[test]
    fn readiness_response_is_unavailable_when_redis_fails() {
        let (status, Json(body)) = readiness_response(true, false);

        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["status"], "not_ready");
        assert_eq!(body["redis"], "error");
    }
}
