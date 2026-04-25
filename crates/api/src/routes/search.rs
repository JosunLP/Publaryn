use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use sqlx::Row;
use std::collections::{HashMap, HashSet};
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
const RELEASE_HISTORY_VISIBLE_STATUSES: &[&str] = &["published", "deprecated", "yanked"];

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/search", get(search_packages))
}

#[derive(Debug, Deserialize)]
struct SearchQueryParams {
    q: Option<String>,
    ecosystem: Option<String>,
    org: Option<String>,
    repository: Option<String>,
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
    let repository_slug = normalize_search_repository_slug(params.repository)?;

    let results = load_visible_search_page(
        &state,
        state.search.as_ref(),
        &query,
        identity.user_id(),
        SearchScopeFilters {
            owner_org_slug: owner_org_slug.as_deref(),
            repository_slug: repository_slug.as_deref(),
        },
        page,
        per_page,
    )
    .await?;
    let packages = build_search_package_responses(&state.db, results.packages).await?;

    Ok(Json(serde_json::json!({
        "total": results.total,
        "page": page,
        "per_page": per_page,
        "packages": packages,
    })))
}

pub(crate) struct VisibleSearchPage {
    pub(crate) total: u64,
    pub(crate) packages: Vec<PackageDocument>,
}

#[derive(Debug, Clone, serde::Serialize, Default, PartialEq, Eq)]
struct SearchPackageDiscoverySummary {
    risk_level: Option<String>,
    unresolved_security_finding_count: i64,
    worst_unresolved_security_severity: Option<String>,
    has_trusted_publisher: bool,
    trusted_publisher_count: i64,
    latest_release_status: Option<String>,
    latest_release_published_at: Option<DateTime<Utc>>,
    signals: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SearchScopeFilters<'a> {
    pub(crate) owner_org_slug: Option<&'a str>,
    pub(crate) repository_slug: Option<&'a str>,
}

