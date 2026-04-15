use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::error::{Error, Result};

use super::organization::OrgRole;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrganizationInvitationStatus {
    Pending,
    Accepted,
    Declined,
    Revoked,
    Expired,
}

impl OrganizationInvitationStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Accepted => "accepted",
            Self::Declined => "declined",
            Self::Revoked => "revoked",
            Self::Expired => "expired",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct OrganizationInvitation {
    pub id: Uuid,
    pub org_id: Uuid,
    pub invited_user_id: Uuid,
    pub role: OrgRole,
    pub invited_by: Uuid,
    pub accepted_by: Option<Uuid>,
    pub accepted_at: Option<DateTime<Utc>>,
    pub declined_by: Option<Uuid>,
    pub declined_at: Option<DateTime<Utc>>,
    pub revoked_by: Option<Uuid>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

impl OrganizationInvitation {
    pub fn new(
        org_id: Uuid,
        invited_user_id: Uuid,
        role: OrgRole,
        invited_by: Uuid,
        expires_at: DateTime<Utc>,
    ) -> Result<Self> {
        let now = Utc::now();
        if expires_at <= now {
            return Err(Error::Validation(
                "Invitation expiry must be set in the future".into(),
            ));
        }

        Ok(Self {
            id: Uuid::new_v4(),
            org_id,
            invited_user_id,
            role,
            invited_by,
            accepted_by: None,
            accepted_at: None,
            declined_by: None,
            declined_at: None,
            revoked_by: None,
            revoked_at: None,
            expires_at,
            created_at: now,
        })
    }

    pub fn status_at(&self, now: DateTime<Utc>) -> OrganizationInvitationStatus {
        if self.accepted_at.is_some() {
            return OrganizationInvitationStatus::Accepted;
        }

        if self.declined_at.is_some() {
            return OrganizationInvitationStatus::Declined;
        }

        if self.revoked_at.is_some() {
            return OrganizationInvitationStatus::Revoked;
        }

        if self.expires_at <= now {
            return OrganizationInvitationStatus::Expired;
        }

        OrganizationInvitationStatus::Pending
    }

    pub fn is_actionable_at(&self, now: DateTime<Utc>) -> bool {
        matches!(self.status_at(now), OrganizationInvitationStatus::Pending)
    }
}
