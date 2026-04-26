//! Axum route handlers for the NuGet V3 protocol.
//!
//! These handlers are designed to be mounted under `/nuget` in the main API
//! router. They implement the core NuGet V3 server resources:
//!
//! - **Service index** (`GET /v3/index.json`)
//! - **Package publish** (`PUT /v2/package`)
//! - **Unlist / relist** (`DELETE`/`POST /v2/package/{id}/{version}`)
//! - **Flat container** — version listing, `.nupkg` download, `.nuspec` download
//! - **Registration** — package metadata for resolution
//! - **Search** — keyword search

use axum::{
    extract::{Multipart, Path, Query, State},
    http::{
        header::{AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE},
        HeaderMap, HeaderName, StatusCode,
    },
    response::{IntoResponse, Response},
    routing::{delete, get, put},
    Json, Router,
};
use bytes::Bytes;
use chrono::Utc;
use serde::Deserialize;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use publaryn_core::{
    authz_queries,
    domain::{
        artifact::{Artifact, ArtifactKind},
        release::Release,
        repository::Visibility,
    },
    error::Error,
};

use crate::{
    metadata::{
        self, RegistrationInput, RegistrationVersionInput, SearchResultInput, SearchVersionInput,
    },
    name::{
        normalize_nuget_id, normalize_nuget_version, nupkg_filename, validate_nuget_package_id,
    },
    nuspec,
    publish::{self, ParsedNuGetPublish},
};

// ─── X-NuGet-ApiKey header ───────────────────────────────────────────────────

static X_NUGET_APIKEY: HeaderName = HeaderName::from_static("x-nuget-apikey");

// ─── Shared state trait ──────────────────────────────────────────────────────

/// Trait abstracting the application state needed by NuGet adapter routes.
///
/// The API crate's `AppState` implements this via a bridge, keeping the
/// adapter free from circular dependencies.
pub trait NuGetAppState: Clone + Send + Sync + 'static {
    fn db(&self) -> &PgPool;
    fn artifact_put(
        &self,
        key: String,
        content_type: String,
        bytes: Bytes,
    ) -> impl std::future::Future<Output = Result<(), Error>> + Send;
    fn artifact_get(
        &self,
        key: &str,
    ) -> impl std::future::Future<Output = Result<Option<StoredObject>, Error>> + Send;
    fn base_url(&self) -> &str;
    fn jwt_secret(&self) -> &str;
    fn jwt_issuer(&self) -> &str;
    fn reindex_package_document(
        &self,
        package_id: Uuid,
    ) -> impl std::future::Future<Output = Result<(), Error>> + Send;
    fn search_packages(
        &self,
        query: &str,
        take: u32,
        skip: u32,
        actor_user_id: Option<Uuid>,
    ) -> impl std::future::Future<Output = Result<NuGetSearchResults, Error>> + Send;
}

/// A retrieved object from artifact storage.
#[derive(Debug, Clone)]
pub struct StoredObject {
    pub content_type: String,
    pub bytes: Bytes,
}

/// A search result projected for the NuGet search response.
#[derive(Debug, Clone)]
pub struct NuGetSearchHit {
    pub id: String,
    pub version: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub total_downloads: i64,
}

/// Search results projected for the NuGet search response format.
#[derive(Debug, Clone)]
pub struct NuGetSearchResults {
    pub total: u64,
    pub hits: Vec<NuGetSearchHit>,
}

/// Identity extracted from an API key or bearer token.
#[derive(Debug, Clone)]
pub struct NuGetIdentity {
    pub user_id: Uuid,
    pub token_id: Option<Uuid>,
    pub scopes: Vec<String>,
}

// ─── Router ──────────────────────────────────────────────────────────────────

/// Build the NuGet adapter router.
///
/// Mount under `/nuget` in the main API router.
pub fn router<S: NuGetAppState>() -> Router<S> {
    Router::new()
        // Service index
        .route("/v3/index.json", get(service_index::<S>))
        // Publish (push)
        .route("/v2/package", put(push_package::<S>))
        // Unlist (delete) / Relist
        .route(
            "/v2/package/{id}/{version}",
            delete(unlist_package::<S>).post(relist_package::<S>),
        )
        // Flat container
        .route("/v3-flatcontainer/{id}/index.json", get(get_versions::<S>))
        .route(
            "/v3-flatcontainer/{id}/{version}/{filename}",
            get(download_content::<S>),
        )
        // Registration
        .route(
            "/v3/registration/{id}/index.json",
            get(get_registration_index::<S>),
        )
        // Search
        .route("/v3/search", get(search::<S>))
}

// ─── Auth helpers ────────────────────────────────────────────────────────────

