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
        actor_can_create_packages_in_repository_by_id, actor_can_manage_repository_by_id,
        actor_can_transfer_repository_by_id, ensure_org_admin_by_id,
        ensure_repository_admin_access, ensure_repository_read_access,
        ensure_repository_transfer_access, AuthenticatedIdentity, OptionalAuthenticatedIdentity,
    },
    scopes::{
        ensure_scope, SCOPE_PACKAGES_WRITE, SCOPE_REPOSITORIES_TRANSFER, SCOPE_REPOSITORIES_WRITE,
    },
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/repositories", post(create_repository))
        .route("/v1/repositories/{slug}", get(get_repository))
        .route("/v1/repositories/{slug}", patch(update_repository))
        .route(
            "/v1/repositories/{slug}/ownership-transfer",
            post(transfer_repository_ownership),
        )
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

#[derive(Debug, Deserialize)]
struct TransferRepositoryOwnershipRequest {
    target_org_slug: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RepositoryOwner {
    User(Uuid),
    Organization(Uuid),
}

#[derive(Debug, Clone)]
struct TargetOrganization {
    id: Uuid,
    slug: String,
    name: String,
}

async fn can_manage_repository(
    db: &sqlx::PgPool,
    repository_id: Uuid,
    identity: &OptionalAuthenticatedIdentity,
) -> ApiResult<bool> {
    match identity.0.as_ref() {
        Some(identity)
            if identity
                .scopes()
                .iter()
                .any(|scope| scope == SCOPE_REPOSITORIES_WRITE) =>
        {
            actor_can_manage_repository_by_id(db, repository_id, Some(identity.user_id)).await
        }
        _ => Ok(false),
    }
}

async fn can_create_packages_in_repository(
    db: &sqlx::PgPool,
    repository_id: Uuid,
    identity: &OptionalAuthenticatedIdentity,
) -> ApiResult<bool> {
    match identity.0.as_ref() {
        Some(identity)
            if identity
                .scopes()
                .iter()
                .any(|scope| scope == SCOPE_PACKAGES_WRITE) =>
        {
            actor_can_create_packages_in_repository_by_id(db, repository_id, Some(identity.user_id))
                .await
        }
        _ => Ok(false),
    }
}

async fn can_transfer_repository(
    db: &sqlx::PgPool,
    repository_id: Uuid,
    identity: &OptionalAuthenticatedIdentity,
) -> ApiResult<bool> {
    match identity.0.as_ref() {
        Some(identity)
            if identity
                .scopes()
                .iter()
                .any(|scope| scope == SCOPE_REPOSITORIES_TRANSFER) =>
        {
            actor_can_transfer_repository_by_id(db, repository_id, Some(identity.user_id)).await
        }
        _ => Ok(false),
    }
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
    let can_manage = can_manage_repository(&state.db, access.repository_id, &identity).await?;
    let can_create_packages =
        can_create_packages_in_repository(&state.db, access.repository_id, &identity).await?;
    let can_transfer = can_transfer_repository(&state.db, access.repository_id, &identity).await?;

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
        "can_manage": can_manage,
        "can_create_packages": can_create_packages,
        "can_transfer": can_transfer,
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

    let repository_id = ensure_repository_admin_access(&state.db, &slug, identity.user_id).await?;

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

async fn transfer_repository_ownership(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path(slug): Path<String>,
    Json(body): Json<TransferRepositoryOwnershipRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_REPOSITORIES_TRANSFER)?;
    validation::validate_slug(&body.target_org_slug).map_err(ApiError::from)?;

    ensure_repository_transfer_access(&state.db, &slug, identity.user_id).await?;

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    let repository_row = sqlx::query(
        "SELECT id, name, slug, kind::text AS kind, visibility::text AS visibility, owner_user_id, owner_org_id \
         FROM repositories \
         WHERE slug = $1 \
         FOR UPDATE",
    )
    .bind(&slug)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| ApiError(Error::NotFound(format!("Repository '{slug}' not found"))))?;