pub(crate) async fn load_visible_search_page(
    state: &AppState,
    search: &(dyn publaryn_search::SearchIndex + Send + Sync),
    query: &SearchQuery,
    actor_user_id: Option<Uuid>,
    filters: SearchScopeFilters<'_>,
    page: u32,
    per_page: u32,
) -> ApiResult<VisibleSearchPage> {
    load_visible_search_window(
        state,
        search,
        query,
        actor_user_id,
        filters,
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
    filters: SearchScopeFilters<'_>,
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
            filter_visible_search_hits(state, results.hits, actor_user_id, filters).await?;
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
    filters: SearchScopeFilters<'_>,
) -> ApiResult<Vec<PackageDocument>> {
    let visible_package_ids =
        load_visible_search_package_ids(state, &hits, actor_user_id, filters).await?;

    Ok(hits
        .into_iter()
        .filter(|hit| {
            Uuid::parse_str(&hit.id)
                .ok()
                .is_some_and(|package_id| visible_package_ids.contains(&package_id))
        })
        .collect())
}

async fn build_search_package_responses(
    db: &sqlx::PgPool,
    packages: Vec<PackageDocument>,
) -> ApiResult<Vec<serde_json::Value>> {
    let discovery = load_search_discovery_summaries(db, &packages).await?;
    packages
        .into_iter()
        .map(|package| {
            let package_id = Uuid::parse_str(&package.id).ok();
            let mut value = serde_json::to_value(package).map_err(|error| {
                ApiError(Error::Internal(format!(
                    "search package serialization failed: {error}"
                )))
            })?;

            if let (serde_json::Value::Object(ref mut object), Some(package_id)) =
                (&mut value, package_id)
            {
                let summary = discovery
                    .get(&package_id)
                    .cloned()
                    .unwrap_or_else(|| build_search_discovery_summary(None, None, 0, None, 0));
                object.insert(
                    "discovery".to_owned(),
                    serde_json::to_value(summary).map_err(|error| {
                        ApiError(Error::Internal(format!(
                            "search discovery serialization failed: {error}"
                        )))
                    })?,
                );
            }

            Ok(value)
        })
        .collect()
}

async fn load_search_discovery_summaries(
    db: &sqlx::PgPool,
    packages: &[PackageDocument],
) -> ApiResult<HashMap<Uuid, SearchPackageDiscoverySummary>> {
    let package_ids = packages
        .iter()
        .filter_map(|package| Uuid::parse_str(&package.id).ok())
        .collect::<Vec<_>>();

    if package_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let rows = sqlx::query(
        "WITH selected_packages AS ( \
             SELECT UNNEST($1::uuid[]) AS package_id \
         ), latest_release AS ( \
             SELECT DISTINCT ON (r.package_id) \
                    r.package_id, \
                    r.id AS release_id, \
                    r.status::text AS latest_release_status, \
                    r.published_at AS latest_release_published_at \
             FROM releases r \
             JOIN selected_packages sp ON sp.package_id = r.package_id \
             WHERE r.status::text = ANY($2) \
             ORDER BY r.package_id, r.published_at DESC \
         ), finding_summary AS ( \
             SELECT lr.package_id, \
                    COUNT(sf.id)::BIGINT AS unresolved_security_finding_count, \
                    CASE \
                        WHEN COUNT(sf.id) FILTER (WHERE sf.severity = 'critical'::security_severity) > 0 THEN 'critical' \
                        WHEN COUNT(sf.id) FILTER (WHERE sf.severity = 'high'::security_severity) > 0 THEN 'high' \
                        WHEN COUNT(sf.id) FILTER (WHERE sf.severity = 'medium'::security_severity) > 0 THEN 'medium' \
                        WHEN COUNT(sf.id) FILTER (WHERE sf.severity = 'low'::security_severity) > 0 THEN 'low' \
                        WHEN COUNT(sf.id) FILTER (WHERE sf.severity = 'info'::security_severity) > 0 THEN 'info' \
                        ELSE NULL \
                    END AS worst_unresolved_security_severity \
             FROM latest_release lr \
             LEFT JOIN security_findings sf ON sf.release_id = lr.release_id AND sf.is_resolved = FALSE \
             GROUP BY lr.package_id \
         ), trusted_summary AS ( \
             SELECT package_id, COUNT(*)::BIGINT AS trusted_publisher_count \
             FROM trusted_publishers \
             WHERE package_id = ANY($1::uuid[]) \
             GROUP BY package_id \
         ) \
         SELECT sp.package_id, \
                lr.latest_release_status, \
                lr.latest_release_published_at, \
                COALESCE(fs.unresolved_security_finding_count, 0)::BIGINT AS unresolved_security_finding_count, \
                fs.worst_unresolved_security_severity, \
                COALESCE(ts.trusted_publisher_count, 0)::BIGINT AS trusted_publisher_count \
         FROM selected_packages sp \
         LEFT JOIN latest_release lr ON lr.package_id = sp.package_id \
         LEFT JOIN finding_summary fs ON fs.package_id = sp.package_id \
         LEFT JOIN trusted_summary ts ON ts.package_id = sp.package_id",
    )
    .bind(&package_ids)
    .bind(RELEASE_HISTORY_VISIBLE_STATUSES)
    .fetch_all(db)
    .await
    .map_err(|error| ApiError(Error::Database(error)))?;

    let mut summaries = HashMap::with_capacity(rows.len());
    for row in rows {
        let package_id = row
            .try_get::<Uuid, _>("package_id")
            .map_err(|error| ApiError(Error::Internal(error.to_string())))?;
        summaries.insert(
            package_id,
            build_search_discovery_summary(
                row.try_get::<Option<String>, _>("latest_release_status")
                    .ok()
                    .flatten(),
                row.try_get::<Option<DateTime<Utc>>, _>("latest_release_published_at")
                    .ok()
                    .flatten(),
                row.try_get::<i64, _>("unresolved_security_finding_count")
                    .unwrap_or(0),
                row.try_get::<Option<String>, _>("worst_unresolved_security_severity")
                    .ok()
                    .flatten(),
                row.try_get::<i64, _>("trusted_publisher_count")
                    .unwrap_or(0),
            ),
        );
    }

    Ok(summaries)
}

fn build_search_discovery_summary(
    latest_release_status: Option<String>,
    latest_release_published_at: Option<DateTime<Utc>>,
    unresolved_security_finding_count: i64,
    worst_unresolved_security_severity: Option<String>,
    trusted_publisher_count: i64,
) -> SearchPackageDiscoverySummary {
    let risk_level = search_discovery_risk_level(
        unresolved_security_finding_count,
        worst_unresolved_security_severity.as_deref(),
        latest_release_status.is_some(),
    )
    .map(str::to_owned);
    let has_trusted_publisher = trusted_publisher_count > 0;
    let mut signals = Vec::new();

    if unresolved_security_finding_count > 0 {
        signals.push(format!(
            "{} unresolved security finding{}{}",
            unresolved_security_finding_count,
            plural_suffix(unresolved_security_finding_count),
            worst_unresolved_security_severity
                .as_deref()
                .map(|severity| format!(" (worst severity: {severity})"))
                .unwrap_or_default(),
        ));
    }

    match latest_release_status.as_deref() {
        Some("deprecated") => signals.push("Latest visible release is deprecated".to_owned()),
        Some("yanked") => signals.push("Latest visible release is yanked".to_owned()),
        None => signals.push("No published release is currently visible".to_owned()),
        _ => {}
    }

    if has_trusted_publisher {
        signals.push(format!(
            "{} trusted publisher{} configured",
            trusted_publisher_count,
            plural_suffix(trusted_publisher_count),
        ));
    }

    SearchPackageDiscoverySummary {
        risk_level,
        unresolved_security_finding_count,
        worst_unresolved_security_severity,
        has_trusted_publisher,
        trusted_publisher_count,
        latest_release_status,
        latest_release_published_at,
        signals,
    }
}

fn search_discovery_risk_level(
    unresolved_security_finding_count: i64,
    worst_unresolved_security_severity: Option<&str>,
    latest_release_visible: bool,
) -> Option<&'static str> {
    match worst_unresolved_security_severity
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("critical") => Some("critical"),
        Some("high") => Some("high"),
        Some("medium") => Some("moderate"),
        Some("low" | "info") if unresolved_security_finding_count > 0 => Some("low"),
        _ if unresolved_security_finding_count > 0 => Some("low"),
        _ if latest_release_visible => Some("low"),
        _ => None,
    }
}