/// Extract an API key from the `X-NuGet-ApiKey` header, or fall back to
/// `Authorization: Bearer`.
fn extract_api_key(headers: &HeaderMap) -> Option<&str> {
    // X-NuGet-ApiKey takes precedence
    if let Some(val) = headers.get(&X_NUGET_APIKEY) {
        if let Ok(s) = val.to_str() {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
    }

    // Fall back to Authorization: Bearer
    headers.get(AUTHORIZATION)?.to_str().ok().and_then(|val| {
        let mut parts = val.splitn(2, ' ');
        let scheme = parts.next()?;
        let token = parts.next()?.trim();
        if scheme.eq_ignore_ascii_case("bearer") && !token.is_empty() {
            Some(token)
        } else {
            None
        }
    })
}

/// Authenticate a NuGet request using API token or JWT.
async fn authenticate<S: NuGetAppState>(
    state: &S,
    headers: &HeaderMap,
) -> Result<NuGetIdentity, Response> {
    let token = extract_api_key(headers).ok_or_else(|| {
        nuget_error_response(
            StatusCode::UNAUTHORIZED,
            "Authentication required. Provide an X-NuGet-ApiKey header.",
        )
    })?;

    // Try API token (pub_ prefix)
    if token.starts_with("pub_") {
        let token_hash = publaryn_core::security::hash_token(token);
        let row = sqlx::query(
            "SELECT id, user_id, scopes, expires_at, kind \
             FROM tokens \
             WHERE token_hash = $1 AND is_revoked = false",
        )
        .bind(&token_hash)
        .fetch_optional(state.db())
        .await
        .map_err(|_| nuget_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Internal error"))?
        .ok_or_else(|| {
            nuget_error_response(StatusCode::UNAUTHORIZED, "Invalid or revoked API key")
        })?;

        let expires_at = row
            .try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("expires_at")
            .unwrap_or(None);
        if expires_at.is_some_and(|exp| exp <= Utc::now()) {
            return Err(nuget_error_response(
                StatusCode::UNAUTHORIZED,
                "API key has expired",
            ));
        }

        let token_kind: String = row.try_get("kind").map_err(|_| {
            nuget_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Internal error")
        })?;
        if token_kind == "oidc_derived" {
            return Err(nuget_error_response(
                StatusCode::UNAUTHORIZED,
                "OIDC-derived tokens are not valid for NuGet operations",
            ));
        }

        let token_id: Uuid = row.try_get("id").map_err(|_| {
            nuget_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Internal error")
        })?;
        let user_id: Option<Uuid> = row.try_get("user_id").unwrap_or(None);
        let user_id = user_id.ok_or_else(|| {
            nuget_error_response(
                StatusCode::UNAUTHORIZED,
                "API key is not associated with a user",
            )
        })?;
        let scopes: Vec<String> = row.try_get("scopes").unwrap_or_default();

        // Update last_used_at (fire-and-forget)
        let _ = sqlx::query("UPDATE tokens SET last_used_at = NOW() WHERE id = $1")
            .bind(token_id)
            .execute(state.db())
            .await;

        return Ok(NuGetIdentity {
            user_id,
            token_id: Some(token_id),
            scopes,
        });
    }

    // Try JWT
    let claims = publaryn_auth::validate_token(token, state.jwt_secret(), state.jwt_issuer())
        .map_err(|_| {
            nuget_error_response(StatusCode::UNAUTHORIZED, "Invalid or expired API key")
        })?;

    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| nuget_error_response(StatusCode::UNAUTHORIZED, "Invalid token subject"))?;
    let token_id = Uuid::parse_str(&claims.jti).ok();

    Ok(NuGetIdentity {
        user_id,
        token_id,
        scopes: claims.scopes,
    })
}

fn identity_has_scope(identity: &NuGetIdentity, scope: &str) -> bool {
    identity.scopes.iter().any(|s| s == scope)
}

// ─── Error format ────────────────────────────────────────────────────────────

fn nuget_error_response(status: StatusCode, message: &str) -> Response {
    // NuGet clients don't have a strict error response format requirement,
    // but a JSON body with a message is widely understood.
    let body = serde_json::json!({ "error": message });
    (status, Json(body)).into_response()
}

// ─── Access control helpers ──────────────────────────────────────────────────

async fn has_package_write_access(
    db: &PgPool,
    package_id: Uuid,
    owner_user_id: Option<Uuid>,
    owner_org_id: Option<Uuid>,
    actor_user_id: Uuid,
) -> bool {
    if owner_user_id == Some(actor_user_id) {
        return true;
    }

    if let Some(org_id) = owner_org_id {
        let roles: Vec<String> = vec![
            "owner".into(),
            "admin".into(),
            "maintainer".into(),
            "publisher".into(),
        ];
        let result = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (\
                 SELECT 1 FROM org_memberships \
                 WHERE org_id = $1 AND user_id = $2 AND role::text = ANY($3)\
             )",
        )
        .bind(org_id)
        .bind(actor_user_id)
        .bind(&roles)
        .fetch_one(db)
        .await;

        if result.unwrap_or(false) {
            return true;
        }

        let permissions: Vec<String> = vec!["admin".into(), "publish".into()];
        let delegated = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (\
                 SELECT 1 \
                 FROM team_package_access tpa \
                 JOIN team_memberships tm ON tm.team_id = tpa.team_id \
                 JOIN teams t ON t.id = tpa.team_id \
                 JOIN packages p ON p.id = tpa.package_id \
                 WHERE tpa.package_id = $1 \
                   AND tm.user_id = $2 \
                   AND t.org_id = p.owner_org_id \
                   AND tpa.permission::text = ANY($3)\
             )",
        )
        .bind(package_id)
        .bind(actor_user_id)
        .bind(&permissions)
        .fetch_one(db)
        .await;

        return delegated.unwrap_or(false);
    }

    false
}

