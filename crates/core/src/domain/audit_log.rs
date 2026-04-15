use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// Actions recorded in the audit log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "audit_action", rename_all = "snake_case")]
pub enum AuditAction {
    // Package lifecycle
    PackageCreate,
    PackageDelete,
    PackageTransfer,
    PackageVisibilityChange,
    // Release lifecycle
    ReleasePublish,
    ReleaseYank,
    ReleaseUnyank,
    ReleaseDeprecate,
    // Auth
    UserLogin,
    UserLogout,
    UserRegister,
    MfaEnable,
    MfaDisable,
    // Tokens
    TokenCreate,
    TokenRevoke,
    // Org / Team
    OrgCreate,
    OrgDelete,
    OrgMemberAdd,
    OrgMemberRemove,
    OrgRoleChange,
    TeamCreate,
    TeamDelete,
    TeamMemberAdd,
    TeamMemberRemove,
    // Namespace
    NamespaceClaimCreate,
    NamespaceClaimTransfer,
    // Security
    SecurityFindingCreate,
    SecurityFindingResolve,
    PolicyChange,
    // SSO
    SsoConfigChange,
}

/// An immutable append-only audit log entry.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AuditLog {
    pub id: Uuid,
    pub action: AuditAction,
    pub actor_user_id: Option<Uuid>,
    pub actor_token_id: Option<Uuid>,
    pub target_user_id: Option<Uuid>,
    pub target_org_id: Option<Uuid>,
    pub target_package_id: Option<Uuid>,
    pub target_release_id: Option<Uuid>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    /// Arbitrary structured context data.
    pub metadata: Option<serde_json::Value>,
    pub occurred_at: DateTime<Utc>,
}

impl AuditLog {
    pub fn new(action: AuditAction) -> Self {
        Self {
            id: Uuid::new_v4(),
            action,
            actor_user_id: None,
            actor_token_id: None,
            target_user_id: None,
            target_org_id: None,
            target_package_id: None,
            target_release_id: None,
            ip_address: None,
            user_agent: None,
            metadata: None,
            occurred_at: Utc::now(),
        }
    }
}
