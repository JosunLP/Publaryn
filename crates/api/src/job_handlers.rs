use std::sync::Arc;

use async_trait::async_trait;
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

use crate::package_search;

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

pub struct ReindexSearchHandler {
    pub db: PgPool,
    pub search: Arc<dyn SearchIndex>,
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