async fn actor_has_any_team_package_access(
    db: &PgPool,
    package_id: Uuid,
    actor_user_id: Uuid,
) -> Result<bool, Response> {
    authz_queries::actor_has_any_team_package_access(db, package_id, actor_user_id)
        .await
        .map_err(|_| nuget_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error"))
}

async fn actor_has_any_team_repository_access(
    db: &PgPool,
    repository_id: Uuid,
    actor_user_id: Uuid,
) -> Result<bool, Response> {
    authz_queries::actor_has_any_team_repository_access(db, repository_id, actor_user_id)
        .await
        .map_err(|_| nuget_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error"))
}

#[allow(clippy::too_many_arguments)]
async fn can_read_package(
    db: &PgPool,
    package_id: Uuid,
    repository_id: Uuid,
    pkg_visibility: &str,
    repo_visibility: &str,
    pkg_owner_user_id: Option<Uuid>,
    pkg_owner_org_id: Option<Uuid>,
    repo_owner_user_id: Option<Uuid>,
    repo_owner_org_id: Option<Uuid>,
    actor_user_id: Option<Uuid>,
) -> Result<bool, Response> {
    let pkg_anonymous = matches!(pkg_visibility, "public" | "unlisted");
    let repo_anonymous = matches!(repo_visibility, "public" | "unlisted");

    if pkg_anonymous && repo_anonymous {
        return Ok(true);
    }

    let Some(actor) = actor_user_id else {
        return Ok(false);
    };

    let pkg_access = is_owner_or_member(db, pkg_owner_user_id, pkg_owner_org_id, actor).await?;
    let repo_access = is_owner_or_member(db, repo_owner_user_id, repo_owner_org_id, actor).await?;
    let team_package_access = if pkg_access {
        false
    } else {
        actor_has_any_team_package_access(db, package_id, actor).await?
    };
    let team_repository_access = if repo_access {
        false
    } else {
        actor_has_any_team_repository_access(db, repository_id, actor).await?
    };
    let delegated_read_access = team_package_access || team_repository_access;

    Ok((pkg_anonymous || pkg_access || delegated_read_access)
        && (repo_anonymous || repo_access || delegated_read_access))
}

async fn is_owner_or_member(
    db: &PgPool,
    owner_user_id: Option<Uuid>,
    owner_org_id: Option<Uuid>,
    actor_user_id: Uuid,
) -> Result<bool, Response> {
    if owner_user_id == Some(actor_user_id) {
        return Ok(true);
    }

    if let Some(org_id) = owner_org_id {
        return sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM org_memberships WHERE org_id = $1 AND user_id = $2)",
        )
        .bind(org_id)
        .bind(actor_user_id)
        .fetch_one(db)
        .await
        .map_err(|_| nuget_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error"));
    }

    Ok(false)
}

// ─── GET /v3/index.json — Service index ──────────────────────────────────────

async fn service_index<S: NuGetAppState>(State(state): State<S>) -> Response {
    let index = metadata::build_service_index(state.base_url());
    (
        StatusCode::OK,
        [(CONTENT_TYPE, "application/json")],
        Json(index),
    )
        .into_response()
}

// ─── PUT /v2/package — Push ──────────────────────────────────────────────────

async fn push_package<S: NuGetAppState>(
    State(state): State<S>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Response {
    // Auth
    let identity = match authenticate(&state, &headers).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    if !identity_has_scope(&identity, "packages:write") {
        return nuget_error_response(
            StatusCode::FORBIDDEN,
            "API key does not have the packages:write scope",
        );
    }

    // Extract .nupkg from multipart body
    let nupkg_bytes = match extract_nupkg_from_multipart(&mut multipart).await {
        Ok(bytes) => bytes,
        Err(resp) => return resp,
    };

    // Parse the .nupkg
    let parsed = match publish::parse_nupkg(nupkg_bytes) {
        Ok(p) => p,
        Err(e) => return nuget_error_response(StatusCode::BAD_REQUEST, &e.to_string()),
    };

    // Validate package ID
    if let Err(e) = validate_nuget_package_id(&parsed.metadata.id) {
        return nuget_error_response(StatusCode::BAD_REQUEST, &e.to_string());
    }

    let normalized = normalize_nuget_id(&parsed.metadata.id);
    let version = normalize_nuget_version(&parsed.metadata.version);

    // Check if package exists
    let existing_package = sqlx::query(
        "SELECT id, owner_user_id, owner_org_id \
         FROM packages \
         WHERE ecosystem = 'nuget' AND normalized_name = $1",
    )
    .bind(&normalized)
    .fetch_optional(state.db())
    .await;

    let package_id = match existing_package {
        Ok(Some(row)) => {
            let pkg_id: Uuid = match row.try_get("id") {
                Ok(id) => id,
                Err(_) => {
                    return nuget_error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Internal error",
                    )
                }
            };

            if !has_package_write_access(
                state.db(),
                pkg_id,
                row.try_get("owner_user_id").unwrap_or(None),
                row.try_get("owner_org_id").unwrap_or(None),
                identity.user_id,
            )
            .await
            {
                return nuget_error_response(
                    StatusCode::FORBIDDEN,
                    "You do not have permission to publish to this package",
                );
            }

            // Update package metadata from latest publish
            let _ = sqlx::query(
                "UPDATE packages \
                 SET description    = COALESCE($1, description), \
                     license        = COALESCE($2, license), \
                     homepage       = COALESCE($3, homepage), \
                     repository_url = COALESCE($4, repository_url), \
                     keywords       = COALESCE($5, keywords), \
                     updated_at     = NOW() \
                 WHERE id = $6",
            )
            .bind(&parsed.metadata.description)
            .bind(&parsed.metadata.license_expression)
            .bind(&parsed.metadata.project_url)
            .bind::<Option<&str>>(None) // repository_url not in nuspec
            .bind(if parsed.metadata.tags.is_empty() {
                None
            } else {
                Some(&parsed.metadata.tags)
            })
            .bind(pkg_id)
            .execute(state.db())
            .await;

            pkg_id
        }
        Ok(None) => {
            // Auto-create the package
            match auto_create_package(&state, &identity, &parsed).await {
                Ok(id) => id,
                Err(resp) => return resp,
            }
        }
        Err(_) => return nuget_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error"),
    };

    // Check if version already exists
    let existing = sqlx::query("SELECT id FROM releases WHERE package_id = $1 AND version = $2")
        .bind(package_id)
        .bind(&version)
        .fetch_optional(state.db())
        .await;

    if matches!(existing, Ok(Some(_))) {
        return nuget_error_response(
            StatusCode::CONFLICT,
            &format!(
                "Version {version} of package {} already exists",
                parsed.metadata.id
            ),
        );
    }

    // Create release in quarantine
    let is_prerelease = version.contains('-');
    let release = Release::new(package_id, version.clone(), identity.user_id);

    if sqlx::query(
        "INSERT INTO releases (id, package_id, version, status, published_by, description, \
         changelog, is_prerelease, is_yanked, yank_reason, is_deprecated, deprecation_message, \
         source_ref, provenance, published_at, updated_at) \
         VALUES ($1, $2, $3, 'quarantine', $4, $5, NULL, $6, false, NULL, false, NULL, NULL, NULL, $7, $8)",
    )
    .bind(release.id)
    .bind(package_id)
    .bind(&version)
    .bind(identity.user_id)
    .bind(&parsed.metadata.description)
    .bind(is_prerelease)
    .bind(release.published_at)
    .bind(release.updated_at)
    .execute(state.db())
    .await
    .is_err()
    {
        return nuget_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to create release",
        );
    }

    // Upload .nupkg to artifact storage
    let storage_key = format!(
        "releases/{}/artifacts/{}/{}",
        release.id,
        parsed.sha256,
        nupkg_filename(&parsed.metadata.id, &version)
    );

    if state
        .artifact_put(
            storage_key.clone(),
            "application/octet-stream".into(),
            parsed.nupkg_bytes,
        )
        .await
        .is_err()
    {
        let _ = sqlx::query("DELETE FROM releases WHERE id = $1")
            .bind(release.id)
            .execute(state.db())
            .await;
        return nuget_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Failed to store package");
    }

    // Upload .nuspec separately for efficient serving
    let nuspec_key = format!(
        "releases/{}/artifacts/{}/{}.nuspec",
        release.id, parsed.sha256, normalized
    );
    let _ = state
        .artifact_put(
            nuspec_key.clone(),
            "application/xml".into(),
            Bytes::from(parsed.nuspec_bytes.clone()),
        )
        .await;

    // Create artifact record for .nupkg
    let artifact = Artifact::new(
        release.id,
        ArtifactKind::Nupkg,
        nupkg_filename(&parsed.metadata.id, &version),
        storage_key.clone(),
        "application/octet-stream".into(),
        parsed.size_bytes,
        parsed.sha256.clone(),
    );

    if sqlx::query(
        "INSERT INTO artifacts (id, release_id, kind, filename, storage_key, content_type, \
         size_bytes, sha256, sha512, md5, is_signed, signature_key_id, uploaded_at) \
         VALUES ($1, $2, 'nupkg', $3, $4, $5, $6, $7, $8, NULL, false, NULL, $9) \
         ON CONFLICT (release_id, filename) DO NOTHING",
    )
    .bind(artifact.id)
    .bind(release.id)
    .bind(&artifact.filename)
    .bind(&artifact.storage_key)
    .bind(&artifact.content_type)
    .bind(parsed.size_bytes)
    .bind(&parsed.sha256)
    .bind(&parsed.sha512)
    .bind(artifact.uploaded_at)
    .execute(state.db())
    .await
    .is_err()
    {
        return nuget_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to record artifact",
        );
    }

    // Store NuGet-specific release metadata
    if sqlx::query(
        "INSERT INTO nuget_release_metadata \
         (release_id, authors, title, icon_url, license_url, license_expression, \
          project_url, require_license_acceptance, min_client_version, summary, \
          tags, dependency_groups, package_types, is_listed) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, true)",
    )
    .bind(release.id)
    .bind(&parsed.metadata.authors)
    .bind(&parsed.metadata.title)
    .bind(&parsed.metadata.icon_url)
    .bind(&parsed.metadata.license_url)
    .bind(&parsed.metadata.license_expression)
    .bind(&parsed.metadata.project_url)
    .bind(parsed.metadata.require_license_acceptance)
    .bind(&parsed.metadata.min_client_version)
    .bind(&parsed.metadata.summary)
    .bind(&parsed.metadata.tags)
    .bind(nuspec::dependency_groups_to_json(
        &parsed.metadata.dependency_groups,
    ))
    .bind(nuspec::package_types_to_json(
        &parsed.metadata.package_types,
    ))
    .execute(state.db())
    .await
    .is_err()
    {
        // Non-fatal: the release still works without NuGet-specific metadata
        tracing::warn!(
            release_id = %release.id,
            "Failed to store NuGet release metadata"
        );
    }

    // Finalize: move release to published
    if sqlx::query("UPDATE releases SET status = 'published', updated_at = NOW() WHERE id = $1")
        .bind(release.id)
        .execute(state.db())
        .await
        .is_err()
    {
        return nuget_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to publish release",
        );
    }

    // Audit log
    let _ = sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, \
         target_package_id, target_release_id, metadata, occurred_at) \
         VALUES ($1, 'release_publish', $2, $3, $4, $5, $6, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.token_id)
    .bind(package_id)
    .bind(release.id)
    .bind(serde_json::json!({
        "ecosystem": "nuget",
        "name": parsed.metadata.id,
        "version": version,
        "source": "nuget_push",
        "artifact_sha256": parsed.sha256,
    }))
    .execute(state.db())
    .await;

    // Trigger search reindex (best-effort)
    let _ = state.reindex_package_document(package_id).await;

    (StatusCode::CREATED, "").into_response()
}

