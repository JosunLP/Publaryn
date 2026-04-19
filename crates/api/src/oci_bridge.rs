//! Bridge between the API crate's `AppState` and the OCI adapter's
//! `OciAppState` trait.

use bytes::Bytes;
use publaryn_adapter_oci::routes::{OciAppState, StoredObject};
use publaryn_core::error::Error;
use sqlx::PgPool;

use crate::state::AppState;
use crate::storage::PutArtifactObject;

impl OciAppState for AppState {
    fn db(&self) -> &PgPool {
        &self.db
    }

    async fn artifact_put(
        &self,
        key: String,
        content_type: String,
        bytes: Bytes,
    ) -> Result<(), Error> {
        self.artifact_store
            .put_object(PutArtifactObject {
                storage_key: key,
                content_type,
                bytes,
            })
            .await
    }

    async fn artifact_get(&self, key: &str) -> Result<Option<StoredObject>, Error> {
        let object = self.artifact_store.get_object(key).await?;
        Ok(object.map(|object| StoredObject {
            content_type: object.content_type,
            bytes: object.bytes,
        }))
    }

    async fn artifact_delete(&self, key: &str) -> Result<(), Error> {
        self.artifact_store.delete_object(key).await
    }

    fn base_url(&self) -> &str {
        &self.config.server.base_url
    }

    fn jwt_secret(&self) -> &str {
        &self.config.auth.jwt_secret
    }

    fn jwt_issuer(&self) -> &str {
        &self.config.auth.issuer
    }
}
