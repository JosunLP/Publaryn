use axum::{extract::State, routing::get, Json, Router};
use sqlx::Row;

use publaryn_core::error::Error;

use crate::{
    error::{ApiError, ApiResult},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/stats", get(platform_stats))
}

/// GET /v1/stats — public platform statistics
async fn platform_stats(State(state): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    let row = sqlx::query(
        "SELECT \
           (SELECT COUNT(*) FROM packages WHERE visibility = 'public') AS package_count, \
           (SELECT COUNT(*) FROM releases WHERE status = 'published') AS release_count, \
           (SELECT COUNT(*) FROM organizations) AS org_count",
    )
    .fetch_one(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    let package_count: i64 = row.try_get("package_count").unwrap_or(0);
    let release_count: i64 = row.try_get("release_count").unwrap_or(0);
    let org_count: i64 = row.try_get("org_count").unwrap_or(0);

    Ok(Json(serde_json::json!({
        "packages": package_count,
        "releases": release_count,
        "organizations": org_count,
    })))
}