    let repository_id: Uuid = repository_row
        .try_get("id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let repository_name: String = repository_row
        .try_get("name")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let repository_slug: String = repository_row
        .try_get("slug")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let repository_kind: String = repository_row
        .try_get("kind")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let repository_visibility: String = repository_row
        .try_get("visibility")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let current_owner = repository_owner_from_fields(
        repository_row
            .try_get::<Option<Uuid>, _>("owner_user_id")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        repository_row
            .try_get::<Option<Uuid>, _>("owner_org_id")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
    )?;

    let target_org_row = sqlx::query(
        "SELECT id, slug, name \
         FROM organizations \
         WHERE slug = $1",
    )
    .bind(&body.target_org_slug)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| {
        ApiError(Error::NotFound(format!(
            "Organization '{}' not found",
            body.target_org_slug
        )))
    })?;

    let target_org = TargetOrganization {
        id: target_org_row
            .try_get("id")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        slug: target_org_row
            .try_get("slug")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        name: target_org_row
            .try_get("name")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
    };

    let actor_controls_target = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (\
             SELECT 1 \
             FROM org_memberships \
             WHERE org_id = $1 AND user_id = $2 AND role::text = ANY($3)\
         )",
    )
    .bind(target_org.id)
    .bind(identity.user_id)
    .bind(vec!["owner".to_owned(), "admin".to_owned()])
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    if !actor_controls_target {
        return Err(ApiError(Error::Forbidden(
            "Transferring a repository into an organization requires owner or admin membership in the target organization".into(),
        )));
    }

    validate_repository_transfer_target(&current_owner, target_org.id)?;

    let revoked_team_grants: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT \
         FROM team_repository_access \
         WHERE repository_id = $1",
    )
    .bind(repository_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query("DELETE FROM team_repository_access WHERE repository_id = $1")
        .bind(repository_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query(
        "UPDATE repositories \
         SET owner_user_id = NULL, \
             owner_org_id = $1, \
             updated_at = NOW() \
         WHERE id = $2",
    )
    .bind(target_org.id)
    .bind(repository_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    let (previous_owner_type, previous_owner_user_id, previous_owner_org_id) = match current_owner {
        RepositoryOwner::User(user_id) => ("user", Some(user_id), None),
        RepositoryOwner::Organization(org_id) => ("organization", None, Some(org_id)),
    };

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_org_id, metadata, occurred_at) \
         VALUES ($1, 'repository_transfer', $2, $3, $4, $5, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(target_org.id)
    .bind(serde_json::json!({
        "repository_id": repository_id,
        "repository_name": repository_name,
        "repository_slug": repository_slug,
        "repository_kind": repository_kind,
        "repository_visibility": repository_visibility,
        "previous_owner_type": previous_owner_type,
        "previous_owner_user_id": previous_owner_user_id,
        "previous_owner_org_id": previous_owner_org_id,
        "new_owner_type": "organization",
        "new_owner_org_id": target_org.id,
        "new_owner_org_slug": target_org.slug,
        "new_owner_org_name": target_org.name,
        "revoked_team_grants": revoked_team_grants,
    }))
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    tx.commit()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(Json(serde_json::json!({
        "message": "Repository ownership transferred",
        "repository": {
            "id": repository_id,
            "name": repository_name,
            "slug": repository_slug,
            "kind": repository_kind,
            "visibility": repository_visibility,
        },
        "owner": {
            "type": "organization",
            "id": target_org.id,
            "slug": target_org.slug,
            "name": target_org.name,
        },
    })))
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

fn repository_owner_from_fields(
    owner_user_id: Option<Uuid>,
    owner_org_id: Option<Uuid>,
) -> ApiResult<RepositoryOwner> {
    match (owner_user_id, owner_org_id) {
        (Some(user_id), None) => Ok(RepositoryOwner::User(user_id)),
        (None, Some(org_id)) => Ok(RepositoryOwner::Organization(org_id)),
        (None, None) => Err(ApiError(Error::Internal(
            "Repository owner is not set".into(),
        ))),
        (Some(_), Some(_)) => Err(ApiError(Error::Internal(
            "Repository owner state is invalid".into(),
        ))),
    }
}

fn validate_repository_transfer_target(
    current_owner: &RepositoryOwner,
    target_org_id: Uuid,
) -> ApiResult<()> {
    if matches!(current_owner, RepositoryOwner::Organization(current_org_id) if *current_org_id == target_org_id)
    {
        return Err(ApiError(Error::Conflict(
            "The selected organization already owns this repository".into(),
        )));
    }

    Ok(())
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
