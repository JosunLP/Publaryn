use async_trait::async_trait;
use publaryn_core::error::Result;

/// Common trait for all ecosystem protocol adapters.
///
/// Each adapter translates native registry wire protocol requests into
/// calls on the shared domain model (stored in PostgreSQL) and responds
/// using the ecosystem-specific format.
#[async_trait]
pub trait EcosystemAdapter: Send + Sync {
    /// Human-readable name of the ecosystem.
    fn ecosystem_name(&self) -> &'static str;

    /// Validate a raw package name according to ecosystem rules.
    fn validate_name(&self, name: &str) -> Result<()>;

    /// Normalize a package name for storage (de-duplication key).
    fn normalize_name(&self, name: &str) -> String;
}
