use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use sqlx::Row;
use std::collections::HashSet;
use uuid::Uuid;

use publaryn_core::{error::Error, validation};
use publaryn_search::{query::SearchQuery, PackageDocument};

use crate::{
    error::{ApiError, ApiResult},
    request_auth::OptionalAuthenticatedIdentity,
    routes::parse_ecosystem,
    state::AppState,
};

pub(crate) const DEFAULT_SEARCH_PER_PAGE: u32 = 20;
pub(crate) const MAX_SEARCH_PER_PAGE: u32 = 100;
pub(crate) const MIN_SEARCH_BATCH_SIZE: u32 = 50;
pub(crate) const MAX_SEARCH_BATCH_SIZE: u32 = 100;

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/search", get(search_packages))
}

#[derive(Debug, Deserialize)]
struct SearchQueryParams {
    q: Option<String>,
    ecosystem: Option<String>,
    org: Option<String>,
    page: Option<u32>,
    per_page: Option<u32>,
}

async fn search_packages(
    State(state): State<AppState>,
    identity: OptionalAuthenticatedIdentity,
    Query(params): Query<SearchQueryParams>,
) -> ApiResult<Json<serde_json::Value>> {
    let per_page = params
        .per_page
        .unwrap_or(DEFAULT_SEARCH_PER_PAGE)
        .min(MAX_SEARCH_PER_PAGE);
    let page = params.page.unwrap_or(1);
    let batch_size = search_batch_size(per_page);

    let query = SearchQuery {
        q: params.q.unwrap_or_default(),
        ecosystem: params
            .ecosystem
            .as_deref()
            .map(parse_ecosystem)
            .transpose()?,
        limit: Some(batch_size),
        offset: Some(0),
    };
    let owner_org_slug = normalize_search_org_slug(params.org)?;

    let results = load_visible_search_page(
        &state,
        state.search.as_ref(),
        &query,
        identity.user_id(),
        owner_org_slug.as_deref(),
        page,
        per_page,
    )
    .await?;

    Ok(Json(serde_json::json!({
        "total": results.total,
        "page": page,
        "per_page": per_page,
        "packages": results.packages,
    })))
}

pub(crate) struct VisibleSearchPage {
    pub(crate) total: u64,
    pub(crate) packages: Vec<PackageDocument>,
}

pub(crate) async fn load_visible_search_page(
    state: &AppState,
    search: &(dyn publaryn_search::SearchIndex + Send + Sync),
    query: &SearchQuery,
    actor_user_id: Option<Uuid>,
    owner_org_slug: Option<&str>,
    page: u32,
    per_page: u32,
) -> ApiResult<VisibleSearchPage> {
    load_visible_search_window(
        state,
        search,
        query,
        actor_user_id,
        owner_org_slug,
        page.saturating_sub(1).saturating_mul(per_page) as usize,
        per_page as usize,
    )
    .await
}

pub(crate) async fn load_visible_search_window(
    state: &AppState,
    search: &(dyn publaryn_search::SearchIndex + Send + Sync),
    query: &SearchQuery,
    actor_user_id: Option<Uuid>,
    owner_org_slug: Option<&str>,
    visible_offset: usize,
    page_size: usize,
) -> ApiResult<VisibleSearchPage> {
    let batch_size = query.limit.unwrap_or(20);
    let mut search_offset = query.offset.unwrap_or(0);
    let mut visible_total = 0_u64;
    let mut packages = Vec::with_capacity(page_size);

    loop {
        let batch_query = SearchQuery {
            q: query.q.clone(),
            ecosystem: query.ecosystem.clone(),
            limit: Some(batch_size),
            offset: Some(search_offset),
        };
        let results = search
            .search(&batch_query)
            .await
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
        let hit_count = results.hits.len();

        if hit_count == 0 {
            break;
        }

        let visible_hits =
            filter_visible_search_hits(state, results.hits, actor_user_id, owner_org_slug).await?;
        let batch_visible_count = visible_hits.len() as u64;
        let previous_visible_total = visible_total;
        visible_total += batch_visible_count;

        if should_collect_batch(page_size, packages.len(), visible_total, visible_offset) {
            let skip = visible_offset.saturating_sub(previous_visible_total as usize);
            packages.extend(
                visible_hits
                    .into_iter()
                    .skip(skip)
                    .take(page_size - packages.len()),
            );
        }

        search_offset = search_offset.saturating_add(hit_count as u32);
        if hit_count < batch_size as usize {
            break;
        }
    }

    Ok(VisibleSearchPage {
        total: visible_total,
        packages,
    })
}