/// Extract the `.nupkg` file bytes from a multipart form data request.
async fn extract_nupkg_from_multipart(multipart: &mut Multipart) -> Result<Bytes, Response> {
    while let Ok(Some(field)) = multipart.next_field().await {
        // The first file field is the .nupkg
        if let Ok(data) = field.bytes().await {
            if !data.is_empty() {
                return Ok(data);
            }
        }
    }

    Err(nuget_error_response(
        StatusCode::BAD_REQUEST,
        "No .nupkg file found in multipart request body",
    ))
}

/// Auto-create a package for a first-time NuGet push.
async fn auto_create_package<S: NuGetAppState>(
    state: &S,
    identity: &NuGetIdentity,
    parsed: &ParsedNuGetPublish,
) -> Result<Uuid, Response> {
    let repo_row = sqlx::query(
        "SELECT id, visibility, owner_user_id, owner_org_id \
         FROM repositories \
         WHERE owner_user_id = $1 \
           AND kind IN ('public', 'private', 'staging', 'release') \
         ORDER BY created_at ASC \
         LIMIT 1",
    )
    .bind(identity.user_id)
    .fetch_optional(state.db())
    .await
    .map_err(|_| nuget_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error"))?
    .ok_or_else(|| {
        nuget_error_response(
            StatusCode::FORBIDDEN,
            "You have no repository to publish into. Create a repository first via the Publaryn API.",
        )
    })?;

    let repo_id: Uuid = repo_row.try_get("id").unwrap();
    let repo_visibility: String = repo_row.try_get("visibility").unwrap_or("public".into());

    let visibility = match repo_visibility.as_str() {
        "private" => Visibility::Private,
        "unlisted" => Visibility::Unlisted,
        _ => Visibility::Public,
    };

    let pkg_id = Uuid::new_v4();
    let now = Utc::now();

    let normalized = normalize_nuget_id(&parsed.metadata.id);
    let vis_str = match visibility {
        Visibility::Private => "private",
        Visibility::Unlisted => "unlisted",
        _ => "public",
    };

    let insert_result = sqlx::query(
        "INSERT INTO packages (id, repository_id, ecosystem, name, normalized_name, \
         display_name, description, readme, homepage, repository_url, license, keywords, \
         visibility, owner_user_id, owner_org_id, is_deprecated, deprecation_message, \
         is_archived, download_count, created_at, updated_at) \
         VALUES ($1, $2, 'nuget', $3, $4, NULL, $5, NULL, $6, NULL, $7, $8, $9, $10, NULL, \
         false, NULL, false, 0, $11, $12)",
    )
    .bind(pkg_id)
    .bind(repo_id)
    .bind(&parsed.metadata.id)
    .bind(&normalized)
    .bind(&parsed.metadata.description)
    .bind(&parsed.metadata.project_url)
    .bind(&parsed.metadata.license_expression)
    .bind(&parsed.metadata.tags)
    .bind(vis_str)
    .bind(identity.user_id)
    .bind(now)
    .bind(now)
    .execute(state.db())
    .await;

    match insert_result {
        Ok(_) => {
            // Audit log
            let _ = sqlx::query(
                "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, \
                 target_package_id, metadata, occurred_at) \
                 VALUES ($1, 'package_create', $2, $3, $4, $5, NOW())",
            )
            .bind(Uuid::new_v4())
            .bind(identity.user_id)
            .bind(identity.token_id)
            .bind(pkg_id)
            .bind(serde_json::json!({
                "ecosystem": "nuget",
                "name": parsed.metadata.id,
                "source": "nuget_push",
            }))
            .execute(state.db())
            .await;

            Ok(pkg_id)
        }
        Err(e) => {
            if let sqlx::Error::Database(ref db_err) = e {
                if db_err.is_unique_violation() {
                    // Race condition: fetch the one that won
                    let row = sqlx::query(
                        "SELECT id FROM packages \
                         WHERE ecosystem = 'nuget' AND normalized_name = $1",
                    )
                    .bind(&normalized)
                    .fetch_optional(state.db())
                    .await
                    .map_err(|_| {
                        nuget_error_response(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "Failed to create package",
                        )
                    })?
                    .ok_or_else(|| {
                        nuget_error_response(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "Failed to create package",
                        )
                    })?;
                    return Ok(row.try_get("id").unwrap());
                }
            }
            Err(nuget_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Database error",
            ))
        }
    }
}

