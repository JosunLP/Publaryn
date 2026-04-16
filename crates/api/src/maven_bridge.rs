//! Bridge between the API crate's `AppState` and the Maven adapter's
//! `MavenAppState` trait.

use publaryn_adapter_maven::routes::{MavenAppState, StoredObject};
use publaryn_core::error::Error;
use sqlx::PgPool;

use crate::state::AppState;

impl MavenAppState for AppState {
    fn db(&self) -> &PgPool {
        &self.db
    }

    async fn artifact_get(&self, key: &str) -> Result<Option<StoredObject>, Error> {
        let object = self.artifact_store.get_object(key).await?;
        Ok(object.map(|object| StoredObject {
            content_type: object.content_type,
            bytes: object.bytes,
        }))
    }

    fn jwt_secret(&self) -> &str {
        &self.config.auth.jwt_secret
    }

    fn jwt_issuer(&self) -> &str {
        &self.config.auth.issuer
    }
}
