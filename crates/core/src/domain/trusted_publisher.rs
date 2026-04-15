use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// OIDC trusted publishing configuration for a package.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct TrustedPublisher {
    pub id: Uuid,
    pub package_id: Uuid,
    pub issuer: String,
    pub subject: String,
    pub repository: Option<String>,
    pub workflow_ref: Option<String>,
    pub environment: Option<String>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
}

impl TrustedPublisher {
    pub fn new(package_id: Uuid, issuer: String, subject: String, created_by: Uuid) -> Self {
        Self {
            id: Uuid::new_v4(),
            package_id,
            issuer,
            subject,
            repository: None,
            workflow_ref: None,
            environment: None,
            created_by,
            created_at: Utc::now(),
        }
    }
}
