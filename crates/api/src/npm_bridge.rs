//! Bridge between the API crate's `AppState` and the npm adapter's
//! `NpmAppState` trait, keeping the adapter free from circular dependencies.

use bytes::Bytes;
use publaryn_core::error::Error;
use sqlx::PgPool;

use publaryn_adapter_npm::routes::{NpmAppState, NpmSearchHit, StoredObject};

use crate::state::AppState;
use crate::storage::PutArtifactObject;

impl NpmAppState for AppState {
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
        let obj = self.artifact_store.get_object(key).await?;
        Ok(obj.map(|o| StoredObject {
            content_type: o.content_type,
            bytes: o.bytes,
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

    async fn search_packages(
        &self,
        query: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<NpmSearchHit>, Error> {
        use publaryn_search::SearchIndex;

        let search_query = publaryn_search::query::SearchQuery {
            q: query.to_owned(),
            ecosystem: Some(publaryn_core::domain::namespace::Ecosystem::Npm),
            limit: Some(limit),
            offset: Some(offset),
        };

        let results = self
            .search
            .search(&search_query)
            .await
            .map_err(|e| Error::Internal(e.to_string()))?;

        Ok(results
            .hits
            .into_iter()
            .filter(|hit| hit.visibility == "public")
            .map(|hit| NpmSearchHit {
                name: hit.name,
                description: hit.description,
                keywords: hit.keywords,
                version: hit.latest_version,
                date: Some(hit.updated_at),
            })
            .collect())
    }
}