// ─── DELETE /v2/package/:id/:version — Unlist ────────────────────────────────

async fn unlist_package<S: NuGetAppState>(
    State(state): State<S>,
    Path((id, version)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let identity = match authenticate(&state, &headers).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    if !identity_has_scope(&identity, "packages:write") {
        return nuget_error_response(
            StatusCode::FORBIDDEN,
            "API key does not have the packages:write scope",
        );
    }

    let normalized = normalize_nuget_id(&id);
    let norm_version = normalize_nuget_version(&version);

    let pkg_row = sqlx::query(
        "SELECT id, owner_user_id, owner_org_id \
         FROM packages \
         WHERE ecosystem = 'nuget' AND normalized_name = $1",
    )
    .bind(&normalized)
    .fetch_optional(state.db())
    .await;

    let pkg_row = match pkg_row {
        Ok(Some(r)) => r,
        Ok(None) => return (StatusCode::NOT_FOUND, "").into_response(),
        Err(_) => return nuget_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error"),
    };

    let package_id: Uuid = pkg_row.try_get("id").unwrap();

    if !has_package_write_access(
        state.db(),
        package_id,
        pkg_row.try_get("owner_user_id").unwrap_or(None),
        pkg_row.try_get("owner_org_id").unwrap_or(None),
        identity.user_id,
    )
    .await
    {
        return nuget_error_response(
            StatusCode::FORBIDDEN,
            "You do not have permission to modify this package",
        );
    }

    // Set release to yanked + update nuget metadata is_listed
    let release_row = sqlx::query(
        "SELECT id FROM releases \
         WHERE package_id = $1 AND version = $2 \
           AND status IN ('published', 'deprecated')",
    )
    .bind(package_id)
    .bind(&norm_version)
    .fetch_optional(state.db())
    .await;

    let release_id: Uuid = match release_row {
        Ok(Some(r)) => r.try_get("id").unwrap(),
        Ok(None) => return (StatusCode::NOT_FOUND, "").into_response(),
        Err(_) => return nuget_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error"),
    };

    let _ = sqlx::query(
        "UPDATE releases SET is_yanked = true, status = 'yanked', updated_at = NOW() \
         WHERE id = $1",
    )
    .bind(release_id)
    .execute(state.db())
    .await;

    let _ =
        sqlx::query("UPDATE nuget_release_metadata SET is_listed = false WHERE release_id = $1")
            .bind(release_id)
            .execute(state.db())
            .await;

    // Audit
    let _ = sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, \
         target_package_id, target_release_id, metadata, occurred_at) \
         VALUES ($1, 'release_yank', $2, $3, $4, $5, $6, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.token_id)
    .bind(package_id)
    .bind(release_id)
    .bind(serde_json::json!({
        "ecosystem": "nuget",
        "version": norm_version,
        "action": "unlist",
    }))
    .execute(state.db())
    .await;

    (StatusCode::NO_CONTENT, "").into_response()
}

// ─── POST /v2/package/:id/:version — Relist ─────────────────────────────────

