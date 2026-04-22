use axum::{extract::State, routing::get, Json, Router};
use serde::Serialize;
use sqlx::Row;
use utoipa::ToSchema;

use publaryn_core::error::Error;

use crate::{
    error::{ApiError, ApiResult},
    state::AppState,
};

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct PlatformStatsResponse {
    pub packages: i64,
    pub releases: i64,
    pub organizations: i64,
    pub security_findings_total: i64,
    pub security_findings_unresolved: i64,
    pub artifacts_stored: i64,
    pub job_queue_pending: i64,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/stats", get(platform_stats))
}

/// GET /v1/stats — public platform statistics
#[utoipa::path(
    get,
    path = "/v1/stats",
    tag = "stats",
    responses(
        (status = 200, description = "Public platform statistics", body = PlatformStatsResponse),
    )
)]
#[allow(dead_code)]
pub async fn platform_stats_doc() {}

async fn platform_stats(State(state): State<AppState>) -> ApiResult<Json<PlatformStatsResponse>> {
    let row = sqlx::query(
        "SELECT \
           (SELECT COUNT(*) FROM packages WHERE visibility = 'public') AS package_count, \
           (SELECT COUNT(*) FROM releases WHERE status = 'published') AS release_count, \
           (SELECT COUNT(*) FROM organizations) AS org_count, \
           (SELECT COUNT(*) FROM security_findings) AS security_findings_total, \
           (SELECT COUNT(*) FROM security_findings WHERE is_resolved = false) AS security_findings_unresolved, \
           (SELECT COUNT(*) FROM artifacts) AS artifacts_stored, \
           (SELECT COUNT(*) FROM background_jobs WHERE status = 'pending') AS job_queue_pending",
    )
    .fetch_one(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    let package_count: i64 = row.try_get("package_count").unwrap_or(0);
    let release_count: i64 = row.try_get("release_count").unwrap_or(0);
    let org_count: i64 = row.try_get("org_count").unwrap_or(0);
    let security_findings_total: i64 = row.try_get("security_findings_total").unwrap_or(0);
    let security_findings_unresolved: i64 =
        row.try_get("security_findings_unresolved").unwrap_or(0);
    let artifacts_stored: i64 = row.try_get("artifacts_stored").unwrap_or(0);
    let job_queue_pending: i64 = row.try_get("job_queue_pending").unwrap_or(0);

    Ok(Json(PlatformStatsResponse {
        packages: package_count,
        releases: release_count,
        organizations: org_count,
        security_findings_total,
        security_findings_unresolved,
        artifacts_stored,
        job_queue_pending,
    }))
}
