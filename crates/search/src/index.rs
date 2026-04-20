use async_trait::async_trait;
use meilisearch_sdk::client::Client;
use publaryn_core::error::{Error, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use uuid::Uuid;

const PACKAGE_FILTERABLE_ATTRIBUTES: &[&str] = &["ecosystem"];

/// A package document indexed in Meilisearch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageDocument {
    pub id: String,
    pub name: String,
    pub normalized_name: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub ecosystem: String,
    pub keywords: Vec<String>,
    pub latest_version: Option<String>,
    pub download_count: i64,
    pub is_deprecated: bool,
    pub visibility: String,
    pub owner_name: Option<String>,
    pub repository_name: Option<String>,
    pub repository_slug: Option<String>,
    pub updated_at: String,
}

impl PackageDocument {
    pub fn index_name() -> &'static str {
        "packages"
    }
}

/// Trait for the search index backend.
#[async_trait]
pub trait SearchIndex: Send + Sync {
    async fn index_package(&self, doc: PackageDocument) -> Result<()>;
    async fn remove_package(&self, package_id: Uuid) -> Result<()>;
    async fn search(
        &self,
        query: &super::query::SearchQuery,
    ) -> Result<super::query::SearchResults>;
}

/// Meilisearch-backed search index implementation.
pub struct MeilisearchIndex {
    client: Client,
    search_settings_ready: Mutex<bool>,
}

impl MeilisearchIndex {
    pub fn new(url: &str, api_key: Option<&str>) -> Self {
        let client = Client::new(url, api_key).expect("Failed to create Meilisearch client");
        Self {
            client,
            search_settings_ready: Mutex::new(false),
        }
    }

    async fn ensure_package_search_settings(&self) -> Result<()> {
        let mut ready = self.search_settings_ready.lock().await;
        if *ready {
            return Ok(());
        }

        let index = self.client.index(PackageDocument::index_name());
        let existing = index.get_filterable_attributes().await.unwrap_or_default();
        let missing = PACKAGE_FILTERABLE_ATTRIBUTES
            .iter()
            .any(|attribute| !existing.iter().any(|current| current == attribute));

        if missing {
            index
                .set_filterable_attributes(PACKAGE_FILTERABLE_ATTRIBUTES)
                .await
                .map_err(|e| Error::Internal(format!("Meilisearch settings error: {e}")))?
                .wait_for_completion(&self.client, None, None)
                .await
                .map_err(|e| Error::Internal(format!("Meilisearch settings error: {e}")))?;
        }

        *ready = true;
        Ok(())
    }
}

#[async_trait]
impl SearchIndex for MeilisearchIndex {
    async fn index_package(&self, doc: PackageDocument) -> Result<()> {
        self.client
            .index(PackageDocument::index_name())
            .add_or_replace(&[doc], Some("id"))
            .await
            .map_err(|e| Error::Internal(format!("Meilisearch index error: {e}")))?
            .wait_for_completion(&self.client, None, None)
            .await
            .map_err(|e| Error::Internal(format!("Meilisearch index error: {e}")))?;
        self.ensure_package_search_settings().await?;
        Ok(())
    }

    async fn remove_package(&self, package_id: Uuid) -> Result<()> {
        self.client
            .index(PackageDocument::index_name())
            .delete_document(package_id.to_string())
            .await
            .map_err(|e| Error::Internal(format!("Meilisearch delete error: {e}")))?
            .wait_for_completion(&self.client, None, None)
            .await
            .map_err(|e| Error::Internal(format!("Meilisearch delete error: {e}")))?;
        Ok(())
    }

    async fn search(
        &self,
        query: &super::query::SearchQuery,
    ) -> Result<super::query::SearchResults> {
        let index = self.client.index(PackageDocument::index_name());
        self.ensure_package_search_settings().await?;
        let per_page = query.limit.unwrap_or(20) as usize;
        let offset = query.offset.unwrap_or(0) as usize;

        let mut builder = index.search();
        builder.with_query(&query.q);
        builder.with_limit(per_page);
        builder.with_offset(offset);

        let filter_str;
        if let Some(eco) = &query.ecosystem {
            filter_str = format!("ecosystem = \"{}\"", eco.as_str());
            builder.with_filter(&filter_str);
        }

        let results = builder
            .execute::<PackageDocument>()
            .await
            .map_err(|e| Error::Internal(format!("Meilisearch search error: {e}")))?;

        Ok(super::query::SearchResults {
            total: results.estimated_total_hits.unwrap_or(0) as u64,
            hits: results.hits.into_iter().map(|h| h.result).collect(),
            offset: query.offset.unwrap_or(0),
            limit: query.limit.unwrap_or(20),
        })
    }
}