async fn relist_package<S: NuGetAppState>(
    State(state): State<S>,
    Path((id, version)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let identity = match authenticate(&state, &headers).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    if !identity_has_scope(&identity, "packages:write") {
        return nuget_error_response(
            StatusCode::FORBIDDEN,
            "API key does not have the packages:write scope",
        );
    }

    let normalized = normalize_nuget_id(&id);
    let norm_version = normalize_nuget_version(&version);

    let pkg_row = sqlx::query(
        "SELECT id, owner_user_id, owner_org_id \
         FROM packages \
         WHERE ecosystem = 'nuget' AND normalized_name = $1",
    )
    .bind(&normalized)
    .fetch_optional(state.db())
    .await;

    let pkg_row = match pkg_row {
        Ok(Some(r)) => r,
        Ok(None) => return (StatusCode::NOT_FOUND, "").into_response(),
        Err(_) => return nuget_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error"),
    };

    let package_id: Uuid = pkg_row.try_get("id").unwrap();

    if !has_package_write_access(
        state.db(),
        package_id,
        pkg_row.try_get("owner_user_id").unwrap_or(None),
        pkg_row.try_get("owner_org_id").unwrap_or(None),
        identity.user_id,
    )
    .await
    {
        return nuget_error_response(
            StatusCode::FORBIDDEN,
            "You do not have permission to modify this package",
        );
    }

    let release_row = sqlx::query(
        "SELECT id FROM releases \
         WHERE package_id = $1 AND version = $2 \
           AND status IN ('published', 'deprecated', 'yanked')",
    )
    .bind(package_id)
    .bind(&norm_version)
    .fetch_optional(state.db())
    .await;

    let release_id: Uuid = match release_row {
        Ok(Some(r)) => r.try_get("id").unwrap(),
        Ok(None) => return (StatusCode::NOT_FOUND, "").into_response(),
        Err(_) => return nuget_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error"),
    };

    let _ = sqlx::query(
        "UPDATE releases SET is_yanked = false, status = 'published', updated_at = NOW() \
         WHERE id = $1",
    )
    .bind(release_id)
    .execute(state.db())
    .await;

    let _ = sqlx::query("UPDATE nuget_release_metadata SET is_listed = true WHERE release_id = $1")
        .bind(release_id)
        .execute(state.db())
        .await;

    // Audit
    let _ = sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, \
         target_package_id, target_release_id, metadata, occurred_at) \
         VALUES ($1, 'release_unyank', $2, $3, $4, $5, $6, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.token_id)
    .bind(package_id)
    .bind(release_id)
    .bind(serde_json::json!({
        "ecosystem": "nuget",
        "version": norm_version,
        "action": "relist",
    }))
    .execute(state.db())
    .await;

    (StatusCode::OK, "").into_response()
}

// ─── GET /v3-flatcontainer/:id/index.json — Version listing ──────────────────

async fn get_versions<S: NuGetAppState>(
    State(state): State<S>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let normalized = normalize_nuget_id(&id);
    let actor = authenticate(&state, &headers).await.ok().map(|i| i.user_id);

    let pkg_row = match sqlx::query(
        "SELECT p.id, p.repository_id, p.visibility, p.owner_user_id, p.owner_org_id, \
                r.visibility AS repo_visibility, \
                r.owner_user_id AS repo_owner_user_id, \
                r.owner_org_id AS repo_owner_org_id \
         FROM packages p \
         JOIN repositories r ON r.id = p.repository_id \
         WHERE p.ecosystem = 'nuget' AND p.normalized_name = $1",
    )
    .bind(&normalized)
    .fetch_optional(state.db())
    .await
    {
        Ok(Some(row)) => row,
        Ok(None) => return (StatusCode::NOT_FOUND, "").into_response(),
        Err(_) => return nuget_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error"),
    };
    let package_id: Uuid = match pkg_row.try_get("id") {
        Ok(id) => id,
        Err(_) => return nuget_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error"),
    };
    let repository_id: Uuid = match pkg_row.try_get("repository_id") {
        Ok(id) => id,
        Err(_) => return nuget_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error"),
    };

    if !match can_read_package(
        state.db(),
        package_id,
        repository_id,
        &pkg_row
            .try_get::<String, _>("visibility")
            .unwrap_or_default(),
        &pkg_row
            .try_get::<String, _>("repo_visibility")
            .unwrap_or_default(),
        pkg_row.try_get("owner_user_id").unwrap_or(None),
        pkg_row.try_get("owner_org_id").unwrap_or(None),
        pkg_row.try_get("repo_owner_user_id").unwrap_or(None),
        pkg_row.try_get("repo_owner_org_id").unwrap_or(None),
        actor,
    )
    .await
    {
        Ok(can_read) => can_read,
        Err(response) => return response,
    } {
        return (StatusCode::NOT_FOUND, "").into_response();
    }

    // NuGet flat container includes both listed and unlisted versions
    let rows = sqlx::query(
        "SELECT version FROM releases \
         WHERE package_id = $1 \
           AND status IN ('published', 'deprecated', 'yanked') \
         ORDER BY published_at ASC",
    )
    .bind(package_id)
    .fetch_all(state.db())
    .await
    .unwrap_or_default();

    let versions: Vec<String> = rows
        .iter()
        .filter_map(|r| r.try_get::<String, _>("version").ok())
        .collect();

    let listing = metadata::build_version_listing(&versions);

    (
        StatusCode::OK,
        [(CONTENT_TYPE, "application/json")],
        Json(listing),
    )
        .into_response()
}

// ─── GET /v3-flatcontainer/:id/:version/:filename — Download ─────────────────

