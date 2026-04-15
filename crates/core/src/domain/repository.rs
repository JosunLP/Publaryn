use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// Visibility level for repositories and packages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "visibility", rename_all = "snake_case")]
pub enum Visibility {
    Public,
    Private,
    InternalOrg,
    Unlisted,
    Quarantined,
}

/// Kind of repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "repository_kind", rename_all = "snake_case")]
pub enum RepositoryKind {
    /// Publicly visible packages.
    Public,
    /// Access-controlled packages.
    Private,
    /// Staging area; packages promoted to release.
    Staging,
    /// Release repository for promoted packages.
    Release,
    /// Proxies/caches an upstream registry.
    Proxy,
    /// Aggregates multiple repositories.
    Virtual,
}

/// A logical collection of packages, scoped to a user or organization.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Repository {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub kind: RepositoryKind,
    pub visibility: Visibility,
    pub owner_user_id: Option<Uuid>,
    pub owner_org_id: Option<Uuid>,
    /// For proxy repositories: upstream URL.
    pub upstream_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Repository {
    pub fn new(name: String, slug: String, kind: RepositoryKind, visibility: Visibility) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            name,
            slug,
            description: None,
            kind,
            visibility,
            owner_user_id: None,
            owner_org_id: None,
            upstream_url: None,
            created_at: now,
            updated_at: now,
        }
    }
}
