use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, QueryBuilder, Row};
use std::collections::BTreeMap;
use utoipa::ToSchema;
use uuid::Uuid;

use publaryn_core::error::Error;
use publaryn_workers::queue::{self, Job, JobKind, JobStatus};

use crate::{
    error::{ApiError, ApiResult},
    request_auth::{ensure_platform_admin, AuthenticatedIdentity},
    scopes::{ensure_scope, SCOPE_AUDIT_READ},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/admin/jobs", get(list_background_jobs))
        .route(
            "/v1/admin/jobs/recover-stale",
            post(recover_stale_background_jobs),
        )
        .route("/v1/admin/jobs/{job_id}/retry", post(retry_background_job))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
enum AdminJobKind {
    ScanArtifact,
    IndexPackage,
    DeliverWebhook,
    CleanupExpiredTokens,
    CleanupOciBlobs,
    ReindexSearch,
}

impl From<JobKind> for AdminJobKind {
    fn from(value: JobKind) -> Self {
        match value {
            JobKind::ScanArtifact => Self::ScanArtifact,
            JobKind::IndexPackage => Self::IndexPackage,
            JobKind::DeliverWebhook => Self::DeliverWebhook,
            JobKind::CleanupExpiredTokens => Self::CleanupExpiredTokens,
            JobKind::CleanupOciBlobs => Self::CleanupOciBlobs,
            JobKind::ReindexSearch => Self::ReindexSearch,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
enum AdminJobStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Dead,
}

impl From<JobStatus> for AdminJobStatus {
    fn from(value: JobStatus) -> Self {
        match value {
            JobStatus::Pending => Self::Pending,
            JobStatus::Running => Self::Running,
            JobStatus::Completed => Self::Completed,
            JobStatus::Failed => Self::Failed,
            JobStatus::Dead => Self::Dead,
        }
    }
}

#[derive(Debug, Deserialize)]
struct AdminJobsQuery {
    state: Option<String>,
    kind: Option<String>,
    page: Option<u32>,
    per_page: Option<u32>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
struct AdminJobsFilters {
    state: Option<AdminJobStatus>,
    kind: Option<AdminJobKind>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
struct AdminJobsByStatusSummary {
    pending: i64,
    running: i64,
    completed: i64,
    failed: i64,
    dead: i64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
struct AdminJobsSummary {
    by_status: AdminJobsByStatusSummary,
    by_kind: BTreeMap<String, i64>,
    oldest_pending_age_minutes: Option<i64>,
    stale_jobs_count: i64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
struct BackgroundJobResponse {
    id: Uuid,
    kind: AdminJobKind,
    #[schema(value_type = Object)]
    payload: serde_json::Value,
    status: AdminJobStatus,
    attempts: i32,
    max_attempts: i32,
    last_error: Option<String>,
    scheduled_at: DateTime<Utc>,
    locked_until: Option<DateTime<Utc>>,
    locked_by: Option<String>,
    started_at: Option<DateTime<Utc>>,
    completed_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    is_stale: bool,
    can_retry: bool,
    recovery_hint: Option<String>,
}

impl From<Job> for BackgroundJobResponse {
    fn from(value: Job) -> Self {
        let is_stale = value.status == JobStatus::Running
            && value
                .locked_until
                .is_some_and(|locked_until| locked_until < Utc::now());
        let can_retry = matches!(value.status, JobStatus::Failed | JobStatus::Dead);
        let recovery_hint = if is_stale {
            Some("Use POST /v1/admin/jobs/recover-stale after confirming the worker lock is abandoned".to_owned())
        } else if can_retry {
            Some(format!(
                "Use POST /v1/admin/jobs/{}/retry after correcting the underlying failure",
                value.id
            ))
        } else {
            None
        };

        Self {
            id: value.id,
            kind: value.kind.into(),
            payload: value.payload,
            status: value.status.into(),
            attempts: value.attempts,
            max_attempts: value.max_attempts,
            last_error: value.last_error,
            scheduled_at: value.scheduled_at,
            locked_until: value.locked_until,
            locked_by: value.locked_by,
            started_at: value.started_at,
            completed_at: value.completed_at,
            created_at: value.created_at,
            is_stale,
            can_retry,
            recovery_hint,
        }
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct AdminJobsResponse {
    page: u32,
    per_page: i64,
    total: i64,
    filters: AdminJobsFilters,
    summary: AdminJobsSummary,
    jobs: Vec<BackgroundJobResponse>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
struct AdminJobRetryResponse {
    message: String,
    job: BackgroundJobResponse,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
struct AdminJobsRecoverStaleResponse {
    message: String,
    recovered_count: u64,
}

#[derive(Debug, sqlx::FromRow)]
struct JobKindCountRow {
    kind: JobKind,
    count: i64,
}

#[utoipa::path(
    get,
    path = "/v1/admin/jobs",
    tag = "admin",
    security(
        ("bearer_auth" = [])
    ),
    params(
        ("state" = Option<String>, Query, description = "Optional background job state filter. Supported values: pending, running, completed, failed, dead."),
        ("kind" = Option<String>, Query, description = "Optional background job kind filter. Supported values: scan_artifact, index_package, deliver_webhook, cleanup_expired_tokens, cleanup_oci_blobs, reindex_search."),
        ("page" = Option<u32>, Query, description = "1-based results page.", minimum = 1),
        ("per_page" = Option<u32>, Query, description = "Results per page.", minimum = 1, maximum = 100),
    ),
    responses(
        (status = 200, description = "Platform-admin background job queue visibility and recovery triage", body = AdminJobsResponse),
        (status = 400, description = "Invalid background job filter parameter"),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 403, description = "Authenticated actor lacks platform-admin access or audit:read scope"),
    )
)]
#[allow(dead_code)]
pub async fn list_background_jobs_doc() {}

async fn list_background_jobs(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Query(query): Query<AdminJobsQuery>,
) -> ApiResult<Json<AdminJobsResponse>> {
    ensure_admin_jobs_access(&state.db, &identity).await?;

    let state_filter = query.state.as_deref().map(parse_job_status).transpose()?;
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

    Ok(Json(AdminJobsResponse {
        page,
        per_page,
        total,
        filters: AdminJobsFilters {
            state: state_filter.map(Into::into),
            kind: kind_filter.map(Into::into),
        },
        summary: AdminJobsSummary {
            by_status: AdminJobsByStatusSummary {
                pending: counts.pending,
                running: counts.running,
                completed: counts.completed,
                failed: counts.failed,
                dead: counts.dead,
            },
            by_kind,
            oldest_pending_age_minutes,
            stale_jobs_count,
        },
        jobs: jobs.into_iter().map(Into::into).collect(),
    }))
}

#[utoipa::path(
    post,
    path = "/v1/admin/jobs/{job_id}/retry",
    tag = "admin",
    security(
        ("bearer_auth" = [])
    ),
    params(
        ("job_id" = Uuid, Path, description = "Background job id to retry. Only failed or dead jobs are eligible."),
    ),
    responses(
        (status = 200, description = "Background job reset to pending for retry", body = AdminJobRetryResponse),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 403, description = "Authenticated actor lacks platform-admin access or audit:read scope"),
        (status = 404, description = "Background job not found"),
        (status = 409, description = "Background job status is not retryable"),
    )
)]
#[allow(dead_code)]
pub async fn retry_background_job_doc() {}

async fn retry_background_job(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path(job_id): Path<Uuid>,
) -> ApiResult<Json<AdminJobRetryResponse>> {
    ensure_admin_jobs_access(&state.db, &identity).await?;

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    let current = sqlx::query(
        "SELECT id, kind, status::text AS status, attempts, max_attempts, last_error \
         FROM background_jobs \
         WHERE id = $1 \
         FOR UPDATE",
    )
    .bind(job_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| {
        ApiError(Error::NotFound(format!(
            "Background job '{job_id}' not found"
        )))
    })?;

    let previous_status = current
        .try_get::<String, _>("status")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    if !matches!(previous_status.as_str(), "failed" | "dead") {
        return Err(ApiError(Error::Conflict(format!(
            "Background job '{job_id}' cannot be retried from status '{previous_status}'"
        ))));
    }

    let job = sqlx::query_as::<_, Job>(
        "UPDATE background_jobs \
         SET status = 'pending'::job_status, \
             attempts = 0, \
             scheduled_at = NOW(), \
             locked_until = NULL, \
             locked_by = NULL, \
             started_at = NULL, \
             completed_at = NULL \
         WHERE id = $1 \
         RETURNING id, kind, payload, status, attempts, max_attempts, last_error, scheduled_at, \
                   locked_until, locked_by, started_at, completed_at, created_at",
    )
    .bind(job_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, metadata, occurred_at) \
         VALUES ($1, 'admin_job_retry', $2, $3, $4, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(serde_json::json!({
        "job_id": job_id,
        "kind": current.try_get::<JobKind, _>("kind").ok().map(|kind| kind.as_str()),
        "previous_status": previous_status,
        "previous_attempts": current.try_get::<i32, _>("attempts").ok(),
        "max_attempts": current.try_get::<i32, _>("max_attempts").ok(),
        "last_error": current.try_get::<Option<String>, _>("last_error").ok().flatten(),
    }))
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    tx.commit()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(Json(AdminJobRetryResponse {
        message: "Background job scheduled for retry".to_owned(),
        job: job.into(),
    }))
}

#[utoipa::path(
    post,
    path = "/v1/admin/jobs/recover-stale",
    tag = "admin",
    security(
        ("bearer_auth" = [])
    ),
    responses(
        (status = 200, description = "Stale running jobs reset to pending", body = AdminJobsRecoverStaleResponse),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 403, description = "Authenticated actor lacks platform-admin access or audit:read scope"),
    )
)]
#[allow(dead_code)]
pub async fn recover_stale_background_jobs_doc() {}

async fn recover_stale_background_jobs(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
) -> ApiResult<Json<AdminJobsRecoverStaleResponse>> {
    ensure_admin_jobs_access(&state.db, &identity).await?;

    let recovered_count = queue::recover_stale_jobs(&state.db)
        .await
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, metadata, occurred_at) \
         VALUES ($1, 'admin_jobs_recover_stale', $2, $3, $4, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(serde_json::json!({
        "recovered_count": recovered_count,
    }))
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(Json(AdminJobsRecoverStaleResponse {
        message: "Stale background jobs recovered".to_owned(),
        recovered_count,
    }))
}

async fn ensure_admin_jobs_access(
    db: &sqlx::PgPool,
    identity: &AuthenticatedIdentity,
) -> ApiResult<()> {
    ensure_scope(identity, SCOPE_AUDIT_READ)?;
    ensure_platform_admin(db, identity.user_id).await
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
    let mut builder =
        QueryBuilder::<Postgres>::new("SELECT COUNT(*) AS total FROM background_jobs WHERE 1 = 1");

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

async fn load_job_counts_by_kind(db: &sqlx::PgPool) -> ApiResult<BTreeMap<String, i64>> {
    let rows = sqlx::query_as::<_, JobKindCountRow>(
        "SELECT kind, COUNT(*) AS count \
         FROM background_jobs \
         GROUP BY kind \
         ORDER BY kind",
    )
    .fetch_all(db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    let mut counts = BTreeMap::new();
    for row in rows {
        counts.insert(row.kind.as_str().to_owned(), row.count);
    }

    Ok(counts)
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
    use super::{parse_job_kind, parse_job_status, BackgroundJobResponse};
    use chrono::{Duration, Utc};
    use publaryn_workers::queue::{JobKind, JobStatus};
    use serde_json::json;
    use uuid::Uuid;

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
        assert_eq!(
            parse_job_kind("scan_artifact").unwrap(),
            JobKind::ScanArtifact
        );
        assert_eq!(
            parse_job_kind("cleanup_oci_blobs").unwrap(),
            JobKind::CleanupOciBlobs
        );
    }

    #[test]
    fn background_job_response_explains_recovery_actions() {
        let stale_job = publaryn_workers::queue::Job {
            id: Uuid::new_v4(),
            kind: JobKind::ScanArtifact,
            payload: json!({ "artifact_id": Uuid::new_v4() }),
            status: JobStatus::Running,
            attempts: 1,
            max_attempts: 5,
            last_error: None,
            scheduled_at: Utc::now() - Duration::minutes(10),
            locked_until: Some(Utc::now() - Duration::minutes(1)),
            locked_by: Some("worker-a".to_owned()),
            started_at: Some(Utc::now() - Duration::minutes(10)),
            completed_at: None,
            created_at: Utc::now() - Duration::minutes(10),
        };
        let stale_response = BackgroundJobResponse::from(stale_job);
        assert!(stale_response.is_stale);
        assert!(!stale_response.can_retry);
        assert!(stale_response
            .recovery_hint
            .as_deref()
            .unwrap_or_default()
            .contains("recover-stale"));

        let dead_job = publaryn_workers::queue::Job {
            id: Uuid::new_v4(),
            kind: JobKind::ReindexSearch,
            payload: json!({ "package_id": Uuid::new_v4() }),
            status: JobStatus::Dead,
            attempts: 5,
            max_attempts: 5,
            last_error: Some("boom".to_owned()),
            scheduled_at: Utc::now() - Duration::minutes(10),
            locked_until: None,
            locked_by: None,
            started_at: Some(Utc::now() - Duration::minutes(10)),
            completed_at: Some(Utc::now() - Duration::minutes(9)),
            created_at: Utc::now() - Duration::minutes(10),
        };
        let dead_response = BackgroundJobResponse::from(dead_job);
        assert!(!dead_response.is_stale);
        assert!(dead_response.can_retry);
        assert!(dead_response
            .recovery_hint
            .as_deref()
            .unwrap_or_default()
            .contains("/retry"));
    }
}
