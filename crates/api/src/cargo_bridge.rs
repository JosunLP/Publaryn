//! Bridge between the API crate's `AppState` and the Cargo adapter's
//! `CargoAppState` trait, keeping the adapter free from circular dependencies.

use bytes::Bytes;
use publaryn_core::error::Error;
use publaryn_search::query::SearchQuery;
use sqlx::PgPool;
use uuid::Uuid;

use publaryn_adapter_cargo_registry::routes::{
    CargoAppState, CargoSearchHit, CargoSearchResults, StoredObject,
};

use crate::routes::search::{load_visible_search_page, search_batch_size, SearchScopeFilters};
use crate::state::AppState;
use crate::storage::PutArtifactObject;

impl CargoAppState for AppState {
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

    async fn reindex_package_document(&self, package_id: Uuid) -> Result<(), Error> {
        crate::package_search::reindex_package_document(&self.db, self.search.as_ref(), package_id)
            .await
    }

    async fn search_crates(
        &self,
        query: &str,
        per_page: u32,
        offset: u32,
        actor_user_id: Option<Uuid>,
    ) -> Result<CargoSearchResults, Error> {
        let page = offset.saturating_div(per_page.max(1)).saturating_add(1);
        let search_query = SearchQuery {
            q: query.to_owned(),
            ecosystem: Some(publaryn_core::domain::namespace::Ecosystem::Cargo),
            limit: Some(search_batch_size(per_page)),
            offset: Some(0),
        };

        let visible_page = load_visible_search_page(
            self,
            self.search.as_ref(),
            &search_query,
            actor_user_id,
            SearchScopeFilters::default(),
            page,
            per_page,
        )
        .await
        .map_err(|e| e.0)?;

        Ok(CargoSearchResults {
            total: visible_page.total,
            hits: visible_page
                .packages
                .into_iter()
                .map(|hit| CargoSearchHit {
                    name: hit.name,
                    max_version: hit.latest_version.unwrap_or_default(),
                    description: hit.description,
                })
                .collect(),
        })
    }
}