pub(crate) fn search_batch_size(per_page: u32) -> u32 {
    per_page.clamp(MIN_SEARCH_BATCH_SIZE, MAX_SEARCH_BATCH_SIZE)
}

fn should_collect_batch(
    page_size: usize,
    collected_count: usize,
    visible_total: u64,
    visible_offset: usize,
) -> bool {
    collected_count < page_size && visible_total > visible_offset as u64
}

async fn filter_visible_search_hits(
    state: &AppState,
    hits: Vec<PackageDocument>,
    actor_user_id: Option<Uuid>,
    owner_org_slug: Option<&str>,
) -> ApiResult<Vec<PackageDocument>> {
    let visible_package_ids =
        load_visible_search_package_ids(state, &hits, actor_user_id, owner_org_slug).await?;

    Ok(hits
        .into_iter()
        .filter(|hit| {
            Uuid::parse_str(&hit.id)
                .ok()
                .is_some_and(|package_id| visible_package_ids.contains(&package_id))
        })
        .collect())
}

async fn load_visible_search_package_ids(
    state: &AppState,
    hits: &[PackageDocument],
    actor_user_id: Option<Uuid>,
    owner_org_slug: Option<&str>,
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
         LEFT JOIN organizations o ON o.id = p.owner_org_id \
         WHERE p.id = ANY($1) \
            AND ($3 IS NULL OR o.slug = $3) \
            AND p.visibility <> 'unlisted' \
            AND p.visibility <> 'quarantined' \
            AND r.visibility <> 'unlisted' \
           AND r.visibility <> 'quarantined' \
           AND (\
                (p.visibility = 'public' OR (\
                    $2 IS NOT NULL \
                    AND p.visibility IN ('private', 'internal_org') \
                    AND (\
                        p.owner_user_id = $2 \
                        OR EXISTS (\
                            SELECT 1 \
                            FROM org_memberships om \
                            WHERE om.org_id = p.owner_org_id AND om.user_id = $2\
                        )\
                    )\
                )) \
                AND (r.visibility = 'public' OR (\
                    $2 IS NOT NULL \
                    AND r.visibility IN ('private', 'internal_org') \
                    AND (\
                        r.owner_user_id = $2 \
                        OR EXISTS (\
                            SELECT 1 \
                            FROM org_memberships om \
                            WHERE om.org_id = r.owner_org_id AND om.user_id = $2\
                        )\
                    )\
                ))\
           )",
    )
    .bind(&package_ids)
    .bind(actor_user_id)
    .bind(owner_org_slug)
    .fetch_all(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(rows
        .into_iter()
        .filter_map(|row| row.try_get::<Uuid, _>("id").ok())
        .collect())
}

fn normalize_search_org_slug(org: Option<String>) -> ApiResult<Option<String>> {
    let Some(org) = org.map(|value| value.trim().to_owned()) else {
        return Ok(None);
    };

    if org.is_empty() {
        return Ok(None);
    }

    validation::validate_slug(&org).map_err(ApiError::from)?;
    Ok(Some(org))
}

#[cfg(test)]
mod tests {
    use super::{
        load_visible_search_package_ids, load_visible_search_page, normalize_search_org_slug,
    };
    use crate::{config::Config, state::AppState};
    use publaryn_search::{
        query::{SearchQuery, SearchResults},
        PackageDocument, SearchIndex,
    };
    use sqlx::PgPool;
    use std::collections::HashSet;
    use uuid::Uuid;

    use async_trait::async_trait;

    struct StaticSearchIndex {
        hits: Vec<PackageDocument>,
    }

    #[async_trait]
    impl SearchIndex for StaticSearchIndex {
        async fn index_package(&self, _doc: PackageDocument) -> publaryn_core::Result<()> {
            Ok(())
        }

        async fn remove_package(&self, _package_id: Uuid) -> publaryn_core::Result<()> {
            Ok(())
        }

