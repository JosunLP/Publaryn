use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, patch, post},
    Json, Router,
};
use serde::Deserialize;
use sqlx::Row;
use uuid::Uuid;

use publaryn_core::{
    domain::repository::{Repository, RepositoryKind, Visibility},
    error::Error,
    validation,
};

use crate::{
    error::{ApiError, ApiResult},
    request_auth::{
        ensure_org_admin_by_id, ensure_repository_read_access, ensure_repository_write_access,
        AuthenticatedIdentity, OptionalAuthenticatedIdentity,
    },
    scopes::{ensure_scope, SCOPE_REPOSITORIES_WRITE},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/repositories", post(create_repository))
        .route("/v1/repositories/{slug}", get(get_repository))
        .route("/v1/repositories/{slug}", patch(update_repository))
        .route(
            "/v1/repositories/{slug}/packages",
            get(list_repository_packages),
        )
}

#[derive(Debug, Deserialize)]
struct CreateRepositoryRequest {
    name: String,
    slug: String,
    description: Option<String>,
    kind: Option<String>,
    visibility: Option<String>,
    owner_user_id: Option<Uuid>,
    owner_org_id: Option<Uuid>,
    upstream_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UpdateRepositoryRequest {
    description: Option<String>,
    visibility: Option<String>,
    upstream_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PackageListQuery {
    page: Option<u32>,
    per_page: Option<u32>,
}

async fn create_repository(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Json(body): Json<CreateRepositoryRequest>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    ensure_scope(&identity, SCOPE_REPOSITORIES_WRITE)?;
    validation::validate_slug(&body.slug).map_err(ApiError::from)?;

    if body.owner_user_id.is_some() && body.owner_org_id.is_some() {
        return Err(ApiError(Error::Validation(
            "Repository must belong to either a user or an organization, not both".into(),
        )));
    }

    let kind = parse_repository_kind(body.kind.as_deref().unwrap_or("public"))?;
    let visibility = parse_visibility(body.visibility.as_deref().unwrap_or("public"))?;
    let kind_str = kind.as_str();
    let visibility_str = visibility.as_str();
    let owner_user_id = match (body.owner_user_id, body.owner_org_id) {
        (Some(owner_user_id), None) if owner_user_id == identity.user_id => Some(owner_user_id),
        (Some(_), None) => {
            return Err(ApiError(Error::Forbidden(
                "You can only create user-owned repositories for your own account".into(),
            )));
        }
        (None, Some(owner_org_id)) => {
            ensure_org_admin_by_id(&state.db, owner_org_id, identity.user_id).await?;
            None
        }
        (None, None) => Some(identity.user_id),
        (Some(_), Some(_)) => unreachable!("validated above"),
    };

    let mut repository = Repository::new(body.name, body.slug, kind.clone(), visibility.clone());
    repository.description = body.description;
    repository.owner_user_id = owner_user_id;
    repository.owner_org_id = body.owner_org_id;
    repository.upstream_url = body.upstream_url;

    sqlx::query(
        "INSERT INTO repositories (id, name, slug, description, kind, visibility, owner_user_id, \
         owner_org_id, upstream_url, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, NOW())",
    )
    .bind(repository.id)
    .bind(&repository.name)
    .bind(&repository.slug)
    .bind(&repository.description)
    .bind(repository.kind.clone())
    .bind(repository.visibility.clone())
    .bind(repository.owner_user_id)
    .bind(repository.owner_org_id)
    .bind(&repository.upstream_url)
    .bind(repository.created_at)
    .execute(&state.db)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db) if db.is_unique_violation() => ApiError(Error::AlreadyExists(
            "Repository slug already exists".into(),
        )),
        _ => ApiError(Error::Database(e)),
    })?;

    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "id": repository.id,
            "slug": repository.slug,
            "kind": kind_str,
            "visibility": visibility_str,
        })),
    ))
}

async fn get_repository(
    State(state): State<AppState>,
    identity: OptionalAuthenticatedIdentity,
    Path(slug): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let access = ensure_repository_read_access(&state.db, &slug, identity.user_id()).await?;

    let row = sqlx::query(
        "SELECT r.id, r.name, r.slug, r.description, r.kind::text AS kind, r.visibility::text AS visibility, r.owner_user_id, r.owner_org_id, \
            r.upstream_url, r.created_at, r.updated_at, u.username AS owner_username, o.slug AS owner_org_slug, o.name AS owner_org_name \
         FROM repositories r \
         LEFT JOIN users u ON u.id = r.owner_user_id \
         LEFT JOIN organizations o ON o.id = r.owner_org_id \
         WHERE r.id = $1",
    )
    .bind(access.repository_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| ApiError(Error::NotFound(format!("Repository '{slug}' not found"))))?;

    Ok(Json(serde_json::json!({
        "id": row.try_get::<Uuid, _>("id").ok(),
        "name": row.try_get::<String, _>("name").ok(),
        "slug": row.try_get::<String, _>("slug").ok(),
        "description": row.try_get::<Option<String>, _>("description").ok().flatten(),
        "kind": row.try_get::<String, _>("kind").ok(),
        "visibility": row.try_get::<String, _>("visibility").ok(),
        "owner_user_id": row.try_get::<Option<Uuid>, _>("owner_user_id").ok().flatten(),
        "owner_org_id": row.try_get::<Option<Uuid>, _>("owner_org_id").ok().flatten(),
        "owner_username": row.try_get::<Option<String>, _>("owner_username").ok().flatten(),
        "owner_org_slug": row.try_get::<Option<String>, _>("owner_org_slug").ok().flatten(),
        "owner_org_name": row.try_get::<Option<String>, _>("owner_org_name").ok().flatten(),
        "upstream_url": row.try_get::<Option<String>, _>("upstream_url").ok().flatten(),
        "created_at": row.try_get::<chrono::DateTime<chrono::Utc>, _>("created_at").ok(),
        "updated_at": row.try_get::<chrono::DateTime<chrono::Utc>, _>("updated_at").ok(),
    })))
}

