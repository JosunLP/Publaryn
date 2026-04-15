use axum::{routing::get, Json, Router};
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
pub async fn health_handler_doc() {}

async fn health_handler() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "service": "publaryn",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

/// GET /readiness — readiness probe (checks DB)
async fn readiness_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Json<Value> {
    let db_ok = sqlx::query("SELECT 1").fetch_one(&state.db).await.is_ok();
    Json(json!({
        "status": if db_ok { "ready" } else { "not_ready" },
        "database": if db_ok { "ok" } else { "error" },
    }))
}