        async fn search(&self, query: &SearchQuery) -> publaryn_core::Result<SearchResults> {
            let offset = query.offset.unwrap_or(0) as usize;
            let limit = query.limit.unwrap_or(20) as usize;
            let hits = self
                .hits
                .iter()
                .skip(offset)
                .take(limit)
                .cloned()
                .collect::<Vec<_>>();

            Ok(SearchResults {
                total: self.hits.len() as u64,
                offset: offset as u32,
                limit: limit as u32,
                hits,
            })
        }
    }

    fn package_doc(id: Uuid, name: &str, visibility: &str) -> PackageDocument {
        PackageDocument {
            id: id.to_string(),
            name: name.to_owned(),
            normalized_name: name.to_owned(),
            display_name: None,
            description: Some(format!("{name} package")),
            ecosystem: "npm".to_owned(),
            keywords: vec![],
            latest_version: Some("1.0.0".to_owned()),
            download_count: 0,
            is_deprecated: false,
            visibility: visibility.to_owned(),
            owner_name: Some("owner".to_owned()),
            updated_at: "2026-01-01T00:00:00Z".to_owned(),
        }
    }

    fn test_state(pool: PgPool) -> AppState {
        let config = Config::test_config("unused://database-url");
        AppState::new_with_pool(pool, config)
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn visible_search_package_ids_respect_actor_visibility(pool: PgPool) {
        let state = test_state(pool.clone());
        let owner_id = Uuid::new_v4();
        let outsider_id = Uuid::new_v4();
        let org_id = Uuid::new_v4();
        let public_repo_id = Uuid::new_v4();
        let private_repo_id = Uuid::new_v4();
        let public_package_id = Uuid::new_v4();
        let private_package_id = Uuid::new_v4();
        let unlisted_package_id = Uuid::new_v4();

        sqlx::query(
            "INSERT INTO users (id, username, email, password_hash, created_at, updated_at) \
             VALUES ($1, 'owner', 'owner@test.dev', 'hash', NOW(), NOW()), \
                    ($2, 'outsider', 'outsider@test.dev', 'hash', NOW(), NOW())",
        )
        .bind(owner_id)
        .bind(outsider_id)
        .execute(&pool)
        .await
        .expect("users should insert");

        sqlx::query(
            "INSERT INTO organizations (id, name, slug, is_verified, mfa_required, created_at, updated_at) \
             VALUES ($1, 'Acme', 'acme', false, false, NOW(), NOW())",
        )
        .bind(org_id)
        .execute(&pool)
        .await
        .expect("org should insert");

        sqlx::query(
            "INSERT INTO org_memberships (id, org_id, user_id, role, invited_by, joined_at) \
             VALUES ($1, $2, $3, 'viewer', NULL, NOW())",
        )
        .bind(Uuid::new_v4())
        .bind(org_id)
        .bind(owner_id)
        .execute(&pool)
        .await
        .expect("membership should insert");

        sqlx::query(
            "INSERT INTO repositories (id, name, slug, kind, visibility, owner_org_id, created_at, updated_at) \
             VALUES ($1, 'Public', 'public', 'public', 'public', $3, NOW(), NOW()), \
                    ($2, 'Private', 'private', 'release', 'private', $3, NOW(), NOW())",
        )
        .bind(public_repo_id)
        .bind(private_repo_id)
        .bind(org_id)
        .execute(&pool)
        .await
        .expect("repositories should insert");

        sqlx::query(
            "INSERT INTO packages (id, ecosystem, name, normalized_name, visibility, repository_id, owner_org_id, created_at, updated_at) \
             VALUES ($1, 'npm', 'public-widget', 'public-widget', 'public', $4, $5, NOW(), NOW()), \
                    ($2, 'npm', 'private-widget', 'private-widget', 'private', $3, $5, NOW(), NOW()), \
                    ($6, 'npm', 'unlisted-widget', 'unlisted-widget', 'unlisted', $4, $5, NOW(), NOW())",
        )
        .bind(public_package_id)
        .bind(private_package_id)
        .bind(private_repo_id)
        .bind(public_repo_id)
        .bind(org_id)
        .bind(unlisted_package_id)
        .execute(&pool)
        .await
        .expect("packages should insert");

        let hits = vec![
            package_doc(public_package_id, "public-widget", "public"),
            package_doc(private_package_id, "private-widget", "private"),
            package_doc(unlisted_package_id, "unlisted-widget", "unlisted"),
        ];

        let anonymous = load_visible_search_package_ids(&state, &hits, None, None)
            .await
            .expect("anonymous ids should load");
        assert_eq!(anonymous, HashSet::from([public_package_id]));

        let org_member = load_visible_search_package_ids(&state, &hits, Some(owner_id), None)
            .await
            .expect("member ids should load");
        assert_eq!(
            org_member,
            HashSet::from([public_package_id, private_package_id])
        );

        let outsider = load_visible_search_package_ids(&state, &hits, Some(outsider_id), None)
            .await
            .expect("outsider ids should load");
        assert_eq!(outsider, HashSet::from([public_package_id]));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn visible_search_page_paginates_over_filtered_hits(pool: PgPool) {
        let state = test_state(pool.clone());
        let user_id = Uuid::new_v4();
        let public_repo_id = Uuid::new_v4();
        let private_repo_id = Uuid::new_v4();
        let public_a_id = Uuid::new_v4();
        let public_b_id = Uuid::new_v4();
        let private_id = Uuid::new_v4();

        sqlx::query(
            "INSERT INTO users (id, username, email, password_hash, created_at, updated_at) \
             VALUES ($1, 'alice', 'alice@test.dev', 'hash', NOW(), NOW())",
        )
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("user should insert");

        sqlx::query(
            "INSERT INTO repositories (id, name, slug, kind, visibility, owner_user_id, created_at, updated_at) \
             VALUES ($1, 'Public', 'public', 'public', 'public', $3, NOW(), NOW()), \
                    ($2, 'Private', 'private', 'release', 'private', $3, NOW(), NOW())",
        )
        .bind(public_repo_id)
        .bind(private_repo_id)
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("repositories should insert");

        sqlx::query(
            "INSERT INTO packages (id, ecosystem, name, normalized_name, visibility, repository_id, owner_user_id, created_at, updated_at) \
             VALUES ($1, 'npm', 'public-a', 'public-a', 'public', $4, $5, NOW(), NOW()), \
                    ($2, 'npm', 'private-a', 'private-a', 'private', $3, $5, NOW(), NOW()), \
                    ($6, 'npm', 'public-b', 'public-b', 'public', $4, $5, NOW(), NOW())",
        )
        .bind(public_a_id)
        .bind(private_id)
        .bind(private_repo_id)
        .bind(public_repo_id)
        .bind(user_id)
        .bind(public_b_id)
        .execute(&pool)
        .await
        .expect("packages should insert");

        let search = StaticSearchIndex {
            hits: vec![
                package_doc(public_a_id, "public-a", "public"),
                package_doc(private_id, "private-a", "private"),
                package_doc(public_b_id, "public-b", "public"),
            ],
        };
        let query = SearchQuery {
            q: "widget".to_owned(),
            ecosystem: None,
            limit: Some(2),
            offset: Some(0),
        };

        let anonymous_page = load_visible_search_page(&state, &search, &query, None, None, 2, 1)
            .await
            .expect("anonymous page should load");
        assert_eq!(anonymous_page.total, 2);
        assert_eq!(anonymous_page.packages.len(), 1);
        assert_eq!(anonymous_page.packages[0].name, "public-b");

        let owner_page =
            load_visible_search_page(&state, &search, &query, Some(user_id), None, 2, 1)
                .await
                .expect("owner page should load");
        assert_eq!(owner_page.total, 3);
        assert_eq!(owner_page.packages.len(), 1);
        assert_eq!(owner_page.packages[0].name, "private-a");
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn visible_search_package_ids_can_filter_to_one_owner_org(pool: PgPool) {
        let state = test_state(pool.clone());
        let member_id = Uuid::new_v4();
        let acme_org_id = Uuid::new_v4();
        let beta_org_id = Uuid::new_v4();
        let acme_public_repo_id = Uuid::new_v4();
        let acme_private_repo_id = Uuid::new_v4();
        let beta_public_repo_id = Uuid::new_v4();
        let acme_public_package_id = Uuid::new_v4();
        let acme_private_package_id = Uuid::new_v4();
        let beta_public_package_id = Uuid::new_v4();

        sqlx::query(
            "INSERT INTO users (id, username, email, password_hash, created_at, updated_at) \
             VALUES ($1, 'member', 'member@test.dev', 'hash', NOW(), NOW())",
        )
        .bind(member_id)
        .execute(&pool)
        .await
        .expect("user should insert");

        sqlx::query(
            "INSERT INTO organizations (id, name, slug, is_verified, mfa_required, created_at, updated_at) \
             VALUES ($1, 'Acme', 'acme-search', false, false, NOW(), NOW()), \
                    ($2, 'Beta', 'beta-search', false, false, NOW(), NOW())",
        )
        .bind(acme_org_id)
        .bind(beta_org_id)
        .execute(&pool)
        .await
        .expect("orgs should insert");

        sqlx::query(
            "INSERT INTO org_memberships (id, org_id, user_id, role, invited_by, joined_at) \
             VALUES ($1, $2, $3, 'viewer', NULL, NOW())",
        )
        .bind(Uuid::new_v4())
        .bind(acme_org_id)
        .bind(member_id)
        .execute(&pool)
        .await
        .expect("membership should insert");

        sqlx::query(
            "INSERT INTO repositories (id, name, slug, kind, visibility, owner_org_id, created_at, updated_at) \
             VALUES ($1, 'Acme Public', 'acme-public', 'public', 'public', $4, NOW(), NOW()), \
                    ($2, 'Acme Private', 'acme-private', 'release', 'private', $4, NOW(), NOW()), \
                    ($3, 'Beta Public', 'beta-public', 'public', 'public', $5, NOW(), NOW())",
        )
        .bind(acme_public_repo_id)
        .bind(acme_private_repo_id)
        .bind(beta_public_repo_id)
        .bind(acme_org_id)
        .bind(beta_org_id)
        .execute(&pool)
        .await
        .expect("repositories should insert");

        sqlx::query(
            "INSERT INTO packages (id, ecosystem, name, normalized_name, visibility, repository_id, owner_org_id, created_at, updated_at) \
             VALUES ($1, 'npm', 'acme-public-widget', 'acme-public-widget', 'public', $4, $7, NOW(), NOW()), \
                    ($2, 'npm', 'acme-private-widget', 'acme-private-widget', 'private', $5, $7, NOW(), NOW()), \
                    ($3, 'npm', 'beta-public-widget', 'beta-public-widget', 'public', $6, $8, NOW(), NOW())",
        )
        .bind(acme_public_package_id)
        .bind(acme_private_package_id)
        .bind(beta_public_package_id)
        .bind(acme_public_repo_id)
        .bind(acme_private_repo_id)
        .bind(beta_public_repo_id)
        .bind(acme_org_id)
        .bind(beta_org_id)
        .execute(&pool)
        .await
        .expect("packages should insert");

        let hits = vec![
            package_doc(acme_public_package_id, "acme-public-widget", "public"),
            package_doc(acme_private_package_id, "acme-private-widget", "private"),
            package_doc(beta_public_package_id, "beta-public-widget", "public"),
        ];

        let anonymous = load_visible_search_package_ids(&state, &hits, None, Some("acme-search"))
            .await
            .expect("anonymous ids should load");
        assert_eq!(anonymous, HashSet::from([acme_public_package_id]));

        let member =
            load_visible_search_package_ids(&state, &hits, Some(member_id), Some("acme-search"))
                .await
                .expect("member ids should load");
        assert_eq!(
            member,
            HashSet::from([acme_public_package_id, acme_private_package_id])
        );

        let beta =
            load_visible_search_package_ids(&state, &hits, Some(member_id), Some("beta-search"))
                .await
                .expect("beta ids should load");
        assert_eq!(beta, HashSet::from([beta_public_package_id]));
    }

    #[test]
    fn normalize_search_org_slug_validates_optional_values() {
        assert_eq!(
            normalize_search_org_slug(Some("acme-search".to_owned()))
                .expect("slug should normalize"),
            Some("acme-search".to_owned())
        );
        assert_eq!(
            normalize_search_org_slug(Some("  ".to_owned())).expect("blank slug should clear"),
            None
        );
        assert!(normalize_search_org_slug(Some("Acme Search".to_owned())).is_err());
    }
}
