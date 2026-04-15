use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// A team within an organization.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Team {
    pub id: Uuid,
    pub org_id: Uuid,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Team {
    pub fn new(org_id: Uuid, name: String, slug: String) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            org_id,
            name,
            slug,
            description: None,
            created_at: now,
            updated_at: now,
        }
    }
}

/// Membership of a user in a team.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct TeamMembership {
    pub id: Uuid,
    pub team_id: Uuid,
    pub user_id: Uuid,
    pub added_at: DateTime<Utc>,
}

/// Permission a team holds on a resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "team_permission", rename_all = "snake_case")]
pub enum TeamPermission {
    Admin,
    Publish,
    WriteMetadata,
    ReadPrivate,
    SecurityReview,
    TransferOwnership,
}

/// Association between a team and a package.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct TeamPackageAccess {
    pub id: Uuid,
    pub team_id: Uuid,
    pub package_id: Uuid,
    pub permission: TeamPermission,
    pub granted_at: DateTime<Utc>,
}
