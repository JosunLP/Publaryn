//! Job queue operations: enqueue, claim, complete, fail, recover stale jobs.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Executor, PgPool, Postgres};
use uuid::Uuid;

/// The kinds of background jobs the system supports.
///
/// Must stay in sync with the `job_kind` enum in migration 010.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "job_kind", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    ScanArtifact,
    IndexPackage,
    DeliverWebhook,
    CleanupExpiredTokens,
    CleanupOciBlobs,
    ReindexSearch,
}

impl JobKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ScanArtifact => "scan_artifact",
            Self::IndexPackage => "index_package",
            Self::DeliverWebhook => "deliver_webhook",
            Self::CleanupExpiredTokens => "cleanup_expired_tokens",
            Self::CleanupOciBlobs => "cleanup_oci_blobs",
            Self::ReindexSearch => "reindex_search",
        }
    }
}

/// Status of a background job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "job_status", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Dead,
}

impl JobStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Dead => "dead",
        }
    }
}

/// A row from the `background_jobs` table.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Job {
    pub id: Uuid,
    pub kind: JobKind,
    pub payload: serde_json::Value,
    pub status: JobStatus,
    pub attempts: i32,
    pub max_attempts: i32,
    pub last_error: Option<String>,
    pub scheduled_at: DateTime<Utc>,
    pub locked_until: Option<DateTime<Utc>>,
    pub locked_by: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Enqueue a new background job for immediate processing.
pub async fn enqueue(
    db: &PgPool,
    kind: JobKind,
    payload: serde_json::Value,
) -> anyhow::Result<Uuid> {
    enqueue_at(db, kind, payload, Utc::now()).await
}

/// Enqueue a background job scheduled for a future time.
pub async fn enqueue_at(
    db: &PgPool,
    kind: JobKind,
    payload: serde_json::Value,
    scheduled_at: DateTime<Utc>,
) -> anyhow::Result<Uuid> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO background_jobs (id, kind, payload, scheduled_at) \
         VALUES ($1, $2, $3, $4)",
    )
    .bind(id)
    .bind(kind)
    .bind(&payload)
    .bind(scheduled_at)
    .execute(db)
    .await?;

    tracing::debug!(job_id = %id, kind = ?kind, "Job enqueued");
    Ok(id)
}

/// Attempt to claim up to `batch_size` pending jobs.
///
/// Uses `FOR UPDATE SKIP LOCKED` so multiple workers can poll concurrently
/// without conflicts. Claimed jobs transition to `running` status and are
/// locked for `lock_duration_seconds`.
pub async fn claim_jobs(
    db: &PgPool,
    worker_id: &str,
    batch_size: i32,
    lock_duration_seconds: i64,
) -> anyhow::Result<Vec<Job>> {
    let now = Utc::now();
    let locked_until = now + chrono::Duration::seconds(lock_duration_seconds);

    let rows = sqlx::query_as::<_, Job>(
        r#"
        UPDATE background_jobs
        SET status = 'running'::job_status,
            attempts = attempts + 1,
            locked_until = $1,
            locked_by = $2,
            started_at = $3
        WHERE id IN (
            SELECT id FROM background_jobs
            WHERE status = 'pending'
              AND scheduled_at <= $3
            ORDER BY scheduled_at ASC
            FOR UPDATE SKIP LOCKED
            LIMIT $4
        )
        RETURNING
            id,
            kind,
            payload,
            status,
            attempts,
            max_attempts,
            last_error,
            scheduled_at,
            locked_until,
            locked_by,
            started_at,
            completed_at,
            created_at
            "#,
    )
    .bind(locked_until)
    .bind(worker_id)
    .bind(now)
    .bind(batch_size as i64)
    .fetch_all(db)
    .await?;

    if !rows.is_empty() {
        tracing::debug!(worker = worker_id, count = rows.len(), "Claimed jobs");
    }

    Ok(rows)
}

/// Mark a job as successfully completed.
pub async fn complete_job(db: &PgPool, job_id: Uuid) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE background_jobs \
         SET status = 'completed', completed_at = NOW(), locked_until = NULL, locked_by = NULL \
         WHERE id = $1",
    )
    .bind(job_id)
    .execute(db)
    .await?;
    Ok(())
}

