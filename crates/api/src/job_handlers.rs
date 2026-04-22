use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use publaryn_core::error::{Error, Result};
use publaryn_search::SearchIndex;
use publaryn_workers::{
    handler::JobHandler,
    queue::{self, JobKind},
    scanners::ScanArtifactPayload,
};

use crate::{package_search, storage::ArtifactStore};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReindexSearchPayload {
    pub package_id: Uuid,
}

pub async fn enqueue_package_reindex_job(db: &PgPool, package_id: Uuid) -> Result<Uuid> {
    let payload = serde_json::to_value(ReindexSearchPayload { package_id })
        .map_err(|e| Error::Internal(e.to_string()))?;

    queue::enqueue(db, JobKind::ReindexSearch, payload)
        .await
        .map_err(|e| Error::Internal(e.to_string()))
}

pub async fn enqueue_scan_artifact_job(db: &PgPool, payload: ScanArtifactPayload) -> Result<Uuid> {
    let payload = serde_json::to_value(payload).map_err(|e| Error::Internal(e.to_string()))?;

    queue::enqueue(db, JobKind::ScanArtifact, payload)
        .await
        .map_err(|e| Error::Internal(e.to_string()))
}

pub async fn enqueue_release_scan_jobs(db: &PgPool, release_id: Uuid) -> Result<Vec<Uuid>> {
    let rows = sqlx::query(
        "SELECT a.id AS artifact_id, a.storage_key, a.filename, p.ecosystem \
         FROM artifacts a \
         JOIN releases r ON r.id = a.release_id \
         JOIN packages p ON p.id = r.package_id \
         WHERE a.release_id = $1 \
         ORDER BY a.uploaded_at ASC, a.filename ASC",
    )
    .bind(release_id)
    .fetch_all(db)
    .await
    .map_err(Error::Database)?;

    let mut job_ids = Vec::with_capacity(rows.len());
    for row in rows {
        let payload = ScanArtifactPayload {
            release_id,
            artifact_id: row
                .try_get("artifact_id")
                .map_err(|e| Error::Internal(e.to_string()))?,
            storage_key: row
                .try_get("storage_key")
                .map_err(|e| Error::Internal(e.to_string()))?,
            filename: row
                .try_get("filename")
                .map_err(|e| Error::Internal(e.to_string()))?,
            ecosystem: row
                .try_get("ecosystem")
                .map_err(|e| Error::Internal(e.to_string()))?,
        };

        job_ids.push(enqueue_scan_artifact_job(db, payload).await?);
    }

    Ok(job_ids)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupOciBlobsPayload {
    pub grace_period_hours: i64,
    pub batch_size: i64,
}

pub async fn enqueue_oci_blob_cleanup_job(
    db: &PgPool,
    payload: CleanupOciBlobsPayload,
    scheduled_at: DateTime<Utc>,
) -> Result<Uuid> {
    let payload = serde_json::to_value(payload).map_err(|e| Error::Internal(e.to_string()))?;

    queue::enqueue_at(db, JobKind::CleanupOciBlobs, payload, scheduled_at)
        .await
        .map_err(|e| Error::Internal(e.to_string()))
}

pub struct ReindexSearchHandler {
    pub db: PgPool,
    pub search: Arc<dyn SearchIndex>,
}

pub struct CleanupOciBlobsHandler {
    pub db: PgPool,
    pub artifact_store: Arc<dyn ArtifactStore>,
}

#[derive(Debug, sqlx::FromRow)]
struct OciBlobInventoryRow {
    digest: String,
    storage_key: String,
}

async fn cleanup_oci_blob_batch(
    db: &PgPool,
    artifact_store: &dyn ArtifactStore,
    grace_period_hours: i64,
    batch_size: i64,
) -> std::result::Result<usize, String> {
    let grace_period_hours = grace_period_hours.max(0);
    let batch_size = batch_size.clamp(1, 1000);

    let candidates = sqlx::query_as::<_, OciBlobInventoryRow>(
        "SELECT digest, storage_key \
         FROM oci_blob_inventory obi \
         WHERE obi.last_uploaded_at <= NOW() - ($1 * INTERVAL '1 hour') \
           AND NOT EXISTS (\
               SELECT 1 \
               FROM oci_manifest_references omr \
               JOIN releases rel ON rel.id = omr.release_id \
               WHERE omr.ref_digest = obi.digest \
                 AND omr.ref_kind IN ('config', 'layer') \
                 AND rel.status = 'published'\
           ) \
         ORDER BY obi.last_uploaded_at ASC, obi.digest ASC \
         LIMIT $2",
    )
    .bind(grace_period_hours)
    .bind(batch_size)
    .fetch_all(db)
    .await
    .map_err(|e| format!("Failed to query OCI blob cleanup candidates: {e}"))?;

    for candidate in &candidates {
        artifact_store
            .delete_object(&candidate.storage_key)
            .await
            .map_err(|e| format!("Failed to delete OCI blob {} from storage: {e}", candidate.digest))?;

        sqlx::query("DELETE FROM oci_blob_inventory WHERE digest = $1")
            .bind(&candidate.digest)
            .execute(db)
            .await
            .map_err(|e| format!("Failed to delete OCI blob {} from inventory: {e}", candidate.digest))?;
    }

    Ok(candidates.len())
}

#[async_trait]
impl JobHandler for ReindexSearchHandler {
    async fn handle(&self, payload: serde_json::Value) -> Result<(), String> {
        let payload: ReindexSearchPayload =
            serde_json::from_value(payload).map_err(|e| format!("Invalid reindex payload: {e}"))?;

        package_search::reindex_package_document(&self.db, self.search.as_ref(), payload.package_id)
            .await
            .map_err(|e| e.to_string())
    }
}

#[async_trait]
impl JobHandler for CleanupOciBlobsHandler {
    async fn handle(&self, payload: serde_json::Value) -> Result<(), String> {
        let payload: CleanupOciBlobsPayload = serde_json::from_value(payload)
            .map_err(|e| format!("Invalid OCI blob cleanup payload: {e}"))?;

        let cleaned = cleanup_oci_blob_batch(
            &self.db,
            self.artifact_store.as_ref(),
            payload.grace_period_hours,
            payload.batch_size,
        )
        .await?;

        if cleaned as i64 >= payload.batch_size.clamp(1, 1000) {
            queue::enqueue(
                &self.db,
                JobKind::CleanupOciBlobs,
                serde_json::to_value(&payload)
                    .map_err(|e| format!("Failed to serialize follow-up OCI cleanup payload: {e}"))?,
            )
            .await
            .map_err(|e| format!("Failed to enqueue follow-up OCI cleanup job: {e}"))?;
        }

        Ok(())
    }
}
