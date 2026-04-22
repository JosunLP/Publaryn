use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use sqlx::{Postgres, QueryBuilder, Row};

use publaryn_core::error::Error;
use publaryn_workers::queue::{self, Job, JobKind, JobStatus};

use crate::{
    error::{ApiError, ApiResult},
    request_auth::{ensure_platform_admin, AuthenticatedIdentity},
    scopes::{ensure_scope, SCOPE_AUDIT_READ},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/admin/jobs", get(list_background_jobs))
}

#[derive(Debug, Deserialize)]
struct AdminJobsQuery {
    state: Option<String>,
    kind: Option<String>,
    page: Option<u32>,
    per_page: Option<u32>,
}

#[derive(Debug, sqlx::FromRow)]
struct JobKindCountRow {
    kind: JobKind,
    count: i64,
}

async fn list_background_jobs(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Query(query): Query<AdminJobsQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_AUDIT_READ)?;
    ensure_platform_admin(&state.db, identity.user_id).await?;

    let state_filter = query
        .state
        .as_deref()
        .map(parse_job_status)
        .transpose()?;
    let kind_filter = query.kind.as_deref().map(parse_job_kind).transpose()?;
    let page = query.page.unwrap_or(1).max(1);
    let per_page = query.per_page.unwrap_or(50).clamp(1, 100) as i64;
    let offset = ((page - 1) as i64) * per_page;

    let total = count_jobs(&state.db, state_filter, kind_filter).await?;
    let jobs = load_jobs(&state.db, state_filter, kind_filter, per_page, offset).await?;
    let counts = queue::job_counts(&state.db)
        .await
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let by_kind = load_job_counts_by_kind(&state.db).await?;
    let oldest_pending_age_minutes = oldest_pending_age_minutes(&state.db).await?;
    let stale_jobs_count = stale_jobs_count(&state.db).await?;

    Ok(Json(serde_json::json!({
        "page": page,
        "per_page": per_page,
        "total": total,
        "filters": {
            "state": state_filter.map(JobStatus::as_str),
            "kind": kind_filter.map(JobKind::as_str),
        },
        "summary": {
            "by_status": {
                "pending": counts.pending,
                "running": counts.running,
                "completed": counts.completed,
                "failed": counts.failed,
                "dead": counts.dead,
            },
            "by_kind": by_kind,
            "oldest_pending_age_minutes": oldest_pending_age_minutes,
            "stale_jobs_count": stale_jobs_count,
        },
        "jobs": jobs,
    })))
}

fn parse_job_status(input: &str) -> ApiResult<JobStatus> {
    match input.to_ascii_lowercase().as_str() {
        "pending" => Ok(JobStatus::Pending),
        "running" => Ok(JobStatus::Running),
        "completed" => Ok(JobStatus::Completed),
        "failed" => Ok(JobStatus::Failed),
        "dead" => Ok(JobStatus::Dead),
        other => Err(ApiError(Error::Validation(format!(
            "Unknown background job status: {other}"
        )))),
    }
}

fn parse_job_kind(input: &str) -> ApiResult<JobKind> {
    match input.to_ascii_lowercase().as_str() {
        "scan_artifact" => Ok(JobKind::ScanArtifact),
        "index_package" => Ok(JobKind::IndexPackage),
        "deliver_webhook" => Ok(JobKind::DeliverWebhook),
        "cleanup_expired_tokens" => Ok(JobKind::CleanupExpiredTokens),
        "cleanup_oci_blobs" => Ok(JobKind::CleanupOciBlobs),
        "reindex_search" => Ok(JobKind::ReindexSearch),
        other => Err(ApiError(Error::Validation(format!(
            "Unknown background job kind: {other}"
        )))),
    }
}

