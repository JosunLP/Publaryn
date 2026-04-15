use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use serde::Deserialize;

use publaryn_core::error::Error;
use publaryn_search::query::SearchQuery;

use crate::{
    error::{ApiError, ApiResult},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/search", get(search_packages))
}

#[derive(Debug, Deserialize)]
struct SearchQueryParams {
    q: Option<String>,
    ecosystem: Option<String>,
    page: Option<u32>,
    per_page: Option<u32>,
}

async fn search_packages(
    State(state): State<AppState>,
    Query(params): Query<SearchQueryParams>,
) -> ApiResult<Json<serde_json::Value>> {
    use publaryn_search::SearchIndex;

    let per_page = params.per_page.unwrap_or(20).min(100);
    let page = params.page.unwrap_or(1);
    let offset = (page - 1) * per_page;

    let ecosystem = params
        .ecosystem
        .as_deref()
        .and_then(|e| match e.to_lowercase().as_str() {
            "npm" | "bun" => Some(publaryn_core::domain::namespace::Ecosystem::Npm),
            "pypi" => Some(publaryn_core::domain::namespace::Ecosystem::Pypi),
            "cargo" => Some(publaryn_core::domain::namespace::Ecosystem::Cargo),
            "nuget" => Some(publaryn_core::domain::namespace::Ecosystem::Nuget),
            "rubygems" => Some(publaryn_core::domain::namespace::Ecosystem::Rubygems),
            "maven" => Some(publaryn_core::domain::namespace::Ecosystem::Maven),
            "composer" => Some(publaryn_core::domain::namespace::Ecosystem::Composer),
            "oci" => Some(publaryn_core::domain::namespace::Ecosystem::Oci),
            _ => None,
        });

    let query = SearchQuery {
        q: params.q.unwrap_or_default(),
        ecosystem,
        limit: Some(per_page),
        offset: Some(offset),
    };

    let results = state
        .search
        .search(&query)
        .await
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    Ok(Json(serde_json::json!({
        "total": results.total,
        "page": page,
        "per_page": per_page,
        "packages": results.hits,
    })))
}
