use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Supported package ecosystems.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "ecosystem", rename_all = "snake_case")]
pub enum Ecosystem {
    Npm,
    /// Bun uses the npm registry adapter.
    Bun,
    Pypi,
    Composer,
    Nuget,
    Rubygems,
    Maven,
    Oci,
    Cargo,
}

impl Ecosystem {
    /// Returns the canonical string label for the ecosystem.
    pub fn as_str(&self) -> &'static str {
        match self {
            Ecosystem::Npm => "npm",
            Ecosystem::Bun => "bun",
            Ecosystem::Pypi => "pypi",
            Ecosystem::Composer => "composer",
            Ecosystem::Nuget => "nuget",
            Ecosystem::Rubygems => "rubygems",
            Ecosystem::Maven => "maven",
            Ecosystem::Oci => "oci",
            Ecosystem::Cargo => "cargo",
        }
    }

    /// Whether this ecosystem reuses another's wire protocol.
    pub fn protocol_family(&self) -> &'static str {
        match self {
            Ecosystem::Bun => "npm",
            other => other.as_str(),
        }
    }
}

impl std::fmt::Display for Ecosystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A namespace claim that links an org (or user) to an ecosystem namespace.
///
/// Examples:
/// - npm `@acme`
/// - Maven `com.acme`
/// - PyPI prefix `acme-`
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct NamespaceClaim {
    pub id: uuid::Uuid,
    pub ecosystem: Ecosystem,
    /// The raw namespace string (e.g. `@acme`, `com.acme`, `acme-`).
    pub namespace: String,
    /// Owner — either a user ID or an org ID.
    pub owner_user_id: Option<Uuid>,
    pub owner_org_id: Option<Uuid>,
    pub is_verified: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}