fn plural_suffix(count: i64) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}

async fn load_visible_search_package_ids(
    state: &AppState,
    hits: &[PackageDocument],
    actor_user_id: Option<Uuid>,
    filters: SearchScopeFilters<'_>,
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
             AND ($3 IS NULL OR EXISTS (\
                SELECT 1 \
                FROM organizations o \
                WHERE o.id = p.owner_org_id AND o.slug = $3 \
             )) \
             AND ($4 IS NULL OR r.slug = $4) \
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
                         ) \
                         OR EXISTS (\
                             SELECT 1 \
                             FROM team_package_access tpa \
                             JOIN team_memberships tm ON tm.team_id = tpa.team_id \
                             JOIN teams t ON t.id = tpa.team_id \
                             WHERE tpa.package_id = p.id \
                               AND tm.user_id = $2 \
                               AND t.org_id = p.owner_org_id\
                         ) \
                         OR EXISTS (\
                             SELECT 1 \
                             FROM team_repository_access tra \
                             JOIN team_memberships tm ON tm.team_id = tra.team_id \
                             JOIN teams t ON t.id = tra.team_id \
                             WHERE tra.repository_id = r.id \
                               AND tm.user_id = $2 \
                               AND t.org_id = r.owner_org_id\
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
                         ) \
                         OR EXISTS (\
                             SELECT 1 \
                             FROM team_package_access tpa \
                             JOIN team_memberships tm ON tm.team_id = tpa.team_id \
                             JOIN teams t ON t.id = tpa.team_id \
                             WHERE tpa.package_id = p.id \
                               AND tm.user_id = $2 \
                               AND t.org_id = p.owner_org_id\
                         ) \
                         OR EXISTS (\
                             SELECT 1 \
                             FROM team_repository_access tra \
                             JOIN team_memberships tm ON tm.team_id = tra.team_id \
                             JOIN teams t ON t.id = tra.team_id \
                             WHERE tra.repository_id = r.id \
                               AND tm.user_id = $2 \
                               AND t.org_id = r.owner_org_id\
                         )\
                     )\
                 ))\
           )",
    )
    .bind(&package_ids)
    .bind(actor_user_id)
    .bind(filters.owner_org_slug)
    .bind(filters.repository_slug)
    .fetch_all(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(rows
        .into_iter()
        .filter_map(|row| row.try_get::<Uuid, _>("id").ok())
        .collect())
}

fn normalize_search_org_slug(org: Option<String>) -> ApiResult<Option<String>> {
    normalize_search_slug(org)
}

fn normalize_search_repository_slug(repository: Option<String>) -> ApiResult<Option<String>> {
    normalize_search_slug(repository)
}

