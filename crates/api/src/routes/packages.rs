use axum::{
    body::{Body, Bytes},
    extract::{Path, Query, State},
    http::{
        header::{CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE},
        HeaderMap, StatusCode,
    },
    response::Response,
    routing::{delete, get, patch, post, put},
    Json, Router,
};
use sha2::{Digest, Sha256, Sha512};
use serde::Deserialize;
use sqlx::Row;
use uuid::Uuid;

use publaryn_core::{
    domain::{
        artifact::{Artifact, ArtifactKind},
        namespace::Ecosystem,
        package::{normalize_package_name, Package},
        release::Release,
        repository::Visibility,
    },
    error::Error,
    policy::{self, PolicyViolation},
    validation,
};
use publaryn_search::{PackageDocument, SearchIndex};

use crate::{
    error::{ApiError, ApiResult},
    request_auth::{
        actor_can_write_package_by_id, ensure_package_admin_access,
        ensure_package_metadata_write_access, ensure_package_publish_access,
        ensure_package_read_access, ensure_package_transfer_access,
        ensure_repository_write_access, AuthenticatedIdentity, OptionalAuthenticatedIdentity,
    },
    routes::parse_ecosystem,
    scopes::{ensure_scope, SCOPE_PACKAGES_TRANSFER, SCOPE_PACKAGES_WRITE},
    state::AppState,
    storage::PutArtifactObject,
};

const PACKAGE_CREATION_ALLOWED_REPOSITORY_KINDS: &[&str] =
    &["public", "private", "staging", "release"];
const RELEASE_HISTORY_VISIBLE_STATUSES: &[&str] = &["published", "deprecated", "yanked"];
const RELEASE_MANAGEMENT_VISIBLE_STATUSES: &[&str] =
    &["quarantine", "scanning", "published", "deprecated", "yanked"];

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/packages", post(create_package))
        .route("/v1/packages/:ecosystem/:name", get(get_package))
        .route("/v1/packages/:ecosystem/:name", patch(update_package))
        .route("/v1/packages/:ecosystem/:name", delete(delete_package))
        .route(
            "/v1/packages/:ecosystem/:name/ownership-transfer",
            post(transfer_package_ownership),
        )
        .route(
            "/v1/packages/:ecosystem/:name/releases",
            get(list_releases).post(create_release),
        )
        .route(
            "/v1/packages/:ecosystem/:name/releases/:version",
            get(get_release),
        )
        .route(
            "/v1/packages/:ecosystem/:name/releases/:version/publish",
            post(publish_release),
        )
        .route(
            "/v1/packages/:ecosystem/:name/releases/:version/artifacts",
            get(list_artifacts),
        )
        .route(
            "/v1/packages/:ecosystem/:name/releases/:version/artifacts/:filename",
            get(download_artifact).put(upload_artifact),
        )
        .route(
            "/v1/packages/:ecosystem/:name/releases/:version/yank",
            put(yank_release),
        )
        .route(
            "/v1/packages/:ecosystem/:name/releases/:version/unyank",
            put(unyank_release),
        )
        .route(
            "/v1/packages/:ecosystem/:name/releases/:version/deprecate",
            put(deprecate_release),
        )
        .route("/v1/packages/:ecosystem/:name/tags", get(list_tags))
        .route("/v1/packages/:ecosystem/:name/tags/:tag", put(upsert_tag))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PackageOwner {
    User(Uuid),
    Organization(Uuid),
}

#[derive(Debug, Clone)]
struct TargetOrganization {
    id: Uuid,
    slug: String,
    name: String,
}

#[derive(Debug, Clone)]
struct ReleaseAccess {
    package_id: Uuid,
    release_id: Uuid,
    status: String,
}

async fn get_package(
    State(state): State<AppState>,
    identity: OptionalAuthenticatedIdentity,
    Path((ecosystem_str, name)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let eco = parse_ecosystem(&ecosystem_str)?;
    let normalized = normalize_package_name(&name, &eco);
    let package_id =
        ensure_package_read_access(&state.db, eco.as_str(), &normalized, identity.user_id())
            .await?;

    let row = sqlx::query(
        "SELECT id, name, ecosystem, description, homepage, repository_url, license, keywords, \
                visibility, is_deprecated, deprecation_message, is_archived, download_count, \
                created_at, updated_at \
         FROM packages \
         WHERE id = $1",
    )
    .bind(package_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| {
        ApiError(Error::NotFound(format!(
            "Package '{name}' not found in {ecosystem_str}"
        )))
    })?;

    Ok(Json(serde_json::json!({
        "id": row.try_get::<Uuid, _>("id").ok(),
        "name": row.try_get::<String, _>("name").ok(),
        "ecosystem": row.try_get::<String, _>("ecosystem").ok(),
        "description": row.try_get::<Option<String>, _>("description").ok().flatten(),
        "homepage": row.try_get::<Option<String>, _>("homepage").ok().flatten(),
        "repository_url": row.try_get::<Option<String>, _>("repository_url").ok().flatten(),
        "license": row.try_get::<Option<String>, _>("license").ok().flatten(),
        "keywords": row.try_get::<Vec<String>, _>("keywords").ok(),
        "visibility": row.try_get::<String, _>("visibility").ok(),
        "is_deprecated": row.try_get::<bool, _>("is_deprecated").ok(),
        "deprecation_message": row.try_get::<Option<String>, _>("deprecation_message").ok().flatten(),
        "is_archived": row.try_get::<bool, _>("is_archived").ok(),
        "download_count": row.try_get::<i64, _>("download_count").ok(),
        "created_at": row.try_get::<chrono::DateTime<chrono::Utc>, _>("created_at").ok(),
        "updated_at": row.try_get::<chrono::DateTime<chrono::Utc>, _>("updated_at").ok(),
    })))
}

#[derive(Debug, Deserialize)]
struct UpdatePackageRequest {
    description: Option<String>,
    homepage: Option<String>,
    repository_url: Option<String>,
    license: Option<String>,
    keywords: Option<Vec<String>>,
    readme: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TransferPackageOwnershipRequest {
    target_org_slug: String,
}

#[derive(Debug, Deserialize)]
struct CreatePackageRequest {
    ecosystem: String,
    name: String,
    repository_slug: String,
    visibility: Option<String>,
    display_name: Option<String>,
    description: Option<String>,
    readme: Option<String>,
    homepage: Option<String>,
    repository_url: Option<String>,
    license: Option<String>,
    keywords: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct CreateReleaseRequest {
    version: String,
    description: Option<String>,
    changelog: Option<String>,
    source_ref: Option<String>,
    provenance: Option<serde_json::Value>,
    is_prerelease: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ArtifactUploadQuery {
    kind: String,
    sha256: Option<String>,
    is_signed: Option<bool>,
    signature_key_id: Option<String>,
}

#[derive(Debug, Clone)]
struct RepositoryPackageCreationTarget {
    id: Uuid,
    slug: String,
    kind: String,
    visibility: Visibility,
    owner_user_id: Option<Uuid>,
    owner_org_id: Option<Uuid>,
}

async fn create_package(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Json(body): Json<CreatePackageRequest>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    ensure_scope(&identity, SCOPE_PACKAGES_WRITE)?;
    validation::validate_slug(&body.repository_slug).map_err(ApiError::from)?;

    let ecosystem = parse_ecosystem(&body.ecosystem)?;
    validation::validate_package_name(&body.name, &ecosystem).map_err(ApiError::from)?;

    let repository_id =
        ensure_repository_write_access(&state.db, &body.repository_slug, identity.user_id).await?;
    let repository = load_repository_package_creation_target(&state.db, repository_id).await?;
    validate_package_creation_repository_kind(&repository.kind)?;

    let requested_visibility = body
        .visibility
        .as_deref()
        .map(parse_package_visibility)
        .transpose()?;
    let package_visibility = derive_package_visibility(
        requested_visibility,
        repository.visibility.clone(),
        repository.owner_org_id.is_some(),
    )?;

    let normalized_name = normalize_package_name(&body.name, &ecosystem);
    let existing_rows =
        sqlx::query("SELECT name, normalized_name FROM packages WHERE ecosystem = $1")
            .bind(ecosystem.as_str())
            .fetch_all(&state.db)
            .await
            .map_err(|e| ApiError(Error::Database(e)))?;

    if existing_rows.iter().any(|row| {
        row.try_get::<String, _>("normalized_name")
            .ok()
            .is_some_and(|existing| existing == normalized_name)
    }) {
        return Err(ApiError(Error::AlreadyExists(format!(
            "A package named '{}' already exists in ecosystem '{}'",
            body.name,
            ecosystem.as_str()
        ))));
    }

    let existing_names = existing_rows
        .iter()
        .filter_map(|row| row.try_get::<String, _>("name").ok())
        .collect::<Vec<_>>();
    let policy_violations = policy::check_name_policy(&body.name, &existing_names, &ecosystem)
        .map_err(ApiError::from)?;
    if !policy_violations.is_empty() {
        return Err(ApiError(Error::PolicyViolation(join_policy_violations(
            &policy_violations,
        ))));
    }

    validate_namespace_claim_for_package(
        &state.db,
        &ecosystem,
        &body.name,
        repository.owner_user_id,
        repository.owner_org_id,
    )
    .await?;

    let mut package = Package::new(
        repository.id,
        ecosystem.clone(),
        body.name.clone(),
        package_visibility.clone(),
    );
    package.display_name = body.display_name;
    package.description = body.description;
    package.readme = body.readme;
    package.homepage = body.homepage;
    package.repository_url = body.repository_url;
    package.license = body.license;
    package.keywords = body.keywords.unwrap_or_default();
    package.owner_user_id = repository.owner_user_id;
    package.owner_org_id = repository.owner_org_id;

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query(
        "INSERT INTO packages (id, repository_id, ecosystem, name, normalized_name, display_name, \
         description, readme, homepage, repository_url, license, keywords, visibility, \
         owner_user_id, owner_org_id, is_deprecated, deprecation_message, is_archived, \
         download_count, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, false, NULL, false, 0, $16, $17)",
    )
    .bind(package.id)
    .bind(package.repository_id)
    .bind(package.ecosystem.as_str())
    .bind(&package.name)
    .bind(&package.normalized_name)
    .bind(&package.display_name)
    .bind(&package.description)
    .bind(&package.readme)
    .bind(&package.homepage)
    .bind(&package.repository_url)
    .bind(&package.license)
    .bind(&package.keywords)
    .bind(visibility_as_str(&package.visibility))
    .bind(package.owner_user_id)
    .bind(package.owner_org_id)
    .bind(package.created_at)
    .bind(package.updated_at)
    .execute(&mut *tx)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db) if db.is_unique_violation() => ApiError(Error::AlreadyExists(
            "Package already exists in the selected repository".into(),
        )),
        _ => ApiError(Error::Database(e)),
    })?;

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_package_id, metadata, occurred_at) \
         VALUES ($1, 'package_create', $2, $3, $4, $5, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(package.id)
    .bind(serde_json::json!({
        "ecosystem": package.ecosystem.as_str(),
        "name": package.name,
        "normalized_name": package.normalized_name,
        "repository_slug": repository.slug,
        "visibility": visibility_as_str(&package.visibility),
    }))
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    tx.commit()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    if let Err(error) = index_package_after_creation(&state, &package).await {
        tracing::warn!(
            package_id = %package.id,
            error = %error,
            "Failed to index newly created package"
        );
    }

    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "id": package.id,
            "ecosystem": package.ecosystem.as_str(),
            "name": package.name,
            "normalized_name": package.normalized_name,
            "repository_slug": repository.slug,
            "visibility": visibility_as_str(&package.visibility),
            "owner_user_id": package.owner_user_id,
            "owner_org_id": package.owner_org_id,
        })),
    ))
}

