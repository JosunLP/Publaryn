use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use super::namespace::Ecosystem;

/// A mutable reference/alias pointing to a release (e.g. npm dist-tag, OCI tag).
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ChannelRef {
    pub id: Uuid,
    pub package_id: Uuid,
    pub ecosystem: Ecosystem,
    /// Tag name (e.g. "latest", "stable", "beta", "next").
    pub name: String,
    /// The release this tag currently points to.
    pub release_id: Uuid,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ChannelRef {
    pub fn new(
        package_id: Uuid,
        ecosystem: Ecosystem,
        name: String,
        release_id: Uuid,
        created_by: Uuid,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            package_id,
            ecosystem,
            name,
            release_id,
            created_by,
            created_at: now,
            updated_at: now,
        }
    }
}
