//! Bridge between the API crate's `AppState` and the PyPI adapter's
//! `PyPiAppState` trait, keeping the adapter free from circular dependencies.

use bytes::Bytes;
use publaryn_core::error::Error;
use sqlx::PgPool;

use publaryn_adapter_pypi::routes::{PyPiAppState, StoredObject};

use crate::state::AppState;
use crate::storage::PutArtifactObject;

impl PyPiAppState for AppState {
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
            bytes: Bytes::from(object.bytes.to_vec()),
        }))
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
