use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::str::FromStr;
use uuid::Uuid;

use crate::error::Error;

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

impl TeamPermission {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Publish => "publish",
            Self::WriteMetadata => "write_metadata",
            Self::ReadPrivate => "read_private",
            Self::SecurityReview => "security_review",
            Self::TransferOwnership => "transfer_ownership",
        }
    }
}

impl FromStr for TeamPermission {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "admin" => Ok(Self::Admin),
            "publish" => Ok(Self::Publish),
            "write_metadata" | "write-metadata" => Ok(Self::WriteMetadata),
            "read_private" | "read-private" => Ok(Self::ReadPrivate),
            "security_review" | "security-review" => Ok(Self::SecurityReview),
            "transfer_ownership" | "transfer-ownership" => Ok(Self::TransferOwnership),
            other => Err(Error::Validation(format!(
                "Unknown team permission: {other}"
            ))),
        }
    }
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

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::TeamPermission;

    #[test]
    fn team_permission_round_trips_to_wire_value() {
        assert_eq!(TeamPermission::Publish.as_str(), "publish");
        assert_eq!(TeamPermission::WriteMetadata.as_str(), "write_metadata");
        assert_eq!(TeamPermission::SecurityReview.as_str(), "security_review");
    }

    #[test]
    fn team_permission_accepts_hyphenated_input() {
        let permission = TeamPermission::from_str("transfer-ownership")
            .expect("hyphenated permissions should parse");

        assert_eq!(permission, TeamPermission::TransferOwnership);
    }

    #[test]
    fn team_permission_rejects_unknown_values() {
        let error = TeamPermission::from_str("please-no")
            .expect_err("unknown permissions must be rejected");

        assert_eq!(
            error.to_string(),
            "Validation error: Unknown team permission: please-no"
        );
    }
}