async fn update_package(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path((ecosystem_str, name)): Path<(String, String)>,
    Json(body): Json<UpdatePackageRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_PACKAGES_WRITE)?;

    let eco = parse_ecosystem(&ecosystem_str)?;
    let normalized = normalize_package_name(&name, &eco);

    ensure_package_metadata_write_access(&state.db, eco.as_str(), &normalized, identity.user_id)
        .await?;

    sqlx::query(
        "UPDATE packages \
         SET description    = COALESCE($1, description), \
             homepage       = COALESCE($2, homepage), \
             repository_url = COALESCE($3, repository_url), \
             license        = COALESCE($4, license), \
             keywords       = COALESCE($5, keywords), \
             readme         = COALESCE($6, readme), \
             updated_at     = NOW() \
         WHERE ecosystem = $7 AND normalized_name = $8",
    )
    .bind(&body.description)
    .bind(&body.homepage)
    .bind(&body.repository_url)
    .bind(&body.license)
    .bind(&body.keywords)
    .bind(&body.readme)
    .bind(eco.as_str())
    .bind(&normalized)
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(Json(serde_json::json!({ "message": "Package updated" })))
}

async fn delete_package(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path((ecosystem_str, name)): Path<(String, String)>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    ensure_scope(&identity, SCOPE_PACKAGES_WRITE)?;

    let eco = parse_ecosystem(&ecosystem_str)?;
    let normalized = normalize_package_name(&name, &eco);
    let package_id =
        ensure_package_admin_access(&state.db, eco.as_str(), &normalized, identity.user_id).await?;

    sqlx::query("UPDATE packages SET is_archived = true, updated_at = NOW() WHERE id = $1")
        .bind(package_id)
        .execute(&state.db)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_package_id, metadata, occurred_at) \
         VALUES ($1, 'package_delete', $2, $3, $4, $5, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(package_id)
    .bind(serde_json::json!({
        "ecosystem": eco.as_str(),
        "name": normalized,
        "mode": "archive",
    }))
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    Ok((
        StatusCode::OK,
        Json(serde_json::json!({ "message": "Package archived" })),
    ))
}

async fn transfer_package_ownership(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path((ecosystem_str, name)): Path<(String, String)>,
    Json(body): Json<TransferPackageOwnershipRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_PACKAGES_TRANSFER)?;
    validation::validate_slug(&body.target_org_slug).map_err(ApiError::from)?;

    let eco = parse_ecosystem(&ecosystem_str)?;
    let normalized = normalize_package_name(&name, &eco);

    ensure_package_transfer_access(&state.db, eco.as_str(), &normalized, identity.user_id)
        .await?;

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    let package_row = sqlx::query(
        "SELECT id, name, owner_user_id, owner_org_id \
         FROM packages \
         WHERE ecosystem = $1 AND normalized_name = $2 \
         FOR UPDATE",
    )
    .bind(eco.as_str())
    .bind(&normalized)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| {
        ApiError(Error::NotFound(format!(
            "Package '{name}' not found in ecosystem '{ecosystem_str}'"
        )))
    })?;

    let package_id: Uuid = package_row
        .try_get("id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let package_name: String = package_row
        .try_get("name")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let current_owner = package_owner_from_fields(
        package_row
            .try_get::<Option<Uuid>, _>("owner_user_id")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        package_row
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
    .bind(&vec!["owner".to_owned(), "admin".to_owned()])
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    if !actor_controls_target {
        return Err(ApiError(Error::Forbidden(
            "Transferring a package into an organization requires owner or admin membership in the target organization".into(),
        )));
    }

    validate_package_transfer_target(&current_owner, target_org.id)?;

    let revoked_team_grants: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT \
         FROM team_package_access \
         WHERE package_id = $1",
    )
    .bind(package_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query("DELETE FROM team_package_access WHERE package_id = $1")
        .bind(package_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query(
        "UPDATE packages \
         SET owner_user_id = NULL, \
             owner_org_id = $1, \
             updated_at = NOW() \
         WHERE id = $2",
    )
    .bind(target_org.id)
    .bind(package_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    let (previous_owner_type, previous_owner_user_id, previous_owner_org_id) = match current_owner {
        PackageOwner::User(user_id) => ("user", Some(user_id), None),
        PackageOwner::Organization(org_id) => ("organization", None, Some(org_id)),
    };

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_package_id, metadata, occurred_at) \
         VALUES ($1, 'package_transfer', $2, $3, $4, $5, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(package_id)
    .bind(serde_json::json!({
        "ecosystem": eco.as_str(),
        "name": package_name,
        "normalized_name": normalized,
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
        "message": "Package ownership transferred",
        "package": {
            "id": package_id,
            "ecosystem": eco.as_str(),
            "name": package_name,
            "normalized_name": normalized,
        },
        "owner": {
            "type": "organization",
            "id": target_org.id,
            "slug": target_org.slug,
            "name": target_org.name,
        },
    })))
}

async fn create_release(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path((ecosystem_str, name)): Path<(String, String)>,
    Json(body): Json<CreateReleaseRequest>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    ensure_scope(&identity, SCOPE_PACKAGES_WRITE)?;

    let eco = parse_ecosystem(&ecosystem_str)?;
    let normalized = normalize_package_name(&name, &eco);
    let package_id =
        ensure_package_publish_access(&state.db, eco.as_str(), &normalized, identity.user_id)
            .await?;

    validation::validate_version(&body.version).map_err(ApiError::from)?;

    let mut release = Release::new(package_id, body.version.clone(), identity.user_id);
    release.description = body.description;
    release.changelog = body.changelog;
    release.source_ref = body.source_ref;
    release.provenance = body.provenance;
    release.is_prerelease = body
        .is_prerelease
        .unwrap_or_else(|| version_looks_prerelease(&release.version));

    sqlx::query(
        "INSERT INTO releases (id, package_id, version, status, published_by, description, changelog, \
         is_prerelease, is_yanked, yank_reason, is_deprecated, deprecation_message, source_ref, \
         provenance, published_at, updated_at) \
         VALUES ($1, $2, $3, 'quarantine', $4, $5, $6, $7, false, NULL, false, NULL, $8, $9, $10, $11)",
    )
    .bind(release.id)
    .bind(release.package_id)
    .bind(&release.version)
    .bind(release.published_by)
    .bind(&release.description)
    .bind(&release.changelog)
    .bind(release.is_prerelease)
    .bind(&release.source_ref)
    .bind(&release.provenance)
    .bind(release.published_at)
    .bind(release.updated_at)
    .execute(&state.db)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db) if db.is_unique_violation() => ApiError(Error::AlreadyExists(
            format!("Release '{}' already exists", release.version),
        )),
        _ => ApiError(Error::Database(e)),
    })?;

    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "id": release.id,
            "package_id": release.package_id,
            "version": release.version,
            "status": "quarantine",
            "is_prerelease": release.is_prerelease,
            "published_at": release.published_at,
        })),
    ))
}

