use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// Publication status of a release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "release_status", rename_all = "snake_case")]
pub enum ReleaseStatus {
    /// Uploaded and awaiting scanning/validation.
    Quarantine,
    /// Actively being scanned.
    Scanning,
    /// Published and visible.
    Published,
    /// Deprecated — use a newer version.
    Deprecated,
    /// Yanked — hidden from default listings, kept for reproducibility.
    Yanked,
    /// Permanently deleted (hard delete).
    Deleted,
}

/// A versioned, immutable release of a package.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Release {
    pub id: Uuid,
    pub package_id: Uuid,
    pub version: String,
    pub status: ReleaseStatus,
    pub published_by: Uuid,
    pub description: Option<String>,
    pub changelog: Option<String>,
    pub is_prerelease: bool,
    pub is_yanked: bool,
    pub yank_reason: Option<String>,
    pub is_deprecated: bool,
    pub deprecation_message: Option<String>,
    /// Upstream source VCS ref (commit SHA, tag, etc.)
    pub source_ref: Option<String>,
    /// Build provenance attestation JSON.
    pub provenance: Option<serde_json::Value>,
    pub published_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Release {
    pub fn new(package_id: Uuid, version: String, published_by: Uuid) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            package_id,
            version,
            status: ReleaseStatus::Quarantine,
            published_by,
            description: None,
            changelog: None,
            is_prerelease: false,
            is_yanked: false,
            yank_reason: None,
            is_deprecated: false,
            deprecation_message: None,
            source_ref: None,
            provenance: None,
            published_at: now,
            updated_at: now,
        }
    }
}
