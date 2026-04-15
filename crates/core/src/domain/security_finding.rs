use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// Severity level of a security finding.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "security_severity", rename_all = "snake_case")]
pub enum SecuritySeverity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

/// Kind of security finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "finding_kind", rename_all = "snake_case")]
pub enum FindingKind {
    Vulnerability,
    Malware,
    PolicyViolation,
    SecretsExposed,
    SuspiciousInstallHook,
    ArchiveBomb,
    FileTypeAnomaly,
    DependencyConfusion,
}

/// A security finding associated with a release or artifact.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SecurityFinding {
    pub id: Uuid,
    pub release_id: Uuid,
    pub artifact_id: Option<Uuid>,
    pub kind: FindingKind,
    pub severity: SecuritySeverity,
    pub title: String,
    pub description: Option<String>,
    /// External advisory ID (CVE, GHSA, OSV, etc.)
    pub advisory_id: Option<String>,
    pub is_resolved: bool,
    pub resolved_at: Option<DateTime<Utc>>,
    pub resolved_by: Option<Uuid>,
    pub detected_at: DateTime<Utc>,
}

impl SecurityFinding {
    pub fn new(
        release_id: Uuid,
        kind: FindingKind,
        severity: SecuritySeverity,
        title: String,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            release_id,
            artifact_id: None,
            kind,
            severity,
            title,
            description: None,
            advisory_id: None,
            is_resolved: false,
            resolved_at: None,
            resolved_by: None,
            detected_at: Utc::now(),
        }
    }
}
