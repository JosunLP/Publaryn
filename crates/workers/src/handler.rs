//! Job handler trait — implement for each job kind.

use async_trait::async_trait;
use serde_json::Value;

/// Implement this trait to define processing logic for a specific job kind.
///
/// Handlers should be idempotent — the same job may be delivered more than
/// once if a worker crashes while processing it.
#[async_trait]
pub trait JobHandler: Send + Sync {
    /// Process a job with the given JSON payload.
    ///
    /// Return `Ok(())` on success or `Err(message)` on failure.
    /// Failures trigger retries up to `max_attempts`.
    async fn handle(&self, payload: Value) -> Result<(), String>;
}