fn normalize_search_slug(slug: Option<String>) -> ApiResult<Option<String>> {
    let Some(slug) = slug.map(|value| value.trim().to_owned()) else {
        return Ok(None);
    };

    if slug.is_empty() {
        return Ok(None);
    }

    validation::validate_slug(&slug).map_err(ApiError::from)?;
    Ok(Some(slug))
}

#[cfg(test)]
mod tests {
    use super::{
        build_search_discovery_summary, load_visible_search_package_ids, load_visible_search_page,
        normalize_search_org_slug, normalize_search_repository_slug, SearchScopeFilters,
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

    fn package_doc(
        id: Uuid,
        name: &str,
        visibility: &str,
        repository_name: &str,
        repository_slug: &str,
    ) -> PackageDocument {
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
            repository_name: Some(repository_name.to_owned()),
            repository_slug: Some(repository_slug.to_owned()),
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
            package_doc(
                public_package_id,
                "public-widget",
                "public",
                "Public",
                "public",
            ),
            package_doc(
                private_package_id,
                "private-widget",
                "private",
                "Private",
                "private",
            ),
            package_doc(
                unlisted_package_id,
                "unlisted-widget",
                "unlisted",
                "Public",
                "public",
            ),
        ];

        let anonymous =
            load_visible_search_package_ids(&state, &hits, None, SearchScopeFilters::default())
                .await
                .expect("anonymous ids should load");
        assert_eq!(anonymous, HashSet::from([public_package_id]));

        let org_member = load_visible_search_package_ids(
            &state,
            &hits,
            Some(owner_id),
            SearchScopeFilters::default(),
        )
        .await
        .expect("member ids should load");
        assert_eq!(
            org_member,
            HashSet::from([public_package_id, private_package_id])
        );

        let outsider = load_visible_search_package_ids(
            &state,
            &hits,
            Some(outsider_id),
            SearchScopeFilters::default(),
        )
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
                package_doc(public_a_id, "public-a", "public", "Public", "public"),
                package_doc(private_id, "private-a", "private", "Private", "private"),
                package_doc(public_b_id, "public-b", "public", "Public", "public"),
            ],
        };
        let query = SearchQuery {
            q: "widget".to_owned(),
            ecosystem: None,
            limit: Some(2),
            offset: Some(0),
        };

        let anonymous_page = load_visible_search_page(
            &state,
            &search,
            &query,
            None,
            SearchScopeFilters::default(),
            2,
            1,
        )
        .await
        .expect("anonymous page should load");
        assert_eq!(anonymous_page.total, 2);
        assert_eq!(anonymous_page.packages.len(), 1);
        assert_eq!(anonymous_page.packages[0].name, "public-b");
        assert_eq!(
            anonymous_page.packages[0].repository_name.as_deref(),
            Some("Public")
        );
        assert_eq!(
            anonymous_page.packages[0].repository_slug.as_deref(),
            Some("public")
        );

        let owner_page = load_visible_search_page(
            &state,
            &search,
            &query,
            Some(user_id),
            SearchScopeFilters::default(),
            2,
            1,
        )
        .await
        .expect("owner page should load");
        assert_eq!(owner_page.total, 3);
        assert_eq!(owner_page.packages.len(), 1);
        assert_eq!(owner_page.packages[0].name, "private-a");
        assert_eq!(
            owner_page.packages[0].repository_name.as_deref(),
            Some("Private")
        );
        assert_eq!(
            owner_page.packages[0].repository_slug.as_deref(),
            Some("private")
        );
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
            package_doc(
                acme_public_package_id,
                "acme-public-widget",
                "public",
                "Acme Public",
                "acme-public",
            ),
            package_doc(
                acme_private_package_id,
                "acme-private-widget",
                "private",
                "Acme Private",
                "acme-private",
            ),
            package_doc(
                beta_public_package_id,
                "beta-public-widget",
                "public",
                "Beta Public",
                "beta-public",
            ),
        ];

        let anonymous = load_visible_search_package_ids(
            &state,
            &hits,
            None,
            SearchScopeFilters {
                owner_org_slug: Some("acme-search"),
                repository_slug: None,
            },
        )
        .await
        .expect("anonymous ids should load");
        assert_eq!(anonymous, HashSet::from([acme_public_package_id]));

        let member = load_visible_search_package_ids(
            &state,
            &hits,
            Some(member_id),
            SearchScopeFilters {
                owner_org_slug: Some("acme-search"),
                repository_slug: None,
            },
        )
        .await
        .expect("member ids should load");
        assert_eq!(
            member,
            HashSet::from([acme_public_package_id, acme_private_package_id])
        );

        let beta = load_visible_search_package_ids(
            &state,
            &hits,
            Some(member_id),
            SearchScopeFilters {
                owner_org_slug: Some("beta-search"),
                repository_slug: None,
            },
        )
        .await
        .expect("beta ids should load");
        assert_eq!(beta, HashSet::from([beta_public_package_id]));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn visible_search_package_ids_can_filter_to_one_repository(pool: PgPool) {
        let state = test_state(pool.clone());
        let member_id = Uuid::new_v4();
        let org_id = Uuid::new_v4();
        let public_repo_id = Uuid::new_v4();
        let private_repo_id = Uuid::new_v4();
        let other_public_repo_id = Uuid::new_v4();
        let public_package_id = Uuid::new_v4();
        let private_package_id = Uuid::new_v4();
        let other_public_package_id = Uuid::new_v4();

        sqlx::query(
            "INSERT INTO users (id, username, email, password_hash, created_at, updated_at) \
             VALUES ($1, 'member', 'member-repo@test.dev', 'hash', NOW(), NOW())",
        )
        .bind(member_id)
        .execute(&pool)
        .await
        .expect("user should insert");

        sqlx::query(
            "INSERT INTO organizations (id, name, slug, is_verified, mfa_required, created_at, updated_at) \
             VALUES ($1, 'Acme', 'acme-repo-search', false, false, NOW(), NOW())",
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
        .bind(member_id)
        .execute(&pool)
        .await
        .expect("membership should insert");

        sqlx::query(
            "INSERT INTO repositories (id, name, slug, kind, visibility, owner_org_id, created_at, updated_at) \
             VALUES ($1, 'Release', 'release-packages', 'release', 'public', $4, NOW(), NOW()), \
                    ($2, 'Private', 'private-packages', 'release', 'private', $4, NOW(), NOW()), \
                    ($3, 'Public', 'public-packages', 'public', 'public', $4, NOW(), NOW())",
        )
        .bind(public_repo_id)
        .bind(private_repo_id)
        .bind(other_public_repo_id)
        .bind(org_id)
        .execute(&pool)
        .await
        .expect("repositories should insert");

        sqlx::query(
            "INSERT INTO packages (id, ecosystem, name, normalized_name, visibility, repository_id, owner_org_id, created_at, updated_at) \
             VALUES ($1, 'npm', 'release-widget', 'release-widget', 'public', $4, $7, NOW(), NOW()), \
                    ($2, 'npm', 'private-widget', 'private-widget', 'private', $5, $7, NOW(), NOW()), \
                    ($3, 'npm', 'public-widget', 'public-widget', 'public', $6, $7, NOW(), NOW())",
        )
        .bind(public_package_id)
        .bind(private_package_id)
        .bind(other_public_package_id)
        .bind(public_repo_id)
        .bind(private_repo_id)
        .bind(other_public_repo_id)
        .bind(org_id)
        .execute(&pool)
        .await
        .expect("packages should insert");

        let hits = vec![
            package_doc(
                public_package_id,
                "release-widget",
                "public",
                "Release",
                "release-packages",
            ),
            package_doc(
                private_package_id,
                "private-widget",
                "private",
                "Private",
                "private-packages",
            ),
            package_doc(
                other_public_package_id,
                "public-widget",
                "public",
                "Public",
                "public-packages",
            ),
        ];

        let anonymous = load_visible_search_package_ids(
            &state,
            &hits,
            None,
            SearchScopeFilters {
                owner_org_slug: Some("acme-repo-search"),
                repository_slug: Some("release-packages"),
            },
        )
        .await
        .expect("anonymous ids should load");
        assert_eq!(anonymous, HashSet::from([public_package_id]));

        let member = load_visible_search_package_ids(
            &state,
            &hits,
            Some(member_id),
            SearchScopeFilters {
                owner_org_slug: Some("acme-repo-search"),
                repository_slug: Some("private-packages"),
            },
        )
        .await
        .expect("member ids should load");
        assert_eq!(member, HashSet::from([private_package_id]));

        let missing = load_visible_search_package_ids(
            &state,
            &hits,
            Some(member_id),
            SearchScopeFilters {
                owner_org_slug: Some("acme-repo-search"),
                repository_slug: Some("missing-packages"),
            },
        )
        .await
        .expect("missing repository should return empty");
        assert!(missing.is_empty());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn visible_search_package_ids_include_delegated_team_private_access(pool: PgPool) {
        let state = test_state(pool.clone());
        let owner_id = Uuid::new_v4();
        let package_grantee_id = Uuid::new_v4();
        let repository_grantee_id = Uuid::new_v4();
        let org_id = Uuid::new_v4();
        let package_team_id = Uuid::new_v4();
        let repository_team_id = Uuid::new_v4();
        let package_repo_id = Uuid::new_v4();
        let repository_repo_id = Uuid::new_v4();
        let public_repo_id = Uuid::new_v4();
        let package_private_id = Uuid::new_v4();
        let repository_private_id = Uuid::new_v4();
        let public_id = Uuid::new_v4();

        sqlx::query(
            "INSERT INTO users (id, username, email, password_hash, created_at, updated_at) \
             VALUES ($1, 'owner', 'owner-search@test.dev', 'hash', NOW(), NOW()), \
                    ($2, 'package-grantee', 'package-grantee@test.dev', 'hash', NOW(), NOW()), \
                    ($3, 'repository-grantee', 'repository-grantee@test.dev', 'hash', NOW(), NOW())",
        )
        .bind(owner_id)
        .bind(package_grantee_id)
        .bind(repository_grantee_id)
        .execute(&pool)
        .await
        .expect("users should insert");

        sqlx::query(
            "INSERT INTO organizations (id, name, slug, is_verified, mfa_required, created_at, updated_at) \
             VALUES ($1, 'Delegated Search', 'delegated-search', false, false, NOW(), NOW())",
        )
        .bind(org_id)
        .execute(&pool)
        .await
        .expect("org should insert");

        sqlx::query(
            "INSERT INTO org_memberships (id, org_id, user_id, role, invited_by, joined_at) \
             VALUES ($1, $2, $3, 'owner', NULL, NOW())",
        )
        .bind(Uuid::new_v4())
        .bind(org_id)
        .bind(owner_id)
        .execute(&pool)
        .await
        .expect("memberships should insert");

        sqlx::query(
            "INSERT INTO teams (id, org_id, name, slug, description, created_at, updated_at) \
             VALUES ($1, $3, 'Package Readers', 'package-readers', NULL, NOW(), NOW()), \
                    ($2, $3, 'Repository Readers', 'repository-readers', NULL, NOW(), NOW())",
        )
        .bind(package_team_id)
        .bind(repository_team_id)
        .bind(org_id)
        .execute(&pool)
        .await
        .expect("teams should insert");

        sqlx::query(
            "INSERT INTO team_memberships (id, team_id, user_id, added_at) \
             VALUES ($1, $2, $3, NOW()), \
                    ($4, $5, $6, NOW())",
        )
        .bind(Uuid::new_v4())
        .bind(package_team_id)
        .bind(package_grantee_id)
        .bind(Uuid::new_v4())
        .bind(repository_team_id)
        .bind(repository_grantee_id)
        .execute(&pool)
        .await
        .expect("team memberships should insert");

        sqlx::query(
            "INSERT INTO repositories (id, name, slug, kind, visibility, owner_org_id, created_at, updated_at) \
             VALUES ($1, 'Package Private', 'package-private', 'release', 'private', $4, NOW(), NOW()), \
                    ($2, 'Repository Private', 'repository-private', 'release', 'private', $4, NOW(), NOW()), \
                    ($3, 'Public', 'public-repo', 'public', 'public', $4, NOW(), NOW())",
        )
        .bind(package_repo_id)
        .bind(repository_repo_id)
        .bind(public_repo_id)
        .bind(org_id)
        .execute(&pool)
        .await
        .expect("repositories should insert");

        sqlx::query(
            "INSERT INTO packages (id, ecosystem, name, normalized_name, visibility, repository_id, owner_org_id, created_at, updated_at) \
             VALUES ($1, 'npm', 'package-granted-widget', 'package-granted-widget', 'private', $4, $6, NOW(), NOW()), \
                    ($2, 'npm', 'repository-granted-widget', 'repository-granted-widget', 'private', $5, $6, NOW(), NOW()), \
                    ($3, 'npm', 'public-widget', 'public-widget', 'public', $7, $6, NOW(), NOW())",
        )
        .bind(package_private_id)
        .bind(repository_private_id)
        .bind(public_id)
        .bind(package_repo_id)
        .bind(repository_repo_id)
        .bind(org_id)
        .bind(public_repo_id)
        .execute(&pool)
        .await
        .expect("packages should insert");

        sqlx::query(
            "INSERT INTO team_package_access (id, team_id, package_id, permission, granted_at) \
             VALUES ($1, $2, $3, 'read_private', NOW())",
        )
        .bind(Uuid::new_v4())
        .bind(package_team_id)
        .bind(package_private_id)
        .execute(&pool)
        .await
        .expect("team package access should insert");

        sqlx::query(
            "INSERT INTO team_repository_access (id, team_id, repository_id, permission, granted_at) \
             VALUES ($1, $2, $3, 'publish', NOW())",
        )
        .bind(Uuid::new_v4())
        .bind(repository_team_id)
        .bind(repository_repo_id)
        .execute(&pool)
        .await
        .expect("team repository access should insert");

        let hits = vec![
            package_doc(
                package_private_id,
                "package-granted-widget",
                "private",
                "Package Private",
                "package-private",
            ),
            package_doc(
                repository_private_id,
                "repository-granted-widget",
                "private",
                "Repository Private",
                "repository-private",
            ),
            package_doc(
                public_id,
                "public-widget",
                "public",
                "Public",
                "public-repo",
            ),
        ];

        let package_grantee_ids = load_visible_search_package_ids(
            &state,
            &hits,
            Some(package_grantee_id),
            SearchScopeFilters::default(),
        )
        .await
        .expect("package grantee ids should load");
        assert_eq!(
            package_grantee_ids,
            HashSet::from([package_private_id, public_id])
        );

        let repository_grantee_ids = load_visible_search_package_ids(
            &state,
            &hits,
            Some(repository_grantee_id),
            SearchScopeFilters::default(),
        )
        .await
        .expect("repository grantee ids should load");
        assert_eq!(
            repository_grantee_ids,
            HashSet::from([repository_private_id, public_id])
        );
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

    #[test]
    fn normalize_search_repository_slug_validates_optional_values() {
        assert_eq!(
            normalize_search_repository_slug(Some("release-packages".to_owned()))
                .expect("slug should normalize"),
            Some("release-packages".to_owned())
        );
        assert_eq!(
            normalize_search_repository_slug(Some("  ".to_owned()))
                .expect("blank slug should clear"),
            None
        );
        assert!(normalize_search_repository_slug(Some("Release Packages".to_owned())).is_err());
    }

    #[test]
    fn search_discovery_summary_explains_risk_and_verification_hints() {
        let summary = build_search_discovery_summary(
            Some("deprecated".to_owned()),
            None,
            2,
            Some("high".to_owned()),
            1,
        );

        assert_eq!(summary.risk_level.as_deref(), Some("high"));
        assert_eq!(summary.unresolved_security_finding_count, 2);
        assert_eq!(
            summary.worst_unresolved_security_severity.as_deref(),
            Some("high")
        );
        assert!(summary.has_trusted_publisher);
        assert_eq!(summary.trusted_publisher_count, 1);
        assert_eq!(
            summary.signals,
            vec![
                "2 unresolved security findings (worst severity: high)".to_owned(),
                "Latest visible release is deprecated".to_owned(),
                "1 trusted publisher configured".to_owned(),
            ]
        );
    }

    #[test]
    fn search_discovery_summary_leaves_risk_pending_without_release_or_findings() {
        let summary = build_search_discovery_summary(None, None, 0, None, 0);

        assert_eq!(summary.risk_level, None);
        assert_eq!(summary.unresolved_security_finding_count, 0);
        assert_eq!(summary.latest_release_status, None);
        assert_eq!(
            summary.signals,
            vec!["No published release is currently visible".to_owned()]
        );
    }

    #[test]
    fn search_discovery_summary_marks_visible_release_without_findings_low_risk() {
        let summary =
            build_search_discovery_summary(Some("published".to_owned()), None, 0, None, 0);

        assert_eq!(summary.risk_level.as_deref(), Some("low"));
        assert_eq!(summary.unresolved_security_finding_count, 0);
        assert_eq!(summary.latest_release_status.as_deref(), Some("published"));
        assert!(summary.signals.is_empty());
    }
}