async fn list_releases(
    State(state): State<AppState>,
    identity: OptionalAuthenticatedIdentity,
    Path((ecosystem_str, name)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let eco = parse_ecosystem(&ecosystem_str)?;
    let normalized = normalize_package_name(&name, &eco);
    let pkg_id =
        ensure_package_read_access(&state.db, eco.as_str(), &normalized, identity.user_id())
            .await?;
    let visible_statuses = if actor_can_write_package_by_id(&state.db, pkg_id, identity.user_id())
        .await?
    {
        release_management_visible_statuses()
    } else {
        release_history_visible_statuses()
    };

    let rows = sqlx::query(
        "SELECT version, status, is_yanked, is_deprecated, is_prerelease, published_at \
         FROM releases \
         WHERE package_id = $1 AND status::text = ANY($2) \
         ORDER BY published_at DESC",
    )
    .bind(pkg_id)
    .bind(&visible_statuses)
    .fetch_all(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    let releases: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "version": r.try_get::<String, _>("version").ok(),
                "status": r.try_get::<String, _>("status").ok(),
                "is_yanked": r.try_get::<bool, _>("is_yanked").ok(),
                "is_deprecated": r.try_get::<bool, _>("is_deprecated").ok(),
                "is_prerelease": r.try_get::<bool, _>("is_prerelease").ok(),
                "published_at": r.try_get::<chrono::DateTime<chrono::Utc>, _>("published_at").ok(),
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "releases": releases })))
}

async fn get_release(
    State(state): State<AppState>,
    identity: OptionalAuthenticatedIdentity,
    Path((ecosystem_str, name, version)): Path<(String, String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let eco = parse_ecosystem(&ecosystem_str)?;
    let release_access = ensure_release_read_access(
        &state.db,
        &eco,
        &name,
        &version,
        identity.user_id(),
    )
    .await?;

    let row = sqlx::query(
        "SELECT r.id, r.version, r.status, r.is_yanked, r.yank_reason, r.is_deprecated, \
                r.deprecation_message, r.is_prerelease, r.changelog, r.source_ref, r.published_at \
         FROM releases r \
         WHERE r.package_id = $1 AND r.version = $2",
    )
    .bind(release_access.package_id)
    .bind(&version)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| {
        ApiError(Error::NotFound(format!(
            "Release '{version}' not found for package '{name}'"
        )))
    })?;

    Ok(Json(serde_json::json!({
        "id": row.try_get::<Uuid, _>("id").ok(),
        "version": row.try_get::<String, _>("version").ok(),
        "status": Some(release_access.status),
        "is_yanked": row.try_get::<bool, _>("is_yanked").ok(),
        "yank_reason": row.try_get::<Option<String>, _>("yank_reason").ok().flatten(),
        "is_deprecated": row.try_get::<bool, _>("is_deprecated").ok(),
        "deprecation_message": row.try_get::<Option<String>, _>("deprecation_message").ok().flatten(),
        "is_prerelease": row.try_get::<bool, _>("is_prerelease").ok(),
        "changelog": row.try_get::<Option<String>, _>("changelog").ok().flatten(),
        "source_ref": row.try_get::<Option<String>, _>("source_ref").ok().flatten(),
        "published_at": row.try_get::<chrono::DateTime<chrono::Utc>, _>("published_at").ok(),
    })))
}

#[derive(Debug, Deserialize)]
struct YankRequest {
    reason: Option<String>,
}

async fn yank_release(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path((ecosystem_str, name, version)): Path<(String, String, String)>,
    Json(body): Json<YankRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_PACKAGES_WRITE)?;

    let eco = parse_ecosystem(&ecosystem_str)?;
    let normalized = normalize_package_name(&name, &eco);
    let package_id =
        ensure_package_publish_access(&state.db, eco.as_str(), &normalized, identity.user_id)
            .await?;

    let release_access = load_release_for_write(&state.db, package_id, &version).await?;
    ensure_release_allows_status_mutation(&release_access.status, "yanked")?;

    let release = sqlx::query(
        "UPDATE releases \
         SET is_yanked = true, yank_reason = $1, status = 'yanked', updated_at = NOW() \
         WHERE package_id = $2 AND version = $3 \
         RETURNING id",
    )
    .bind(&body.reason)
    .bind(package_id)
    .bind(&version)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| ApiError(Error::NotFound(format!("Release '{version}' not found"))))?;

    let release_id: Uuid = release
        .try_get("id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_package_id, target_release_id, metadata, occurred_at) \
         VALUES ($1, 'release_yank', $2, $3, $4, $5, $6, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(package_id)
    .bind(release_id)
    .bind(serde_json::json!({
        "ecosystem": eco.as_str(),
        "name": normalized,
        "version": version,
        "reason": body.reason,
    }))
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(Json(serde_json::json!({ "message": "Release yanked" })))
}

async fn unyank_release(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path((ecosystem_str, name, version)): Path<(String, String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_PACKAGES_WRITE)?;

    let eco = parse_ecosystem(&ecosystem_str)?;
    let normalized = normalize_package_name(&name, &eco);
    let package_id =
        ensure_package_publish_access(&state.db, eco.as_str(), &normalized, identity.user_id)
            .await?;

    let release_row = sqlx::query(
        "SELECT id, is_yanked, is_deprecated \
         FROM releases \
         WHERE package_id = $1 AND version = $2",
    )
    .bind(package_id)
    .bind(&version)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| ApiError(Error::NotFound(format!("Release '{version}' not found"))))?;

    let release_id: Uuid = release_row
        .try_get("id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let is_yanked = release_row
        .try_get::<bool, _>("is_yanked")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let is_deprecated = release_row
        .try_get::<bool, _>("is_deprecated")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    if !is_yanked {
        return Err(ApiError(Error::Conflict(format!(
            "Release '{version}' is not yanked"
        ))));
    }

    let restored_status = release_status_after_unyank(is_deprecated);

    sqlx::query(
        "UPDATE releases \
         SET is_yanked = false, \
             yank_reason = NULL, \
             status = $1, \
             updated_at = NOW() \
         WHERE id = $2",
    )
    .bind(restored_status)
    .bind(release_id)
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_package_id, target_release_id, metadata, occurred_at) \
         VALUES ($1, 'release_unyank', $2, $3, $4, $5, $6, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(package_id)
    .bind(release_id)
    .bind(serde_json::json!({
        "ecosystem": eco.as_str(),
        "name": normalized,
        "version": version,
        "restored_status": restored_status,
    }))
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(Json(serde_json::json!({
        "message": "Release restored",
        "version": version,
        "status": restored_status,
    })))
}

#[derive(Debug, Deserialize)]
struct DeprecateRequest {
    message: Option<String>,
}

