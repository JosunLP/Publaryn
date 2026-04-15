use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// Role a user can hold within an organization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "org_role", rename_all = "snake_case")]
pub enum OrgRole {
    Owner,
    Admin,
    Maintainer,
    Publisher,
    SecurityManager,
    Auditor,
    BillingManager,
    Viewer,
}

/// An organization that groups users, teams, and packages.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Organization {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub avatar_url: Option<String>,
    pub website: Option<String>,
    pub email: Option<String>,
    pub is_verified: bool,
    pub verified_domain: Option<String>,
    pub mfa_required: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Organization {
    pub fn new(name: String, slug: String) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            name,
            slug,
            display_name: None,
            description: None,
            avatar_url: None,
            website: None,
            email: None,
            is_verified: false,
            verified_domain: None,
            mfa_required: false,
            created_at: now,
            updated_at: now,
        }
    }
}

/// Membership of a user in an organization.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct OrgMembership {
    pub id: Uuid,
    pub org_id: Uuid,
    pub user_id: Uuid,
    pub role: OrgRole,
    pub invited_by: Option<Uuid>,
    pub joined_at: DateTime<Utc>,
}