async fn download_content<S: NuGetAppState>(
    State(state): State<S>,
    Path((id, version, filename)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Response {
    let normalized = normalize_nuget_id(&id);
    let norm_version = normalize_nuget_version(&version);
    let actor = authenticate(&state, &headers).await.ok().map(|i| i.user_id);

    let pkg_row = match sqlx::query(
        "SELECT p.id, p.repository_id, p.visibility, p.owner_user_id, p.owner_org_id, \
                r.visibility AS repo_visibility, \
                r.owner_user_id AS repo_owner_user_id, \
                r.owner_org_id AS repo_owner_org_id \
         FROM packages p \
         JOIN repositories r ON r.id = p.repository_id \
         WHERE p.ecosystem = 'nuget' AND p.normalized_name = $1",
    )
    .bind(&normalized)
    .fetch_optional(state.db())
    .await
    {
        Ok(Some(row)) => row,
        Ok(None) => return (StatusCode::NOT_FOUND, "").into_response(),
        Err(_) => return nuget_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error"),
    };
    let package_id: Uuid = match pkg_row.try_get("id") {
        Ok(id) => id,
        Err(_) => return nuget_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error"),
    };
    let repository_id: Uuid = match pkg_row.try_get("repository_id") {
        Ok(id) => id,
        Err(_) => return nuget_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error"),
    };

    if !match can_read_package(
        state.db(),
        package_id,
        repository_id,
        &pkg_row
            .try_get::<String, _>("visibility")
            .unwrap_or_default(),
        &pkg_row
            .try_get::<String, _>("repo_visibility")
            .unwrap_or_default(),
        pkg_row.try_get("owner_user_id").unwrap_or(None),
        pkg_row.try_get("owner_org_id").unwrap_or(None),
        pkg_row.try_get("repo_owner_user_id").unwrap_or(None),
        pkg_row.try_get("repo_owner_org_id").unwrap_or(None),
        actor,
    )
    .await
    {
        Ok(can_read) => can_read,
        Err(response) => return response,
    } {
        return (StatusCode::NOT_FOUND, "").into_response();
    }

    // Determine if this is a .nupkg or .nuspec request
    let is_nuspec = filename.ends_with(".nuspec");

    if is_nuspec {
        // Serve .nuspec — look for stored nuspec or extract from nupkg
        let release_row = sqlx::query(
            "SELECT a.storage_key \
             FROM artifacts a \
             JOIN releases r ON r.id = a.release_id \
             WHERE r.package_id = $1 AND r.version = $2 \
               AND a.kind = 'nupkg' \
               AND r.status IN ('published', 'deprecated', 'yanked') \
             LIMIT 1",
        )
        .bind(package_id)
        .bind(&norm_version)
        .fetch_optional(state.db())
        .await;

        let storage_key = match release_row {
            Ok(Some(r)) => r.try_get::<String, _>("storage_key").unwrap_or_default(),
            Ok(None) => return (StatusCode::NOT_FOUND, "").into_response(),
            Err(_) => {
                return nuget_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error")
            }
        };

        // Try to serve separately stored nuspec first
        let nuspec_key = storage_key.replace(".nupkg", ".nuspec");
        // Derive nuspec key from the nupkg storage key pattern
        let parts: Vec<&str> = storage_key.rsplitn(2, '/').collect();
        let nuspec_storage_key = if parts.len() == 2 {
            format!("{}/{}.nuspec", parts[1], normalized)
        } else {
            nuspec_key
        };

        if let Ok(Some(obj)) = state.artifact_get(&nuspec_storage_key).await {
            return (
                StatusCode::OK,
                [(CONTENT_TYPE, "application/xml")],
                obj.bytes,
            )
                .into_response();
        }

        // Fallback: extract from .nupkg
        let nupkg = match state.artifact_get(&storage_key).await {
            Ok(Some(obj)) => obj,
            Ok(None) => return (StatusCode::NOT_FOUND, "").into_response(),
            Err(_) => {
                return nuget_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Storage error")
            }
        };

        match nuspec::parse_nuspec_from_nupkg(&nupkg.bytes) {
            Ok((_meta, nuspec_bytes)) => (
                StatusCode::OK,
                [(CONTENT_TYPE, "application/xml")],
                Bytes::from(nuspec_bytes),
            )
                .into_response(),
            Err(_) => nuget_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to extract .nuspec from package",
            ),
        }
    } else {
        // Serve .nupkg
        let artifact_row = sqlx::query(
            "SELECT a.storage_key, a.content_type, a.size_bytes, a.sha256 \
             FROM artifacts a \
             JOIN releases r ON r.id = a.release_id \
             WHERE r.package_id = $1 \
               AND r.version = $2 \
               AND a.kind = 'nupkg' \
               AND r.status IN ('published', 'deprecated', 'yanked') \
             LIMIT 1",
        )
        .bind(package_id)
        .bind(&norm_version)
        .fetch_optional(state.db())
        .await;

        let artifact_row = match artifact_row {
            Ok(Some(r)) => r,
            Ok(None) => return (StatusCode::NOT_FOUND, "").into_response(),
            Err(_) => {
                return nuget_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error")
            }
        };

        let storage_key: String = artifact_row.try_get("storage_key").unwrap_or_default();

        let stored = match state.artifact_get(&storage_key).await {
            Ok(Some(obj)) => obj,
            Ok(None) => return (StatusCode::NOT_FOUND, "").into_response(),
            Err(_) => {
                return nuget_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Storage error")
            }
        };

        // Increment download count (fire-and-forget)
        let _ =
            sqlx::query("UPDATE packages SET download_count = download_count + 1 WHERE id = $1")
                .bind(package_id)
                .execute(state.db())
                .await;

        let size = stored.bytes.len();
        (
            StatusCode::OK,
            [
                (CONTENT_TYPE, "application/octet-stream"),
                (CONTENT_LENGTH, &size.to_string()),
            ],
            stored.bytes,
        )
            .into_response()
    }
}

// ─── GET /v3/registration/:id/index.json — Registration metadata ─────────────

async fn get_registration_index<S: NuGetAppState>(
    State(state): State<S>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let normalized = normalize_nuget_id(&id);
    let actor = authenticate(&state, &headers).await.ok().map(|i| i.user_id);

    let pkg_row = match sqlx::query(
        "SELECT p.id, p.repository_id, p.name, p.visibility, p.owner_user_id, p.owner_org_id, \
                r.visibility AS repo_visibility, \
                r.owner_user_id AS repo_owner_user_id, \
                r.owner_org_id AS repo_owner_org_id \
         FROM packages p \
         JOIN repositories r ON r.id = p.repository_id \
         WHERE p.ecosystem = 'nuget' AND p.normalized_name = $1",
    )
    .bind(&normalized)
    .fetch_optional(state.db())
    .await
    {
        Ok(Some(row)) => row,
        Ok(None) => return (StatusCode::NOT_FOUND, "").into_response(),
        Err(_) => return nuget_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error"),
    };
    let package_id: Uuid = match pkg_row.try_get("id") {
        Ok(id) => id,
        Err(_) => return nuget_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error"),
    };
    let repository_id: Uuid = match pkg_row.try_get("repository_id") {
        Ok(id) => id,
        Err(_) => return nuget_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error"),
    };

    if !match can_read_package(
        state.db(),
        package_id,
        repository_id,
        &pkg_row
            .try_get::<String, _>("visibility")
            .unwrap_or_default(),
        &pkg_row
            .try_get::<String, _>("repo_visibility")
            .unwrap_or_default(),
        pkg_row.try_get("owner_user_id").unwrap_or(None),
        pkg_row.try_get("owner_org_id").unwrap_or(None),
        pkg_row.try_get("repo_owner_user_id").unwrap_or(None),
        pkg_row.try_get("repo_owner_org_id").unwrap_or(None),
        actor,
    )
    .await
    {
        Ok(can_read) => can_read,
        Err(response) => return response,
    } {
        return (StatusCode::NOT_FOUND, "").into_response();
    }

    let package_name: String = pkg_row.try_get("name").unwrap_or_default();

    // Load releases + NuGet metadata
    let release_rows = sqlx::query(
        "SELECT rel.id, rel.version, rel.is_deprecated, rel.deprecation_message, \
                rel.published_at, \
                nm.authors, nm.title, nm.icon_url, nm.license_url, nm.license_expression, \
                nm.project_url, nm.require_license_acceptance, nm.summary, nm.tags, \
                nm.dependency_groups, nm.package_types, nm.is_listed \
         FROM releases rel \
         LEFT JOIN nuget_release_metadata nm ON nm.release_id = rel.id \
         WHERE rel.package_id = $1 \
           AND rel.status IN ('published', 'deprecated', 'yanked') \
         ORDER BY rel.published_at ASC",
    )
    .bind(package_id)
    .fetch_all(state.db())
    .await
    .unwrap_or_default();

    let versions: Vec<RegistrationVersionInput> = release_rows
        .iter()
        .map(|row| {
            let description_val: Option<String> = row.try_get("summary").unwrap_or(None);
            RegistrationVersionInput {
                version: row.try_get("version").unwrap_or_default(),
                description: description_val,
                authors: row.try_get("authors").unwrap_or(None),
                tags: row.try_get("tags").unwrap_or_default(),
                license_url: row.try_get("license_url").unwrap_or(None),
                license_expression: row.try_get("license_expression").unwrap_or(None),
                project_url: row.try_get("project_url").unwrap_or(None),
                icon_url: row.try_get("icon_url").unwrap_or(None),
                require_license_acceptance: row
                    .try_get("require_license_acceptance")
                    .unwrap_or(false),
                summary: row.try_get("summary").unwrap_or(None),
                title: row.try_get("title").unwrap_or(None),
                dependency_groups: row
                    .try_get::<serde_json::Value, _>("dependency_groups")
                    .unwrap_or(serde_json::json!([])),
                is_listed: row.try_get("is_listed").unwrap_or(true),
                is_deprecated: row.try_get("is_deprecated").unwrap_or(false),
                deprecation_message: row.try_get("deprecation_message").unwrap_or(None),
                published_at: row.try_get("published_at").unwrap_or_else(|_| Utc::now()),
                package_types: row
                    .try_get::<serde_json::Value, _>("package_types")
                    .unwrap_or(serde_json::json!([{"name": "Dependency"}])),
            }
        })
        .collect();

    let input = RegistrationInput {
        package_id: package_name,
        versions,
    };

    let index = metadata::build_registration_index(&input, state.base_url());

    (
        StatusCode::OK,
        [(CONTENT_TYPE, "application/json")],
        Json(index),
    )
        .into_response()
}

// ─── GET /v3/search — Search ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct SearchParams {
    q: Option<String>,
    skip: Option<u32>,
    take: Option<u32>,
    #[allow(dead_code)]
    prerelease: Option<bool>,
    #[serde(rename = "semVerLevel")]
    #[allow(dead_code)]
    sem_ver_level: Option<String>,
    #[serde(rename = "packageType")]
    #[allow(dead_code)]
    package_type: Option<String>,
}