async fn deprecate_release(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path((ecosystem_str, name, version)): Path<(String, String, String)>,
    Json(body): Json<DeprecateRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_PACKAGES_WRITE)?;

    let eco = parse_ecosystem(&ecosystem_str)?;
    let normalized = normalize_package_name(&name, &eco);
    let package_id =
        ensure_package_publish_access(&state.db, eco.as_str(), &normalized, identity.user_id)
            .await?;

    let release_access = load_release_for_write(&state.db, package_id, &version).await?;
    ensure_release_allows_status_mutation(&release_access.status, "deprecated")?;

    let release = sqlx::query(
        "UPDATE releases \
         SET is_deprecated = true, deprecation_message = $1, status = 'deprecated', updated_at = NOW() \
         WHERE package_id = $2 AND version = $3 \
         RETURNING id",
    )
    .bind(&body.message)
    .bind(package_id)
    .bind(&version)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| ApiError(Error::NotFound(format!("Release '{version}' not found"))))?;

    let release_id: Uuid = release
        .try_get("id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_package_id, target_release_id, metadata, occurred_at) \
         VALUES ($1, 'release_deprecate', $2, $3, $4, $5, $6, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(package_id)
    .bind(release_id)
    .bind(serde_json::json!({
        "ecosystem": eco.as_str(),
        "name": normalized,
        "version": version,
        "message": body.message,
    }))
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(Json(serde_json::json!({ "message": "Release deprecated" })))
}

async fn list_tags(
    State(state): State<AppState>,
    identity: OptionalAuthenticatedIdentity,
    Path((ecosystem_str, name)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let eco = parse_ecosystem(&ecosystem_str)?;
    let normalized = normalize_package_name(&name, &eco);
    let package_id =
        ensure_package_read_access(&state.db, eco.as_str(), &normalized, identity.user_id())
            .await?;

    let rows = sqlx::query(
        "SELECT cr.name, r.version, cr.updated_at \
         FROM channel_refs cr \
         JOIN releases r ON r.id = cr.release_id \
         WHERE cr.package_id = $1 \
         ORDER BY cr.name",
    )
    .bind(package_id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    let mut tags = serde_json::Map::new();
    for r in &rows {
        let tag_name: String = r.try_get("name").unwrap_or_default();
        let version: String = r.try_get("version").unwrap_or_default();
        let updated_at: Option<chrono::DateTime<chrono::Utc>> = r.try_get("updated_at").ok();
        tags.insert(
            tag_name,
            serde_json::json!({ "version": version, "updated_at": updated_at }),
        );
    }

    Ok(Json(serde_json::json!({ "tags": tags })))
}

#[derive(Debug, Deserialize)]
struct UpsertTagRequest {
    version: String,
}

async fn upsert_tag(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path((ecosystem_str, name, tag)): Path<(String, String, String)>,
    Json(body): Json<UpsertTagRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_PACKAGES_WRITE)?;

    let eco = parse_ecosystem(&ecosystem_str)?;
    let normalized = normalize_package_name(&name, &eco);
    let pkg_id =
        ensure_package_publish_access(&state.db, eco.as_str(), &normalized, identity.user_id)
            .await?;

    let rel_row = sqlx::query("SELECT id FROM releases WHERE package_id = $1 AND version = $2")
        .bind(pkg_id)
        .bind(&body.version)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?
        .ok_or_else(|| {
            ApiError(Error::NotFound(format!(
                "Release '{}' not found",
                body.version
            )))
        })?;

    let release_id: Uuid = rel_row
        .try_get("id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    sqlx::query(
        "INSERT INTO channel_refs (id, package_id, ecosystem, name, release_id, created_by, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, NOW(), NOW()) \
         ON CONFLICT (package_id, name) \
         DO UPDATE SET release_id = EXCLUDED.release_id, updated_at = NOW()",
    )
    .bind(Uuid::new_v4())
    .bind(pkg_id)
    .bind(eco.as_str())
    .bind(&tag)
    .bind(release_id)
    .bind(identity.user_id)
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(Json(serde_json::json!({
        "message": "Tag updated",
        "tag": tag,
        "version": body.version,
    })))
}

async fn publish_release(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path((ecosystem_str, name, version)): Path<(String, String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_PACKAGES_WRITE)?;

    let eco = parse_ecosystem(&ecosystem_str)?;
    let normalized = normalize_package_name(&name, &eco);
    let package_id =
        ensure_package_publish_access(&state.db, eco.as_str(), &normalized, identity.user_id)
            .await?;

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    let release_row = sqlx::query(
        "SELECT id, status, is_yanked, is_deprecated \
         FROM releases \
         WHERE package_id = $1 AND version = $2 \
         FOR UPDATE",
    )
    .bind(package_id)
    .bind(&version)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| ApiError(Error::NotFound(format!("Release '{version}' not found"))))?;

    let release_id: Uuid = release_row
        .try_get("id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let current_status = release_row
        .try_get::<String, _>("status")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let is_yanked = release_row
        .try_get::<bool, _>("is_yanked")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let is_deprecated = release_row
        .try_get::<bool, _>("is_deprecated")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    let artifact_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT \
         FROM artifacts \
         WHERE release_id = $1",
    )
    .bind(release_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    if current_status == "published" {
        tx.commit()
            .await
            .map_err(|e| ApiError(Error::Database(e)))?;

        return Ok(Json(serde_json::json!({
            "message": "Release already published",
            "version": version,
            "status": current_status,
            "artifact_count": artifact_count,
        })));
    }

    if !release_status_can_be_published(&current_status) {
        return Err(ApiError(Error::Conflict(format!(
            "Release '{version}' cannot be published from status '{current_status}'"
        ))));
    }

    if artifact_count == 0 {
        return Err(ApiError(Error::Conflict(
            "A release must have at least one uploaded artifact before publication".into(),
        )));
    }

    let next_status = finalize_release_status(is_deprecated, is_yanked);

    sqlx::query(
        "UPDATE releases \
         SET status = $1, updated_at = NOW() \
         WHERE id = $2",
    )
    .bind(next_status)
    .bind(release_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_package_id, target_release_id, metadata, occurred_at) \
         VALUES ($1, 'release_publish', $2, $3, $4, $5, $6, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(package_id)
    .bind(release_id)
    .bind(serde_json::json!({
        "ecosystem": eco.as_str(),
        "name": normalized,
        "version": version,
        "status": next_status,
        "artifact_count": artifact_count,
    }))
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    tx.commit()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    if let Err(error) = reindex_package_document(&state, package_id).await {
        tracing::warn!(
            package_id = %package_id,
            release_id = %release_id,
            error = %error,
            "Failed to reindex package after release publication"
        );
    }

    Ok(Json(serde_json::json!({
        "message": "Release published",
        "version": version,
        "status": next_status,
        "artifact_count": artifact_count,
    })))
}

async fn list_artifacts(
    State(state): State<AppState>,
    identity: OptionalAuthenticatedIdentity,
    Path((ecosystem_str, name, version)): Path<(String, String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let eco = parse_ecosystem(&ecosystem_str)?;
    let release_access = ensure_release_read_access(
        &state.db,
        &eco,
        &name,
        &version,
        identity.user_id(),
    )
    .await?;

    let rows = sqlx::query(
        "SELECT id, kind, filename, content_type, size_bytes, sha256, sha512, is_signed, signature_key_id, uploaded_at \
         FROM artifacts \
         WHERE release_id = $1 \
         ORDER BY filename",
    )
    .bind(release_access.release_id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    let artifacts = rows
        .iter()
        .map(|row| {
            serde_json::json!({
                "id": row.try_get::<Uuid, _>("id").ok(),
                "kind": row.try_get::<ArtifactKind, _>("kind").ok().map(artifact_kind_as_str),
                "filename": row.try_get::<String, _>("filename").ok(),
                "content_type": row.try_get::<String, _>("content_type").ok(),
                "size_bytes": row.try_get::<i64, _>("size_bytes").ok(),
                "sha256": row.try_get::<String, _>("sha256").ok(),
                "sha512": row.try_get::<Option<String>, _>("sha512").ok().flatten(),
                "is_signed": row.try_get::<bool, _>("is_signed").ok(),
                "signature_key_id": row.try_get::<Option<String>, _>("signature_key_id").ok().flatten(),
                "uploaded_at": row.try_get::<chrono::DateTime<chrono::Utc>, _>("uploaded_at").ok(),
            })
        })
        .collect::<Vec<_>>();

    Ok(Json(serde_json::json!({ "artifacts": artifacts })))
}

async fn upload_artifact(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path((ecosystem_str, name, version, filename)): Path<(String, String, String, String)>,
    Query(query): Query<ArtifactUploadQuery>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    ensure_scope(&identity, SCOPE_PACKAGES_WRITE)?;

    let eco = parse_ecosystem(&ecosystem_str)?;
    let normalized = normalize_package_name(&name, &eco);
    let package_id =
        ensure_package_publish_access(&state.db, eco.as_str(), &normalized, identity.user_id)
            .await?;
    let release_access = load_release_for_write(&state.db, package_id, &version).await?;
    validate_artifact_filename(&filename)?;

    if !release_status_accepts_artifact_upload(&release_access.status) {
        return Err(ApiError(Error::Conflict(format!(
            "Artifacts can only be uploaded while a release is in quarantine or scanning; current status is '{}'",
            release_access.status,
        ))));
    }

    if body.is_empty() {
        return Err(ApiError(Error::Validation(
            "Artifact uploads must not be empty".into(),
        )));
    }

    let artifact_kind = parse_artifact_kind(&query.kind)?;
    let content_type = headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_owned();
    let size_bytes = i64::try_from(body.len()).map_err(|_| {
        ApiError(Error::Validation(
            "Artifact upload is too large to be represented safely".into(),
        ))
    })?;

    let sha256 = hex::encode(Sha256::digest(&body));
    let sha512 = hex::encode(Sha512::digest(&body));
    validate_expected_sha256(query.sha256.as_deref(), &sha256)?;

    if let Some(existing_row) = sqlx::query(
        "SELECT id, storage_key, content_type, size_bytes, sha256, sha512, is_signed, signature_key_id, uploaded_at \
         FROM artifacts \
         WHERE release_id = $1 AND filename = $2",
    )
    .bind(release_access.release_id)
    .bind(&filename)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    {
        let existing_sha256 = existing_row
            .try_get::<String, _>("sha256")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

        if existing_sha256 != sha256 {
            return Err(ApiError(Error::Conflict(format!(
                "An artifact named '{}' already exists for release '{}' with different content",
                filename, version,
            ))));
        }

        return Ok((
            StatusCode::OK,
            Json(serde_json::json!({
                "id": existing_row.try_get::<Uuid, _>("id").ok(),
                "release_id": release_access.release_id,
                "kind": artifact_kind_as_str(artifact_kind),
                "filename": filename,
                "storage_key": existing_row.try_get::<String, _>("storage_key").ok(),
                "content_type": existing_row.try_get::<String, _>("content_type").ok(),
                "size_bytes": existing_row.try_get::<i64, _>("size_bytes").ok(),
                "sha256": existing_sha256,
                "sha512": existing_row.try_get::<Option<String>, _>("sha512").ok().flatten(),
                "is_signed": existing_row.try_get::<bool, _>("is_signed").ok(),
                "signature_key_id": existing_row.try_get::<Option<String>, _>("signature_key_id").ok().flatten(),
                "uploaded_at": existing_row.try_get::<chrono::DateTime<chrono::Utc>, _>("uploaded_at").ok(),
            })),
        ));
    }

    let storage_key = build_artifact_storage_key(release_access.release_id, &filename, &sha256);
    state
        .artifact_store
        .put_object(PutArtifactObject {
            storage_key: storage_key.clone(),
            content_type: content_type.clone(),
            bytes: body.clone(),
        })
        .await
        .map_err(ApiError::from)?;

    let is_signed = query.is_signed.unwrap_or(query.signature_key_id.is_some());
    let mut artifact = Artifact::new(
        release_access.release_id,
        artifact_kind,
        filename.clone(),
        storage_key.clone(),
        content_type.clone(),
        size_bytes,
        sha256.clone(),
    );
    artifact.sha512 = Some(sha512.clone());
    artifact.is_signed = is_signed;
    artifact.signature_key_id = query.signature_key_id.clone();

    let insert_result = sqlx::query(
        "INSERT INTO artifacts (id, release_id, kind, filename, storage_key, content_type, size_bytes, sha256, sha512, md5, is_signed, signature_key_id, uploaded_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NULL, $10, $11, $12) \
         ON CONFLICT (release_id, filename) DO NOTHING",
    )
    .bind(artifact.id)
    .bind(artifact.release_id)
    .bind(artifact.kind.clone())
    .bind(&artifact.filename)
    .bind(&artifact.storage_key)
    .bind(&artifact.content_type)
    .bind(artifact.size_bytes)
    .bind(&artifact.sha256)
    .bind(&artifact.sha512)
    .bind(artifact.is_signed)
    .bind(&artifact.signature_key_id)
    .bind(artifact.uploaded_at)
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    if insert_result.rows_affected() == 0 {
        let existing_row = sqlx::query(
            "SELECT id, storage_key, content_type, size_bytes, sha256, sha512, is_signed, signature_key_id, uploaded_at \
             FROM artifacts \
             WHERE release_id = $1 AND filename = $2",
        )
        .bind(release_access.release_id)
        .bind(&filename)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?
        .ok_or_else(|| {
            ApiError(Error::Internal(
                "Artifact insert conflicted but no existing artifact could be loaded".into(),
            ))
        })?;

        let existing_sha256 = existing_row
            .try_get::<String, _>("sha256")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
        if existing_sha256 != sha256 {
            return Err(ApiError(Error::Conflict(format!(
                "An artifact named '{}' already exists for release '{}' with different content",
                filename, version,
            ))));
        }

        return Ok((
            StatusCode::OK,
            Json(serde_json::json!({
                "id": existing_row.try_get::<Uuid, _>("id").ok(),
                "release_id": release_access.release_id,
                "kind": artifact_kind_as_str(artifact.kind),
                "filename": artifact.filename,
                "storage_key": existing_row.try_get::<String, _>("storage_key").ok(),
                "content_type": existing_row.try_get::<String, _>("content_type").ok(),
                "size_bytes": existing_row.try_get::<i64, _>("size_bytes").ok(),
                "sha256": existing_sha256,
                "sha512": existing_row.try_get::<Option<String>, _>("sha512").ok().flatten(),
                "is_signed": existing_row.try_get::<bool, _>("is_signed").ok(),
                "signature_key_id": existing_row.try_get::<Option<String>, _>("signature_key_id").ok().flatten(),
                "uploaded_at": existing_row.try_get::<chrono::DateTime<chrono::Utc>, _>("uploaded_at").ok(),
            })),
        ));
    }

    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "id": artifact.id,
            "release_id": artifact.release_id,
            "kind": artifact_kind_as_str(artifact.kind.clone()),
            "filename": artifact.filename,
            "storage_key": artifact.storage_key,
            "content_type": artifact.content_type,
            "size_bytes": artifact.size_bytes,
            "sha256": artifact.sha256,
            "sha512": artifact.sha512,
            "is_signed": artifact.is_signed,
            "signature_key_id": artifact.signature_key_id,
            "uploaded_at": artifact.uploaded_at,
        })),
    ))
}