async fn count_jobs(
    db: &sqlx::PgPool,
    state_filter: Option<JobStatus>,
    kind_filter: Option<JobKind>,
) -> ApiResult<i64> {
    let mut builder = QueryBuilder::<Postgres>::new(
        "SELECT COUNT(*) AS total FROM background_jobs WHERE 1 = 1",
    );

    if let Some(state_filter) = state_filter {
        builder.push(" AND status = ").push_bind(state_filter);
    }

    if let Some(kind_filter) = kind_filter {
        builder.push(" AND kind = ").push_bind(kind_filter);
    }

    let row = builder
        .build()
        .fetch_one(db)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    row.try_get::<i64, _>("total")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))
}

async fn load_jobs(
    db: &sqlx::PgPool,
    state_filter: Option<JobStatus>,
    kind_filter: Option<JobKind>,
    limit: i64,
    offset: i64,
) -> ApiResult<Vec<Job>> {
    let mut builder = QueryBuilder::<Postgres>::new(
        "SELECT \
            id, \
            kind, \
            payload, \
            status, \
            attempts, \
            max_attempts, \
            last_error, \
            scheduled_at, \
            locked_until, \
            locked_by, \
            started_at, \
            completed_at, \
            created_at \
         FROM background_jobs \
         WHERE 1 = 1",
    );

    if let Some(state_filter) = state_filter {
        builder.push(" AND status = ").push_bind(state_filter);
    }

    if let Some(kind_filter) = kind_filter {
        builder.push(" AND kind = ").push_bind(kind_filter);
    }

    builder
        .push(" ORDER BY scheduled_at ASC, created_at DESC LIMIT ")
        .push_bind(limit)
        .push(" OFFSET ")
        .push_bind(offset);

    builder
        .build_query_as::<Job>()
        .fetch_all(db)
        .await
        .map_err(|e| ApiError(Error::Database(e)))
}

async fn load_job_counts_by_kind(db: &sqlx::PgPool) -> ApiResult<serde_json::Value> {
    let rows = sqlx::query_as::<_, JobKindCountRow>(
        "SELECT kind, COUNT(*) AS count \
         FROM background_jobs \
         GROUP BY kind \
         ORDER BY kind",
    )
    .fetch_all(db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    let mut object = serde_json::Map::new();
    for row in rows {
        object.insert(row.kind.as_str().to_owned(), serde_json::json!(row.count));
    }

    Ok(serde_json::Value::Object(object))
}

async fn oldest_pending_age_minutes(db: &sqlx::PgPool) -> ApiResult<Option<i64>> {
    let row = sqlx::query(
        "SELECT CASE \
             WHEN MIN(scheduled_at) IS NULL THEN NULL \
             ELSE FLOOR(EXTRACT(EPOCH FROM (NOW() - MIN(scheduled_at))) / 60)::BIGINT \
         END AS oldest_pending_age_minutes \
         FROM background_jobs \
         WHERE status = 'pending' AND scheduled_at <= NOW()",
    )
    .fetch_one(db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    row.try_get::<Option<i64>, _>("oldest_pending_age_minutes")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))
}

async fn stale_jobs_count(db: &sqlx::PgPool) -> ApiResult<i64> {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) \
         FROM background_jobs \
         WHERE status = 'running' AND locked_until < NOW()",
    )
    .fetch_one(db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))
}

#[cfg(test)]
mod tests {
    use super::{parse_job_kind, parse_job_status};
    use publaryn_workers::queue::{JobKind, JobStatus};

    #[test]
    fn parses_known_job_status_values() {
        assert_eq!(parse_job_status("pending").unwrap(), JobStatus::Pending);
        assert_eq!(parse_job_status("dead").unwrap(), JobStatus::Dead);
    }

    #[test]
    fn rejects_unknown_job_status_values() {
        let error = parse_job_status("mysterious").expect_err("unknown job status must fail");
        assert_eq!(
            error.0.to_string(),
            "Validation error: Unknown background job status: mysterious"
        );
    }

    #[test]
    fn parses_known_job_kind_values() {
        assert_eq!(parse_job_kind("scan_artifact").unwrap(), JobKind::ScanArtifact);
        assert_eq!(parse_job_kind("cleanup_oci_blobs").unwrap(), JobKind::CleanupOciBlobs);
    }
}