/// Mark a job as failed. If it has exceeded max_attempts it becomes `dead`;
/// otherwise it returns to `pending` for retry with exponential backoff.
pub async fn fail_job(
    db: &PgPool,
    job_id: Uuid,
    error: &str,
    current_attempts: i32,
    max_attempts: i32,
) -> anyhow::Result<()> {
    if current_attempts >= max_attempts {
        sqlx::query(
            "UPDATE background_jobs \
             SET status = 'dead', last_error = $2, completed_at = NOW(), \
                 locked_until = NULL, locked_by = NULL \
             WHERE id = $1",
        )
        .bind(job_id)
        .bind(error)
        .execute(db)
        .await?;
        tracing::warn!(job_id = %job_id, error = error, "Job dead-lettered after max attempts");
    } else {
        // Exponential backoff: 2^attempts * 10 seconds (10s, 20s, 40s, ...)
        let backoff_seconds = (1i64 << current_attempts) * 10;
        let retry_at = Utc::now() + chrono::Duration::seconds(backoff_seconds);
        sqlx::query(
            "UPDATE background_jobs \
             SET status = 'pending', last_error = $2, scheduled_at = $3, \
                 locked_until = NULL, locked_by = NULL \
             WHERE id = $1",
        )
        .bind(job_id)
        .bind(error)
        .bind(retry_at)
        .execute(db)
        .await?;
        tracing::info!(
            job_id = %job_id,
            attempt = current_attempts,
            retry_at = %retry_at,
            "Job scheduled for retry"
        );
    }
    Ok(())
}

/// Recover stale jobs whose lock has expired without completion.
/// These are likely from crashed workers. They are reset to `pending`.
pub async fn recover_stale_jobs<'e, E>(executor: E) -> sqlx::Result<u64>
where
    E: Executor<'e, Database = Postgres>,
{
    let result = sqlx::query(
        "UPDATE background_jobs \
         SET status = 'pending', locked_until = NULL, locked_by = NULL, started_at = NULL \
         WHERE status = 'running' AND locked_until < NOW()",
    )
    .execute(executor)
    .await?;

    let recovered = result.rows_affected();
    if recovered > 0 {
        tracing::warn!(count = recovered, "Recovered stale background jobs");
    }
    Ok(recovered)
}

/// Delete completed and dead jobs older than the given retention period.
pub async fn cleanup_finished_jobs(db: &PgPool, retention_hours: i64) -> anyhow::Result<u64> {
    let cutoff = Utc::now() - chrono::Duration::hours(retention_hours);
    let result = sqlx::query(
        "DELETE FROM background_jobs \
         WHERE status IN ('completed', 'dead') AND completed_at < $1",
    )
    .bind(cutoff)
    .execute(db)
    .await?;

    let deleted = result.rows_affected();
    if deleted > 0 {
        tracing::info!(count = deleted, "Cleaned up old background jobs");
    }
    Ok(deleted)
}

/// Count jobs by status for metrics/observability.
pub async fn job_counts(db: &PgPool) -> anyhow::Result<JobCounts> {
    let row = sqlx::query_as::<_, JobCounts>(
        r#"
        SELECT
            COALESCE(SUM(CASE WHEN status = 'pending' THEN 1 ELSE 0 END), 0) AS pending,
            COALESCE(SUM(CASE WHEN status = 'running' THEN 1 ELSE 0 END), 0) AS running,
            COALESCE(SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END), 0) AS completed,
            COALESCE(SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END), 0) AS failed,
            COALESCE(SUM(CASE WHEN status = 'dead' THEN 1 ELSE 0 END), 0) AS dead
        FROM background_jobs
        "#,
    )
    .fetch_one(db)
    .await?;

    Ok(row)
}

#[derive(Debug, Clone, Default, sqlx::FromRow)]
pub struct JobCounts {
    pub pending: i64,
    pub running: i64,
    pub completed: i64,
    pub failed: i64,
    pub dead: i64,
}
