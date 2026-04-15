use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// Kind of token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "token_kind", rename_all = "snake_case")]
pub enum TokenKind {
    /// Personal access token for a user.
    Personal,
    /// Organization-level automation token.
    OrgAutomation,
    /// Scoped to a single repository.
    Repository,
    /// Scoped to a single package.
    Package,
    /// Ephemeral CI token (short-lived).
    Ci,
    /// One-time publish token.
    Publish,
    /// OIDC-derived token (trusted publishing).
    OidcDerived,
}

/// Permissions a token grants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "token_scope", rename_all = "snake_case")]
pub enum TokenScope {
    ReadPackages,
    WritePackages,
    DeletePackages,
    ReadOrg,
    WriteOrg,
    ManageTokens,
    AuditRead,
    SecurityRead,
    SecurityWrite,
    Admin,
}

/// An API token that grants access to the platform.
///
/// The actual token value is only ever returned once (at creation time) and
/// is stored as a hashed value (`token_hash`).
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Token {
    pub id: Uuid,
    pub kind: TokenKind,
    /// Prefix shown in the UI (e.g. `pub_`).
    pub prefix: String,
    /// SHA-256 hash of the raw token value.
    pub token_hash: String,
    /// Human-readable name for the token.
    pub name: String,
    pub user_id: Option<Uuid>,
    pub org_id: Option<Uuid>,
    pub package_id: Option<Uuid>,
    pub repository_id: Option<Uuid>,
    pub scopes: Vec<String>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub is_revoked: bool,
    pub created_at: DateTime<Utc>,
}

impl Token {
    pub fn is_expired(&self) -> bool {
        if let Some(exp) = self.expires_at {
            Utc::now() >= exp
        } else {
            false
        }
    }

    pub fn is_valid(&self) -> bool {
        !self.is_revoked && !self.is_expired()
    }
}
