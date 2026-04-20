//! Bridge between the API crate's `AppState` and the npm adapter's
//! `NpmAppState` trait, keeping the adapter free from circular dependencies.

use bytes::Bytes;
use publaryn_core::error::Error;
use sqlx::PgPool;

use publaryn_adapter_npm::routes::{NpmAppState, NpmSearchHit, NpmSearchResults, StoredObject};

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
        actor_user_id: Option<uuid::Uuid>,
    ) -> Result<NpmSearchResults, Error> {
        let search_query = publaryn_search::query::SearchQuery {
            q: query.to_owned(),
            ecosystem: Some(publaryn_core::domain::namespace::Ecosystem::Npm),
            limit: Some(crate::routes::search::search_batch_size(limit)),
            offset: Some(0),
        };

        let results = crate::routes::search::load_visible_search_window(
            self,
            self.search.as_ref(),
            &search_query,
            actor_user_id,
            None,
            None,
            offset as usize,
            limit as usize,
        )
        .await
        .map_err(|err| err.0)?;

        Ok(NpmSearchResults {
            total: results.total,
            hits: results
                .packages
                .into_iter()
                .map(|hit| NpmSearchHit {
                    name: hit.name,
                    description: hit.description,
                    keywords: hit.keywords,
                    version: hit.latest_version,
                    date: Some(hit.updated_at),
                })
                .collect(),
        })
    }
}