async fn download_artifact(
    State(state): State<AppState>,
    identity: OptionalAuthenticatedIdentity,
    Path((ecosystem_str, name, version, filename)): Path<(String, String, String, String)>,
) -> ApiResult<Response> {
    let eco = parse_ecosystem(&ecosystem_str)?;
    let release_access = ensure_release_read_access(
        &state.db,
        &eco,
        &name,
        &version,
        identity.user_id(),
    )
    .await?;

    let artifact_row = sqlx::query(
        "SELECT storage_key, content_type, size_bytes, sha256 \
         FROM artifacts \
         WHERE release_id = $1 AND filename = $2",
    )
    .bind(release_access.release_id)
    .bind(&filename)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| ApiError(Error::NotFound(format!("Artifact '{}' not found", filename))))?;

    let storage_key = artifact_row
        .try_get::<String, _>("storage_key")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let sha256 = artifact_row
        .try_get::<String, _>("sha256")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let declared_content_type = artifact_row
        .try_get::<String, _>("content_type")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    let stored_object = state
        .artifact_store
        .get_object(&storage_key)
        .await
        .map_err(ApiError::from)?
        .ok_or_else(|| {
            ApiError(Error::Internal(
                "Artifact metadata exists but the stored object is missing".into(),
            ))
        })?;

    sqlx::query("UPDATE packages SET download_count = download_count + 1 WHERE id = $1")
        .bind(release_access.package_id)
        .execute(&state.db)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    let content_type = if stored_object.content_type.is_empty() {
        declared_content_type
    } else {
        stored_object.content_type.clone()
    };
    let content_length = stored_object.bytes.len().to_string();
    let disposition = format!("attachment; filename=\"{}\"", filename.replace('"', ""));

    let response = Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, content_type)
        .header(CONTENT_LENGTH, content_length)
        .header(CONTENT_DISPOSITION, disposition)
        .header("x-checksum-sha256", sha256)
        .body(Body::from(stored_object.bytes))
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    Ok(response)
}