async fn update_repository(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path(slug): Path<String>,
    Json(body): Json<UpdateRepositoryRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_REPOSITORIES_WRITE)?;
    let visibility = body
        .visibility
        .as_deref()
        .map(parse_visibility)
        .transpose()?;

    let repository_id = ensure_repository_write_access(&state.db, &slug, identity.user_id).await?;

    sqlx::query(
        "UPDATE repositories \
         SET description = COALESCE($1, description), \
             visibility  = COALESCE($2, visibility), \
             upstream_url = COALESCE($3, upstream_url), \
             updated_at  = NOW() \
         WHERE id = $4",
    )
    .bind(&body.description)
    .bind(visibility)
    .bind(&body.upstream_url)
    .bind(repository_id)
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(Json(serde_json::json!({ "message": "Repository updated" })))
}

async fn list_repository_packages(
    State(state): State<AppState>,
    identity: OptionalAuthenticatedIdentity,
    Path(slug): Path<String>,
    Query(query): Query<PackageListQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let limit = query.per_page.unwrap_or(20).min(100) as i64;
    let offset = ((query.page.unwrap_or(1).saturating_sub(1)) as i64) * limit;
    let access = ensure_repository_read_access(&state.db, &slug, identity.user_id()).await?;

    let rows = sqlx::query(
        "SELECT p.id, p.name, p.ecosystem, p.description, p.visibility, p.download_count, p.created_at \
         FROM packages p \
         WHERE p.repository_id = $1 \
           AND ($2::bool = true OR p.visibility = 'public') \
         ORDER BY p.download_count DESC, p.created_at DESC \
         LIMIT $3 OFFSET $4",
    )
    .bind(access.repository_id)
    .bind(access.can_view_non_public_packages)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    let packages: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            serde_json::json!({
                "id": row.try_get::<Uuid, _>("id").ok(),
                "name": row.try_get::<String, _>("name").ok(),
                "ecosystem": row.try_get::<String, _>("ecosystem").ok(),
                "description": row.try_get::<Option<String>, _>("description").ok().flatten(),
                "visibility": row.try_get::<String, _>("visibility").ok(),
                "download_count": row.try_get::<i64, _>("download_count").ok(),
                "created_at": row.try_get::<chrono::DateTime<chrono::Utc>, _>("created_at").ok(),
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "packages": packages })))
}

fn parse_repository_kind(input: &str) -> ApiResult<RepositoryKind> {
    match input.to_lowercase().as_str() {
        "public" => Ok(RepositoryKind::Public),
        "private" => Ok(RepositoryKind::Private),
        "staging" => Ok(RepositoryKind::Staging),
        "release" => Ok(RepositoryKind::Release),
        "proxy" => Ok(RepositoryKind::Proxy),
        "virtual" => Ok(RepositoryKind::Virtual),
        other => Err(ApiError(Error::Validation(format!(
            "Unknown repository kind: {other}"
        )))),
    }
}

fn parse_visibility(input: &str) -> ApiResult<Visibility> {
    match input.to_lowercase().as_str() {
        "public" => Ok(Visibility::Public),
        "private" => Ok(Visibility::Private),
        "internal-org" | "internal_org" => Ok(Visibility::InternalOrg),
        "unlisted" => Ok(Visibility::Unlisted),
        "quarantined" => Ok(Visibility::Quarantined),
        other => Err(ApiError(Error::Validation(format!(
            "Unknown visibility: {other}"
        )))),
    }
}

trait RepositoryKindExt {
    fn as_str(&self) -> &'static str;
}

impl RepositoryKindExt for RepositoryKind {
    fn as_str(&self) -> &'static str {
        match self {
            RepositoryKind::Public => "public",
            RepositoryKind::Private => "private",
            RepositoryKind::Staging => "staging",
            RepositoryKind::Release => "release",
            RepositoryKind::Proxy => "proxy",
            RepositoryKind::Virtual => "virtual",
        }
    }
}

trait VisibilityExt {
    fn as_str(&self) -> &'static str;
}

impl VisibilityExt for Visibility {
    fn as_str(&self) -> &'static str {
        match self {
            Visibility::Public => "public",
            Visibility::Private => "private",
            Visibility::InternalOrg => "internal_org",
            Visibility::Unlisted => "unlisted",
            Visibility::Quarantined => "quarantined",
        }
    }
}