async fn search<S: NuGetAppState>(
    State(state): State<S>,
    Query(params): Query<SearchParams>,
    headers: HeaderMap,
) -> Response {
    let query = params.q.unwrap_or_default();
    let skip = params.skip.unwrap_or(0);
    let take = params.take.unwrap_or(20).min(1000);
    let actor_user_id = if headers.contains_key(&X_NUGET_APIKEY) {
        match authenticate(&state, &headers).await {
            Ok(identity) => Some(identity.user_id),
            Err(_) => return nuget_error_response(StatusCode::UNAUTHORIZED, "Unauthorized"),
        }
    } else if headers.contains_key(AUTHORIZATION) {
        authenticate(&state, &headers)
            .await
            .ok()
            .map(|identity| identity.user_id)
    } else {
        None
    };

    let search_results = match state
        .search_packages(&query, take, skip, actor_user_id)
        .await
    {
        Ok(h) => h,
        Err(_) => return nuget_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Search error"),
    };

    let total_hits = search_results.total as i64;

    let results: Vec<SearchResultInput> = search_results
        .hits
        .into_iter()
        .map(|hit| SearchResultInput {
            package_id: hit.id.clone(),
            latest_version: hit.version.clone(),
            description: hit.description,
            authors: None,
            tags: hit.tags,
            total_downloads: hit.total_downloads,
            verified: false,
            versions: vec![SearchVersionInput {
                version: hit.version,
                downloads: hit.total_downloads,
            }],
            package_types: serde_json::json!([{"name": "Dependency"}]),
        })
        .collect();

    let response = metadata::build_search_response(&results, total_hits, state.base_url());

    (
        StatusCode::OK,
        [(CONTENT_TYPE, "application/json")],
        Json(response),
    )
        .into_response()
}