fn package_owner_from_fields(
    owner_user_id: Option<Uuid>,
    owner_org_id: Option<Uuid>,
) -> ApiResult<PackageOwner> {
    match (owner_user_id, owner_org_id) {
        (Some(user_id), None) => Ok(PackageOwner::User(user_id)),
        (None, Some(org_id)) => Ok(PackageOwner::Organization(org_id)),
        (None, None) => Err(ApiError(Error::Internal("Package owner is not set".into()))),
        (Some(_), Some(_)) => Err(ApiError(Error::Internal(
            "Package owner state is invalid".into(),
        ))),
    }
}

fn release_history_visible_statuses() -> Vec<String> {
    RELEASE_HISTORY_VISIBLE_STATUSES
        .iter()
        .map(|status| (*status).to_owned())
        .collect()
}

fn release_management_visible_statuses() -> Vec<String> {
    RELEASE_MANAGEMENT_VISIBLE_STATUSES
        .iter()
        .map(|status| (*status).to_owned())
        .collect()
}

fn release_status_after_unyank(is_deprecated: bool) -> &'static str {
    if is_deprecated {
        "deprecated"
    } else {
        "published"
    }
}

fn release_status_accepts_artifact_upload(status: &str) -> bool {
    matches!(status, "quarantine" | "scanning")
}

fn release_status_allows_direct_read(status: &str) -> bool {
    RELEASE_HISTORY_VISIBLE_STATUSES.contains(&status)
}

fn release_status_can_be_published(status: &str) -> bool {
    matches!(status, "quarantine" | "scanning" | "published")
}

fn ensure_release_allows_status_mutation(status: &str, action: &str) -> ApiResult<()> {
    if matches!(status, "quarantine" | "scanning" | "deleted") {
        return Err(ApiError(Error::Conflict(format!(
            "Releases in status '{status}' cannot be {action}"
        ))));
    }

    Ok(())
}

fn finalize_release_status(is_deprecated: bool, is_yanked: bool) -> &'static str {
    if is_yanked {
        "yanked"
    } else if is_deprecated {
        "deprecated"
    } else {
        "published"
    }
}

fn version_looks_prerelease(version: &str) -> bool {
    version.contains('-')
}

fn parse_artifact_kind(input: &str) -> ApiResult<ArtifactKind> {
    match input.trim().to_ascii_lowercase().as_str() {
        "tarball" => Ok(ArtifactKind::Tarball),
        "wheel" => Ok(ArtifactKind::Wheel),
        "sdist" => Ok(ArtifactKind::Sdist),
        "jar" => Ok(ArtifactKind::Jar),
        "pom" => Ok(ArtifactKind::Pom),
        "gem" => Ok(ArtifactKind::Gem),
        "nupkg" => Ok(ArtifactKind::Nupkg),
        "snupkg" => Ok(ArtifactKind::Snupkg),
        "oci_manifest" => Ok(ArtifactKind::OciManifest),
        "oci_layer" => Ok(ArtifactKind::OciLayer),
        "crate" => Ok(ArtifactKind::Crate),
        "composer_zip" => Ok(ArtifactKind::ComposerZip),
        "checksum" => Ok(ArtifactKind::Checksum),
        "signature" => Ok(ArtifactKind::Signature),
        "sbom" => Ok(ArtifactKind::Sbom),
        "source_zip" => Ok(ArtifactKind::SourceZip),
        other => Err(ApiError(Error::Validation(format!(
            "Unknown artifact kind: {other}"
        )))),
    }
}

fn artifact_kind_as_str(kind: ArtifactKind) -> &'static str {
    match kind {
        ArtifactKind::Tarball => "tarball",
        ArtifactKind::Wheel => "wheel",
        ArtifactKind::Sdist => "sdist",
        ArtifactKind::Jar => "jar",
        ArtifactKind::Pom => "pom",
        ArtifactKind::Gem => "gem",
        ArtifactKind::Nupkg => "nupkg",
        ArtifactKind::Snupkg => "snupkg",
        ArtifactKind::OciManifest => "oci_manifest",
        ArtifactKind::OciLayer => "oci_layer",
        ArtifactKind::Crate => "crate",
        ArtifactKind::ComposerZip => "composer_zip",
        ArtifactKind::Checksum => "checksum",
        ArtifactKind::Signature => "signature",
        ArtifactKind::Sbom => "sbom",
        ArtifactKind::SourceZip => "source_zip",
    }
}

fn validate_artifact_filename(filename: &str) -> ApiResult<()> {
    if filename.trim().is_empty() {
        return Err(ApiError(Error::Validation(
            "Artifact filename must not be empty".into(),
        )));
    }

    if filename.contains('/') || filename.contains('\\') {
        return Err(ApiError(Error::Validation(
            "Artifact filename must not contain path separators".into(),
        )));
    }

    if filename.chars().any(|character| character.is_control()) {
        return Err(ApiError(Error::Validation(
            "Artifact filename must not contain control characters".into(),
        )));
    }

    Ok(())
}

fn validate_expected_sha256(expected: Option<&str>, actual: &str) -> ApiResult<()> {
    let Some(expected) = expected else {
        return Ok(());
    };

    let normalized = expected.trim().to_ascii_lowercase();
    if normalized.len() != 64 || !normalized.chars().all(|character| character.is_ascii_hexdigit()) {
        return Err(ApiError(Error::Validation(
            "The supplied sha256 checksum must be a 64-character hexadecimal value".into(),
        )));
    }

    if normalized != actual {
        return Err(ApiError(Error::Validation(
            "The supplied sha256 checksum does not match the uploaded artifact".into(),
        )));
    }

    Ok(())
}

fn build_artifact_storage_key(release_id: Uuid, filename: &str, sha256: &str) -> String {
    format!("releases/{release_id}/artifacts/{sha256}/{filename}")
}

async fn load_release_for_write(
    db: &sqlx::PgPool,
    package_id: Uuid,
    version: &str,
) -> ApiResult<ReleaseAccess> {
    let row = sqlx::query(
        "SELECT id, status, is_yanked, is_deprecated \
         FROM releases \
         WHERE package_id = $1 AND version = $2",
    )
    .bind(package_id)
    .bind(version)
    .fetch_optional(db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| ApiError(Error::NotFound(format!("Release '{version}' not found"))))?;

    Ok(ReleaseAccess {
        package_id,
        release_id: row
            .try_get("id")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        status: row
            .try_get("status")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
    })
}

async fn ensure_release_read_access(
    db: &sqlx::PgPool,
    ecosystem: &Ecosystem,
    package_name: &str,
    version: &str,
    actor_user_id: Option<Uuid>,
) -> ApiResult<ReleaseAccess> {
    let normalized = normalize_package_name(package_name, ecosystem);
    let package_id = ensure_package_read_access(db, ecosystem.as_str(), &normalized, actor_user_id).await?;
    let release = load_release_for_write(db, package_id, version).await?;

    if release_status_allows_direct_read(&release.status) {
        return Ok(release);
    }

    if actor_can_write_package_by_id(db, package_id, actor_user_id).await? {
        return Ok(release);
    }

    Err(ApiError(Error::NotFound(format!(
        "Release '{version}' not found for package '{package_name}'"
    ))))
}

fn validate_package_transfer_target(
    current_owner: &PackageOwner,
    target_org_id: Uuid,
) -> ApiResult<()> {
    if matches!(current_owner, PackageOwner::Organization(current_org_id) if *current_org_id == target_org_id)
    {
        return Err(ApiError(Error::Conflict(
            "The selected organization already owns this package".into(),
        )));
    }

    Ok(())
}

async fn load_repository_package_creation_target(
    db: &sqlx::PgPool,
    repository_id: Uuid,
) -> ApiResult<RepositoryPackageCreationTarget> {
    let row = sqlx::query(
        "SELECT r.id, r.slug, r.kind, r.visibility, r.owner_user_id, r.owner_org_id, \
                u.username AS owner_username, o.slug AS owner_org_slug \
         FROM repositories r \
         LEFT JOIN users u ON u.id = r.owner_user_id \
         LEFT JOIN organizations o ON o.id = r.owner_org_id \
         WHERE r.id = $1",
    )
    .bind(repository_id)
    .fetch_optional(db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| ApiError(Error::NotFound("Repository not found".into())))?;

    Ok(RepositoryPackageCreationTarget {
        id: row
            .try_get("id")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        slug: row
            .try_get("slug")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        kind: row
            .try_get("kind")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        visibility: row
            .try_get("visibility")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        owner_user_id: row
            .try_get::<Option<Uuid>, _>("owner_user_id")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        owner_org_id: row
            .try_get::<Option<Uuid>, _>("owner_org_id")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
    })
}

