use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use sqlx::Row;
use std::collections::HashSet;
use uuid::Uuid;

use publaryn_core::error::Error;
use publaryn_search::{query::SearchQuery, PackageDocument};

use crate::{
    error::{ApiError, ApiResult},
    request_auth::visibility_is_discoverable,
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

    let discoverable_package_ids = load_discoverable_package_ids(&state, &results.hits).await?;
    let packages: Vec<PackageDocument> = results
        .hits
        .into_iter()
        .filter(|hit| {
            if !visibility_is_discoverable(&hit.visibility) {
                return false;
            }

            Uuid::parse_str(&hit.id)
                .ok()
                .is_some_and(|package_id| discoverable_package_ids.contains(&package_id))
        })
        .collect();
    let total = packages.len() as u64;

    Ok(Json(serde_json::json!({
        "total": total,
        "page": page,
        "per_page": per_page,
        "packages": packages,
    })))
}

async fn load_discoverable_package_ids(
    state: &AppState,
    hits: &[PackageDocument],
) -> ApiResult<HashSet<Uuid>> {
    let package_ids = hits
        .iter()
        .filter_map(|hit| Uuid::parse_str(&hit.id).ok())
        .collect::<Vec<_>>();

    if package_ids.is_empty() {
        return Ok(HashSet::new());
    }

    let rows = sqlx::query(
        "SELECT p.id \
         FROM packages p \
         JOIN repositories r ON r.id = p.repository_id \
         WHERE p.id = ANY($1) \
           AND p.visibility = 'public' \
           AND r.visibility = 'public'",
    )
    .bind(&package_ids)
    .fetch_all(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(rows
        .into_iter()
        .filter_map(|row| row.try_get::<Uuid, _>("id").ok())
        .collect())
}
