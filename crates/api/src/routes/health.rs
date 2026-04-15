use axum::{http::StatusCode, routing::get, Json, Router};
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

/// GET /readiness — readiness probe (checks DB and returns 503 when unavailable)
async fn readiness_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> (StatusCode, Json<Value>) {
    let db_ok = sqlx::query("SELECT 1").fetch_one(&state.db).await.is_ok();

    readiness_response(db_ok)
}

fn readiness_response(database_ready: bool) -> (StatusCode, Json<Value>) {
    let status = if database_ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (status, Json(json!({
        "status": if database_ready { "ready" } else { "not_ready" },
        "database": if database_ready { "ok" } else { "error" },
    })))
}

#[cfg(test)]
mod tests {
    use super::readiness_response;
    use axum::{http::StatusCode, Json};

    #[test]
    fn readiness_response_is_ok_when_dependencies_are_ready() {
        let (status, Json(body)) = readiness_response(true);

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "ready");
        assert_eq!(body["database"], "ok");
    }

    #[test]
    fn readiness_response_is_unavailable_when_dependencies_fail() {
        let (status, Json(body)) = readiness_response(false);

        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["status"], "not_ready");
        assert_eq!(body["database"], "error");
    }
}