fn validate_package_creation_repository_kind(kind: &str) -> ApiResult<()> {
    if PACKAGE_CREATION_ALLOWED_REPOSITORY_KINDS.contains(&kind) {
        return Ok(());
    }

    Err(ApiError(Error::Conflict(
        "Packages can only be created in public, private, staging, or release repositories".into(),
    )))
}

fn parse_package_visibility(input: &str) -> ApiResult<Visibility> {
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

fn visibility_as_str(visibility: &Visibility) -> &'static str {
    match visibility {
        Visibility::Public => "public",
        Visibility::Private => "private",
        Visibility::InternalOrg => "internal_org",
        Visibility::Unlisted => "unlisted",
        Visibility::Quarantined => "quarantined",
    }
}

fn derive_package_visibility(
    requested_visibility: Option<Visibility>,
    repository_visibility: Visibility,
    repository_is_org_owned: bool,
) -> ApiResult<Visibility> {
    let package_visibility = requested_visibility.unwrap_or_else(|| repository_visibility.clone());

    if repository_visibility == Visibility::Quarantined
        && package_visibility != Visibility::Quarantined
    {
        return Err(ApiError(Error::Validation(
            "Packages in quarantined repositories must remain quarantined".into(),
        )));
    }

    if package_visibility == Visibility::InternalOrg && !repository_is_org_owned {
        return Err(ApiError(Error::Validation(
            "internal_org visibility requires an organization-owned repository".into(),
        )));
    }

    if visibility_scope_rank(&package_visibility) > visibility_scope_rank(&repository_visibility) {
        return Err(ApiError(Error::Validation(
            "Package visibility cannot be broader than the enclosing repository visibility".into(),
        )));
    }

    Ok(package_visibility)
}

fn visibility_scope_rank(visibility: &Visibility) -> u8 {
    match visibility {
        Visibility::Public => 2,
        Visibility::Unlisted => 1,
        Visibility::Private | Visibility::InternalOrg | Visibility::Quarantined => 0,
    }
}

fn extract_namespace_claim_value(ecosystem: &Ecosystem, package_name: &str) -> Option<String> {
    match ecosystem {
        Ecosystem::Npm | Ecosystem::Bun => package_name.strip_prefix('@').and_then(|_| {
            package_name
                .split_once('/')
                .map(|(scope, _)| scope.to_owned())
        }),
        Ecosystem::Composer => package_name
            .split_once('/')
            .map(|(vendor, _)| vendor.to_owned()),
        Ecosystem::Maven => package_name
            .split_once(':')
            .map(|(group_id, _)| group_id.to_owned()),
        _ => None,
    }
}

