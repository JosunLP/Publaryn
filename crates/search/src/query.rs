use serde::{Deserialize, Serialize};

use publaryn_core::domain::namespace::Ecosystem;

use super::index::PackageDocument;

/// A search query for packages.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SearchQuery {
    pub q: String,
    pub ecosystem: Option<Ecosystem>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

/// Paginated search results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResults {
    pub total: u64,
    pub offset: u32,
    pub limit: u32,
    pub hits: Vec<PackageDocument>,
}