async fn validate_namespace_claim_for_package(
    db: &sqlx::PgPool,
    ecosystem: &Ecosystem,
    package_name: &str,
    owner_user_id: Option<Uuid>,
    owner_org_id: Option<Uuid>,
) -> ApiResult<()> {
    let Some(namespace) = extract_namespace_claim_value(ecosystem, package_name) else {
        return Ok(());
    };

    let Some(row) = sqlx::query(
        "SELECT owner_user_id, owner_org_id \
         FROM namespace_claims \
         WHERE ecosystem = $1 AND namespace = $2",
    )
    .bind(ecosystem.as_str())
    .bind(&namespace)
    .fetch_optional(db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    else {
        return Ok(());
    };

    let claim_owner_user_id = row
        .try_get::<Option<Uuid>, _>("owner_user_id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let claim_owner_org_id = row
        .try_get::<Option<Uuid>, _>("owner_org_id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    if claim_owner_user_id != owner_user_id || claim_owner_org_id != owner_org_id {
        return Err(ApiError(Error::Forbidden(format!(
            "Namespace '{}' is claimed by another owner",
            namespace
        ))));
    }

    Ok(())
}

fn join_policy_violations(violations: &[PolicyViolation]) -> String {
    violations
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

async fn index_package_after_creation(
    state: &AppState,
    package: &Package,
) -> publaryn_core::Result<()> {
    reindex_package_document(state, package.id).await
}

async fn reindex_package_document(state: &AppState, package_id: Uuid) -> publaryn_core::Result<()> {
    let row = sqlx::query(
        "SELECT p.id, p.name, p.normalized_name, p.display_name, p.description, p.ecosystem, \
                p.keywords, p.download_count, p.is_deprecated, p.visibility, p.updated_at, \
                u.username AS owner_username, o.slug AS owner_org_slug, \
                latest_release.version AS latest_version \
         FROM packages p \
         LEFT JOIN users u ON u.id = p.owner_user_id \
         LEFT JOIN organizations o ON o.id = p.owner_org_id \
         LEFT JOIN LATERAL ( \
             SELECT version \
             FROM releases \
             WHERE package_id = p.id AND status::text = ANY($2) \
             ORDER BY published_at DESC \
             LIMIT 1 \
         ) latest_release ON true \
         WHERE p.id = $1",
    )
    .bind(package_id)
    .bind(&release_history_visible_statuses())
    .fetch_optional(&state.db)
    .await
    .map_err(Error::Database)?
    .ok_or_else(|| Error::NotFound(format!("Package '{package_id}' not found for indexing")))?;

    let owner_name = row
        .try_get::<Option<String>, _>("owner_org_slug")
        .map_err(|e| Error::Internal(e.to_string()))?
        .or_else(|| row.try_get::<Option<String>, _>("owner_username").ok().flatten());

    let document = PackageDocument {
        id: row
            .try_get::<Uuid, _>("id")
            .map_err(|e| Error::Internal(e.to_string()))?
            .to_string(),
        name: row
            .try_get("name")
            .map_err(|e| Error::Internal(e.to_string()))?,
        normalized_name: row
            .try_get("normalized_name")
            .map_err(|e| Error::Internal(e.to_string()))?,
        display_name: row
            .try_get::<Option<String>, _>("display_name")
            .map_err(|e| Error::Internal(e.to_string()))?,
        description: row
            .try_get::<Option<String>, _>("description")
            .map_err(|e| Error::Internal(e.to_string()))?,
        ecosystem: row
            .try_get::<String, _>("ecosystem")
            .map_err(|e| Error::Internal(e.to_string()))?,
        keywords: row
            .try_get::<Vec<String>, _>("keywords")
            .map_err(|e| Error::Internal(e.to_string()))?,
        latest_version: row
            .try_get::<Option<String>, _>("latest_version")
            .map_err(|e| Error::Internal(e.to_string()))?,
        download_count: row
            .try_get::<i64, _>("download_count")
            .map_err(|e| Error::Internal(e.to_string()))?,
        is_deprecated: row
            .try_get::<bool, _>("is_deprecated")
            .map_err(|e| Error::Internal(e.to_string()))?,
        visibility: row
            .try_get::<String, _>("visibility")
            .map_err(|e| Error::Internal(e.to_string()))?,
        owner_name,
        updated_at: row
            .try_get::<chrono::DateTime<chrono::Utc>, _>("updated_at")
            .map_err(|e| Error::Internal(e.to_string()))?
            .to_rfc3339(),
    };

    state.search.index_package(document).await
}

#[cfg(test)]
mod tests {
    use publaryn_core::{
        domain::{namespace::Ecosystem, repository::Visibility},
        policy::PolicyViolation,
    };
    use uuid::Uuid;

    use super::{
        build_artifact_storage_key, derive_package_visibility, ensure_release_allows_status_mutation,
        extract_namespace_claim_value, finalize_release_status, join_policy_violations,
        package_owner_from_fields, release_history_visible_statuses,
        release_management_visible_statuses, release_status_accepts_artifact_upload,
        release_status_after_unyank, release_status_allows_direct_read,
        release_status_can_be_published, validate_artifact_filename,
        validate_expected_sha256, validate_package_creation_repository_kind,
        validate_package_transfer_target, version_looks_prerelease, visibility_scope_rank,
        PackageOwner,
    };

    #[test]
    fn package_owner_from_fields_accepts_user_owner() {
        let owner = package_owner_from_fields(Some(Uuid::new_v4()), None)
            .expect("user-owned packages should parse");

        assert!(matches!(owner, PackageOwner::User(_)));
    }

    #[test]
    fn package_owner_from_fields_rejects_missing_owner() {
        let error = package_owner_from_fields(None, None)
            .expect_err("packages without an owner should be rejected");

        assert_eq!(
            error.0.to_string(),
            "Internal error: Package owner is not set"
        );
    }

    #[test]
    fn package_owner_from_fields_rejects_invalid_double_owner_state() {
        let error = package_owner_from_fields(Some(Uuid::new_v4()), Some(Uuid::new_v4()))
            .expect_err("packages must not have both a user owner and an org owner");

        assert_eq!(
            error.0.to_string(),
            "Internal error: Package owner state is invalid"
        );
    }

    #[test]
    fn package_transfer_rejects_same_target_org() {
        let org_id = Uuid::new_v4();
        let error = validate_package_transfer_target(&PackageOwner::Organization(org_id), org_id)
            .expect_err("package transfer should reject the current owning organization");

        assert_eq!(
            error.0.to_string(),
            "Conflict: The selected organization already owns this package"
        );
    }

    #[test]
    fn package_transfer_allows_user_owned_package_to_org() {
        validate_package_transfer_target(&PackageOwner::User(Uuid::new_v4()), Uuid::new_v4())
            .expect("user-owned packages should be transferable to a new organization");
    }

    #[test]
    fn release_history_visible_statuses_include_yanked_and_deprecated() {
        let statuses = release_history_visible_statuses();

        assert!(statuses.contains(&"published".to_owned()));
        assert!(statuses.contains(&"deprecated".to_owned()));
        assert!(statuses.contains(&"yanked".to_owned()));
        assert!(!statuses.contains(&"deleted".to_owned()));
    }

    #[test]
    fn management_release_statuses_include_quarantine_states() {
        let statuses = release_management_visible_statuses();

        assert!(statuses.contains(&"quarantine".to_owned()));
        assert!(statuses.contains(&"scanning".to_owned()));
        assert!(statuses.contains(&"published".to_owned()));
    }

    #[test]
    fn direct_release_reads_hide_quarantine_statuses() {
        assert!(release_status_allows_direct_read("published"));
        assert!(release_status_allows_direct_read("deprecated"));
        assert!(!release_status_allows_direct_read("quarantine"));
        assert!(!release_status_allows_direct_read("scanning"));
    }

    #[test]
    fn artifact_uploads_are_allowed_only_before_final_publication() {
        assert!(release_status_accepts_artifact_upload("quarantine"));
        assert!(release_status_accepts_artifact_upload("scanning"));
        assert!(!release_status_accepts_artifact_upload("published"));
        assert!(!release_status_accepts_artifact_upload("yanked"));
    }

    #[test]
    fn publish_is_allowed_only_from_quarantine_or_scanning_or_idempotent_published() {
        assert!(release_status_can_be_published("quarantine"));
        assert!(release_status_can_be_published("scanning"));
        assert!(release_status_can_be_published("published"));
        assert!(!release_status_can_be_published("deprecated"));
    }

    #[test]
    fn finalize_release_status_prefers_yanked_over_deprecated() {
        assert_eq!(finalize_release_status(false, false), "published");
        assert_eq!(finalize_release_status(true, false), "deprecated");
        assert_eq!(finalize_release_status(true, true), "yanked");
    }

    #[test]
    fn release_status_mutation_rejects_quarantine_releases() {
        let error = ensure_release_allows_status_mutation("quarantine", "yanked")
            .expect_err("quarantined releases should not be mutable through yank/deprecate routes");

        assert_eq!(
            error.0.to_string(),
            "Conflict: Releases in status 'quarantine' cannot be yanked"
        );
    }

    #[test]
    fn prerelease_detection_uses_hyphenated_versions() {
        assert!(version_looks_prerelease("1.2.3-rc.1"));
        assert!(!version_looks_prerelease("1.2.3"));
    }

    #[test]
    fn artifact_filename_validation_rejects_path_separators() {
        let error = validate_artifact_filename("../../payload.tgz")
            .expect_err("artifact filenames must not allow path traversal");

        assert_eq!(
            error.0.to_string(),
            "Validation error: Artifact filename must not contain path separators"
        );
    }

    #[test]
    fn artifact_storage_keys_include_release_and_checksum() {
        let release_id = Uuid::nil();
        let key = build_artifact_storage_key(release_id, "demo-1.0.0.tgz", "abc123");

        assert_eq!(
            key,
            "releases/00000000-0000-0000-0000-000000000000/artifacts/abc123/demo-1.0.0.tgz"
        );
    }

    #[test]
    fn expected_sha256_validation_rejects_mismatches() {
        let error = validate_expected_sha256(Some("def456"), &"a".repeat(64))
            .expect_err("mismatched checksums should be rejected");

        assert_eq!(
            error.0.to_string(),
            "Validation error: The supplied sha256 checksum must be a 64-character hexadecimal value"
        );

        let mismatch = validate_expected_sha256(Some(&"b".repeat(64)), &"a".repeat(64))
            .expect_err("different checksums should be rejected");

        assert_eq!(
            mismatch.0.to_string(),
            "Validation error: The supplied sha256 checksum does not match the uploaded artifact"
        );
    }

    #[test]
    fn release_status_after_unyank_restores_published_for_normal_release() {
        assert_eq!(release_status_after_unyank(false), "published");
    }

    #[test]
    fn release_status_after_unyank_restores_deprecated_for_deprecated_release() {
        assert_eq!(release_status_after_unyank(true), "deprecated");
    }

    #[test]
    fn package_creation_rejects_proxy_repositories() {
        let error = validate_package_creation_repository_kind("proxy")
            .expect_err("proxy repositories must not accept created packages");

        assert_eq!(
            error.0.to_string(),
            "Conflict: Packages can only be created in public, private, staging, or release repositories"
        );
    }

    #[test]
    fn package_visibility_defaults_to_repository_visibility() {
        let visibility = derive_package_visibility(None, Visibility::Public, false)
            .expect("repository visibility should be reusable as package default");

        assert_eq!(visibility, Visibility::Public);
    }

    #[test]
    fn package_visibility_rejects_broader_scope_than_repository() {
        let error = derive_package_visibility(Some(Visibility::Public), Visibility::Private, false)
            .expect_err("package visibility must not be broader than repository visibility");

        assert_eq!(
            error.0.to_string(),
            "Validation error: Package visibility cannot be broader than the enclosing repository visibility"
        );
    }

    #[test]
    fn internal_org_visibility_requires_org_owned_repository() {
        let error =
            derive_package_visibility(Some(Visibility::InternalOrg), Visibility::Public, false)
                .expect_err("internal_org visibility must require an org-owned repository");

        assert_eq!(
            error.0.to_string(),
            "Validation error: internal_org visibility requires an organization-owned repository"
        );
    }

    #[test]
    fn quarantined_repository_forces_quarantined_package_visibility() {
        let error =
            derive_package_visibility(Some(Visibility::Private), Visibility::Quarantined, true)
                .expect_err("quarantined repositories must keep packages quarantined");

        assert_eq!(
            error.0.to_string(),
            "Validation error: Packages in quarantined repositories must remain quarantined"
        );
    }

    #[test]
    fn visibility_scope_rank_treats_public_as_broader_than_unlisted() {
        assert!(
            visibility_scope_rank(&Visibility::Public)
                > visibility_scope_rank(&Visibility::Unlisted)
        );
        assert_eq!(visibility_scope_rank(&Visibility::Private), 0);
    }

    #[test]
    fn namespace_extraction_supports_scoped_ecosystems() {
        assert_eq!(
            extract_namespace_claim_value(&Ecosystem::Npm, "@acme/widget"),
            Some("@acme".to_owned())
        );
        assert_eq!(
            extract_namespace_claim_value(&Ecosystem::Composer, "acme/widget"),
            Some("acme".to_owned())
        );
        assert_eq!(
            extract_namespace_claim_value(&Ecosystem::Maven, "com.acme:widget"),
            Some("com.acme".to_owned())
        );
        assert_eq!(
            extract_namespace_claim_value(&Ecosystem::Pypi, "acme-widget"),
            None
        );
    }

    #[test]
    fn policy_violations_are_joined_into_a_readable_message() {
        let message = join_policy_violations(&[
            PolicyViolation::ReservedName("admin".to_owned()),
            PolicyViolation::NamespaceMismatch,
        ]);

        assert_eq!(
            message,
            "Package name 'admin' is reserved; Package name does not match the claimed namespace"
        );
    }
}
