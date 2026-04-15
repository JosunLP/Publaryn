//! Axum route handlers for the npm registry protocol.
//!
//! These handlers are designed to be mounted under a configurable prefix
//! (e.g. `/npm`) in the main API router. They translate npm wire-format
//! requests into shared domain model operations.

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{
        header::{AUTHORIZATION, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE},
        HeaderMap, StatusCode,
    },
    response::{IntoResponse, Response},
    routing::{get, put},
    Json, Router,
};
use bytes::Bytes;
use chrono::Utc;
use serde::Deserialize;
use sha2::{Digest, Sha256, Sha512};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use publaryn_core::{
    domain::{
        artifact::{Artifact, ArtifactKind},
        namespace::Ecosystem,
        package::{normalize_package_name, Package},
        release::Release,
    },
    error::Error,
};

use crate::{
    metadata::{self, PackumentInput, VersionRecord},
    name::validate_npm_package_name,
    publish::{self, extract_version_fields, NpmPublishPayload},
};

// ─── Shared state trait ──────────────────────────────────────────────────────
// The npm adapter needs access to the DB pool, artifact storage, search index,
// and configuration. Rather than depending on the API crate directly (which
// would create a circular dependency), we define a trait that the API crate's
// AppState can implement.

/// Trait abstracting the application state needed by npm adapter routes.
///
/// The API crate's `AppState` implements this via a blanket impl, keeping the
/// adapter crate free from circular dependencies.
pub trait NpmAppState: Clone + Send + Sync + 'static {
    fn db(&self) -> &PgPool;
    fn artifact_put(&self, key: String, content_type: String, bytes: Bytes)
        -> impl std::future::Future<Output = Result<(), Error>> + Send;
    fn artifact_get(
        &self,
        key: &str,
    ) -> impl std::future::Future<Output = Result<Option<StoredObject>, Error>> + Send;
    fn base_url(&self) -> &str;
    fn jwt_secret(&self) -> &str;
    fn jwt_issuer(&self) -> &str;
    fn search_packages(
        &self,
        query: &str,
        limit: u32,
        offset: u32,
    ) -> impl std::future::Future<Output = Result<Vec<NpmSearchHit>, Error>> + Send;
}

/// A retrieved object from artifact storage.
#[derive(Debug, Clone)]
pub struct StoredObject {
    pub content_type: String,
    pub bytes: Bytes,
}

/// A search result projected for npm search response format.
#[derive(Debug, Clone)]
pub struct NpmSearchHit {
    pub name: String,
    pub description: Option<String>,
    pub keywords: Vec<String>,
    pub version: Option<String>,
    pub date: Option<String>,
}

/// Identity extracted from a Bearer token.
#[derive(Debug, Clone)]
pub struct NpmIdentity {
    pub user_id: Uuid,
    pub token_id: Option<Uuid>,
    pub scopes: Vec<String>,
}

// ─── Router ──────────────────────────────────────────────────────────────────

/// Build the npm registry router.
///
/// Mount this under `/npm` (or any prefix) in the main API router.
pub fn router<S: NpmAppState>() -> Router<S> {
    Router::new()
        // Search
        .route("/-/v1/search", get(search_handler::<S>))
        // Dist-tags
        .route(
            "/-/package/:package/dist-tags",
            get(list_dist_tags::<S>),
        )
        .route(
            "/-/package/:package/dist-tags/:tag",
            put(set_dist_tag::<S>).delete(delete_dist_tag::<S>),
        )
        // Scoped package: packument, publish, tarball
        .route("/:scope/:name", get(get_packument::<S>).put(publish_handler::<S>))
        .route("/:scope/:name/-/:filename", get(download_tarball::<S>))
        // Unscoped package: packument, publish, tarball
        .route("/:package", get(get_packument_unscoped::<S>).put(publish_handler_unscoped::<S>))
        .route("/:package/-/:filename", get(download_tarball_unscoped::<S>))
}

// ─── Auth helpers ────────────────────────────────────────────────────────────

/// Parse a Bearer token from the Authorization header.
fn extract_bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(AUTHORIZATION)?
        .to_str()
        .ok()
        .and_then(|val| {
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

/// Authenticate a bearer token. Returns the user identity or an npm-formatted
/// error response.
async fn authenticate<S: NpmAppState>(
    state: &S,
    headers: &HeaderMap,
) -> Result<NpmIdentity, Response> {
    let token = extract_bearer_token(headers).ok_or_else(|| {
        npm_error_response(
            StatusCode::UNAUTHORIZED,
            "Authentication required. Use `npm login` to authenticate.",
        )
    })?;

    // Try API token (prefixed with "pub_")
    if token.starts_with("pub_") {
        let token_hash = publaryn_core::security::hash_token(token);
        let row = sqlx::query(
            "SELECT id, user_id, scopes, expires_at \
             FROM tokens \
             WHERE token_hash = $1 AND is_revoked = false",
        )
        .bind(&token_hash)
        .fetch_optional(state.db())
        .await
        .map_err(|_| npm_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Internal error"))?
        .ok_or_else(|| {
            npm_error_response(StatusCode::UNAUTHORIZED, "Invalid or revoked token")
        })?;

        let expires_at = row
            .try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("expires_at")
            .unwrap_or(None);
        if expires_at.is_some_and(|exp| exp <= Utc::now()) {
            return Err(npm_error_response(
                StatusCode::UNAUTHORIZED,
                "Token has expired",
            ));
        }

        let token_id: Uuid = row.try_get("id").map_err(|_| {
            npm_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Internal error")
        })?;
        let user_id: Option<Uuid> = row.try_get("user_id").unwrap_or(None);
        let user_id = user_id.ok_or_else(|| {
            npm_error_response(
                StatusCode::UNAUTHORIZED,
                "Token is not associated with a user",
            )
        })?;
        let scopes: Vec<String> = row.try_get("scopes").unwrap_or_default();

        // Update last_used_at (fire-and-forget)
        let _ = sqlx::query("UPDATE tokens SET last_used_at = NOW() WHERE id = $1")
            .bind(token_id)
            .execute(state.db())
            .await;

        return Ok(NpmIdentity {
            user_id,
            token_id: Some(token_id),
            scopes,
        });
    }

    // Try JWT
    let claims = publaryn_auth::validate_token(token, state.jwt_secret(), state.jwt_issuer())
        .map_err(|_| npm_error_response(StatusCode::UNAUTHORIZED, "Invalid or expired token"))?;

    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| {
        npm_error_response(StatusCode::UNAUTHORIZED, "Invalid token subject")
    })?;
    let token_id = Uuid::parse_str(&claims.jti).ok();

    Ok(NpmIdentity {
        user_id,
        token_id,
        scopes: claims.scopes,
    })
}

fn identity_has_scope(identity: &NpmIdentity, scope: &str) -> bool {
    identity.scopes.iter().any(|s| s == scope)
}

// ─── npm error format ────────────────────────────────────────────────────────

fn npm_error_response(status: StatusCode, message: &str) -> Response {
    let body = serde_json::json!({ "error": message });
    (status, Json(body)).into_response()
}

// ─── Path helpers ────────────────────────────────────────────────────────────

/// Reassemble a scoped package name from path segments.
/// The npm CLI URL-encodes `@scope/name` → `@scope%2fname` but Axum may also
/// split on the `/` before we see it, so we handle both patterns.
fn scoped_name(scope: &str, name: &str) -> String {
    let scope = if scope.starts_with('@') {
        scope.to_owned()
    } else {
        format!("@{scope}")
    };
    format!("{scope}/{name}")
}

// ─── GET /:package — Packument ───────────────────────────────────────────────

async fn get_packument<S: NpmAppState>(
    State(state): State<S>,
    Path((scope, name)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let package_name = scoped_name(&scope, &name);
    get_packument_inner(&state, &package_name, &headers).await
}

async fn get_packument_unscoped<S: NpmAppState>(
    State(state): State<S>,
    Path(package): Path<String>,
    headers: HeaderMap,
) -> Response {
    get_packument_inner(&state, &package, &headers).await
}

async fn get_packument_inner<S: NpmAppState>(
    state: &S,
    package_name: &str,
    headers: &HeaderMap,
) -> Response {
    let normalized = normalize_package_name(package_name, &Ecosystem::Npm);
    let actor_user_id = authenticate(state, headers).await.ok().map(|id| id.user_id);

    // Load package
    let package_row = match sqlx::query(
        "SELECT p.id, p.name, p.description, p.license, p.homepage, p.repository_url, \
                p.keywords, p.readme, p.is_deprecated, p.deprecation_message, \
                p.visibility, p.owner_user_id, p.owner_org_id, \
                p.created_at, p.updated_at, \
                r.visibility AS repo_visibility, \
                r.owner_user_id AS repo_owner_user_id, \
                r.owner_org_id AS repo_owner_org_id \
         FROM packages p \
         JOIN repositories r ON r.id = p.repository_id \
         WHERE p.ecosystem = 'npm' AND p.normalized_name = $1",
    )
    .bind(&normalized)
    .fetch_optional(state.db())
    .await
    {
        Ok(Some(row)) => row,
        Ok(None) => return npm_error_response(StatusCode::NOT_FOUND, "Package not found"),
        Err(_) => return npm_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error"),
    };

    // Visibility check
    let pkg_visibility: String = package_row.try_get("visibility").unwrap_or_default();
    let repo_visibility: String = package_row.try_get("repo_visibility").unwrap_or_default();
    if !can_read_package(
        state.db(),
        &pkg_visibility,
        &repo_visibility,
        package_row.try_get("owner_user_id").unwrap_or(None),
        package_row.try_get("owner_org_id").unwrap_or(None),
        package_row.try_get("repo_owner_user_id").unwrap_or(None),
        package_row.try_get("repo_owner_org_id").unwrap_or(None),
        actor_user_id,
    )
    .await
    {
        return npm_error_response(StatusCode::NOT_FOUND, "Package not found");
    }

    let package_id: Uuid = match package_row.try_get("id") {
        Ok(id) => id,
        Err(_) => return npm_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Internal error"),
    };
    let db_name: String = package_row.try_get("name").unwrap_or_default();

    // Load published releases (exclude quarantine/scanning/deleted; include yanked for metadata)
    let release_rows = match sqlx::query(
        "SELECT r.id, r.version, r.description, r.is_deprecated, r.deprecation_message, \
                r.is_yanked, r.published_at, r.provenance \
         FROM releases r \
         WHERE r.package_id = $1 \
           AND r.status IN ('published', 'deprecated', 'yanked') \
         ORDER BY r.published_at ASC",
    )
    .bind(package_id)
    .fetch_all(state.db())
    .await
    {
        Ok(rows) => rows,
        Err(_) => return npm_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error"),
    };

    // For each release, find the tarball artifact sha256/sha512
    let mut versions = Vec::with_capacity(release_rows.len());
    for rr in &release_rows {
        let release_id: Uuid = match rr.try_get("id") {
            Ok(id) => id,
            Err(_) => continue,
        };
        let version: String = rr.try_get("version").unwrap_or_default();

        let artifact_row = sqlx::query(
            "SELECT sha256, sha512, size_bytes \
             FROM artifacts \
             WHERE release_id = $1 AND kind = 'tarball' \
             LIMIT 1",
        )
        .bind(release_id)
        .fetch_optional(state.db())
        .await
        .unwrap_or(None);

        let (sha256, sha512, size) = if let Some(ar) = &artifact_row {
            (
                ar.try_get::<String, _>("sha256").ok(),
                ar.try_get::<Option<String>, _>("sha512").ok().flatten(),
                ar.try_get::<i64, _>("size_bytes").ok(),
            )
        } else {
            (None, None, None)
        };

        versions.push(VersionRecord {
            version,
            description: rr.try_get("description").unwrap_or(None),
            license: package_row.try_get("license").unwrap_or(None),
            homepage: package_row.try_get("homepage").unwrap_or(None),
            repository_url: package_row.try_get("repository_url").unwrap_or(None),
            keywords: package_row
                .try_get::<Vec<String>, _>("keywords")
                .unwrap_or_default(),
            is_deprecated: rr.try_get("is_deprecated").unwrap_or(false),
            deprecation_message: rr.try_get("deprecation_message").unwrap_or(None),
            is_yanked: rr.try_get("is_yanked").unwrap_or(false),
            tarball_sha256: sha256,
            tarball_sha512: sha512,
            tarball_size: size,
            published_at: rr
                .try_get("published_at")
                .unwrap_or_else(|_| Utc::now()),
            extra_metadata: rr.try_get("provenance").unwrap_or(None),
        });
    }

    // Load dist-tags (channel_refs)
    let tag_rows = match sqlx::query(
        "SELECT cr.name, r.version \
         FROM channel_refs cr \
         JOIN releases r ON r.id = cr.release_id \
         WHERE cr.package_id = $1",
    )
    .bind(package_id)
    .fetch_all(state.db())
    .await
    {
        Ok(rows) => rows,
        Err(_) => vec![],
    };

    let dist_tags: Vec<(String, String)> = tag_rows
        .iter()
        .filter_map(|tr| {
            let tag: String = tr.try_get("name").ok()?;
            let ver: String = tr.try_get("version").ok()?;
            Some((tag, ver))
        })
        .collect();

    let base_url = state.base_url().trim_end_matches('/');
    let tarball_base = format!(
        "{base_url}/npm/{}/-",
        urlencoded_package_name(&db_name)
    );

    let input = PackumentInput {
        name: db_name.clone(),
        description: package_row.try_get("description").unwrap_or(None),
        license: package_row.try_get("license").unwrap_or(None),
        homepage: package_row.try_get("homepage").unwrap_or(None),
        repository_url: package_row.try_get("repository_url").unwrap_or(None),
        keywords: package_row
            .try_get::<Vec<String>, _>("keywords")
            .unwrap_or_default(),
        readme: package_row.try_get("readme").unwrap_or(None),
        is_deprecated: package_row.try_get("is_deprecated").unwrap_or(false),
        deprecation_message: package_row.try_get("deprecation_message").unwrap_or(None),
        created_at: package_row
            .try_get("created_at")
            .unwrap_or_else(|_| Utc::now()),
        updated_at: package_row
            .try_get("updated_at")
            .unwrap_or_else(|_| Utc::now()),
        versions,
        dist_tags,
    };

    let packument = metadata::build_packument(&input, &tarball_base);

    (
        StatusCode::OK,
        [(CONTENT_TYPE, "application/json")],
        Json(packument),
    )
        .into_response()
}

// ─── PUT /:package — Publish ─────────────────────────────────────────────────

async fn publish_handler<S: NpmAppState>(
    State(state): State<S>,
    Path((scope, name)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let package_name = scoped_name(&scope, &name);
    publish_inner(&state, &package_name, &headers, body).await
}

async fn publish_handler_unscoped<S: NpmAppState>(
    State(state): State<S>,
    Path(package): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    publish_inner(&state, &package, &headers, body).await
}

async fn publish_inner<S: NpmAppState>(
    state: &S,
    package_name: &str,
    headers: &HeaderMap,
    body: Bytes,
) -> Response {
    // Auth
    let identity = match authenticate(state, headers).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    if !identity_has_scope(&identity, "packages:write") {
        return npm_error_response(
            StatusCode::FORBIDDEN,
            "Token does not have the packages:write scope",
        );
    }

    // Parse payload
    let payload: NpmPublishPayload = match serde_json::from_slice(&body) {
        Ok(p) => p,
        Err(e) => {
            return npm_error_response(
                StatusCode::BAD_REQUEST,
                &format!("Invalid publish payload: {e}"),
            )
        }
    };

    let parsed = match publish::parse_publish_payload(payload) {
        Ok(p) => p,
        Err(e) => return npm_error_response(StatusCode::BAD_REQUEST, &e.to_string()),
    };

    // Validate name matches path
    let normalized_path = normalize_package_name(package_name, &Ecosystem::Npm);
    let normalized_payload = normalize_package_name(&parsed.package_name, &Ecosystem::Npm);
    if normalized_path != normalized_payload {
        return npm_error_response(
            StatusCode::BAD_REQUEST,
            "Package name in URL does not match name in publish payload",
        );
    }

    if let Err(e) = validate_npm_package_name(&parsed.package_name) {
        return npm_error_response(StatusCode::BAD_REQUEST, &e.to_string());
    }

    let version_fields = extract_version_fields(&parsed.version_metadata);

    // Check if package exists
    let existing_package = sqlx::query(
        "SELECT id, owner_user_id, owner_org_id \
         FROM packages \
         WHERE ecosystem = 'npm' AND normalized_name = $1",
    )
    .bind(&normalized_payload)
    .fetch_optional(state.db())
    .await;

    let package_id = match existing_package {
        Ok(Some(row)) => {
            let pkg_id: Uuid = match row.try_get("id") {
                Ok(id) => id,
                Err(_) => {
                    return npm_error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Internal error",
                    )
                }
            };

            // Verify write access
            if !has_package_write_access(
                state.db(),
                pkg_id,
                row.try_get("owner_user_id").unwrap_or(None),
                row.try_get("owner_org_id").unwrap_or(None),
                identity.user_id,
            )
            .await
            {
                return npm_error_response(
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
                     readme         = COALESCE($6, readme), \
                     updated_at     = NOW() \
                 WHERE id = $7",
            )
            .bind(&version_fields.description)
            .bind(&version_fields.license)
            .bind(&version_fields.homepage)
            .bind(&version_fields.repository_url)
            .bind(if version_fields.keywords.is_empty() {
                None
            } else {
                Some(&version_fields.keywords)
            })
            .bind(&parsed.readme)
            .bind(pkg_id)
            .execute(state.db())
            .await;

            pkg_id
        }
        Ok(None) => {
            // Auto-create the package. Find the user's default repository or the
            // first writable repository they own.
            let repo_row = match sqlx::query(
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
            {
                Ok(Some(r)) => r,
                Ok(None) => {
                    return npm_error_response(
                        StatusCode::FORBIDDEN,
                        "You have no repository to publish into. Create a repository first via the Publaryn API.",
                    )
                }
                Err(_) => {
                    return npm_error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Database error",
                    )
                }
            };

            let repo_id: Uuid = repo_row.try_get("id").unwrap();
            let repo_visibility: String =
                repo_row.try_get("visibility").unwrap_or("public".into());

            let visibility = match repo_visibility.as_str() {
                "public" => "public",
                "private" => "private",
                "unlisted" => "unlisted",
                _ => "public",
            };

            let pkg = Package::new(
                repo_id,
                Ecosystem::Npm,
                parsed.package_name.clone(),
                match visibility {
                    "private" => publaryn_core::domain::repository::Visibility::Private,
                    "unlisted" => publaryn_core::domain::repository::Visibility::Unlisted,
                    _ => publaryn_core::domain::repository::Visibility::Public,
                },
            );

            let insert_result = sqlx::query(
                "INSERT INTO packages (id, repository_id, ecosystem, name, normalized_name, \
                 display_name, description, readme, homepage, repository_url, license, keywords, \
                 visibility, owner_user_id, owner_org_id, is_deprecated, deprecation_message, \
                 is_archived, download_count, created_at, updated_at) \
                 VALUES ($1, $2, 'npm', $3, $4, NULL, $5, $6, $7, $8, $9, $10, $11, $12, NULL, \
                 false, NULL, false, 0, $13, $14)",
            )
            .bind(pkg.id)
            .bind(repo_id)
            .bind(&parsed.package_name)
            .bind(&normalize_package_name(&parsed.package_name, &Ecosystem::Npm))
            .bind(&version_fields.description)
            .bind(&parsed.readme)
            .bind(&version_fields.homepage)
            .bind(&version_fields.repository_url)
            .bind(&version_fields.license)
            .bind(&version_fields.keywords)
            .bind(visibility)
            .bind(identity.user_id)
            .bind(pkg.created_at)
            .bind(pkg.updated_at)
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
                    .bind(pkg.id)
                    .bind(serde_json::json!({
                        "ecosystem": "npm",
                        "name": parsed.package_name,
                        "source": "npm_publish",
                    }))
                    .execute(state.db())
                    .await;

                    pkg.id
                }
                Err(e) => {
                    if let sqlx::Error::Database(ref db_err) = e {
                        if db_err.is_unique_violation() {
                            // Race condition: another request created it first, fetch it
                            match sqlx::query(
                                "SELECT id FROM packages \
                                 WHERE ecosystem = 'npm' AND normalized_name = $1",
                            )
                            .bind(&normalized_payload)
                            .fetch_optional(state.db())
                            .await
                            {
                                Ok(Some(row)) => row.try_get("id").unwrap(),
                                _ => {
                                    return npm_error_response(
                                        StatusCode::INTERNAL_SERVER_ERROR,
                                        "Failed to create package",
                                    )
                                }
                            }
                        } else {
                            return npm_error_response(
                                StatusCode::INTERNAL_SERVER_ERROR,
                                "Database error",
                            );
                        }
                    } else {
                        return npm_error_response(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "Database error",
                        );
                    }
                }
            }
        }
        Err(_) => {
            return npm_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error")
        }
    };

    // Check if version already exists
    let existing_release = sqlx::query("SELECT id FROM releases WHERE package_id = $1 AND version = $2")
        .bind(package_id)
        .bind(&parsed.version)
        .fetch_optional(state.db())
        .await;

    if matches!(existing_release, Ok(Some(_))) {
        return npm_error_response(
            StatusCode::CONFLICT,
            &format!(
                "Version {} already exists. You cannot publish over an existing version.",
                parsed.version
            ),
        );
    }

    // Create release in quarantine
    let is_prerelease = parsed.version.contains('-');
    let release = Release::new(package_id, parsed.version.clone(), identity.user_id);

    if let Err(_) = sqlx::query(
        "INSERT INTO releases (id, package_id, version, status, published_by, description, \
         changelog, is_prerelease, is_yanked, yank_reason, is_deprecated, deprecation_message, \
         source_ref, provenance, published_at, updated_at) \
         VALUES ($1, $2, $3, 'quarantine', $4, $5, NULL, $6, false, NULL, false, NULL, NULL, $7, $8, $9)",
    )
    .bind(release.id)
    .bind(package_id)
    .bind(&parsed.version)
    .bind(identity.user_id)
    .bind(&version_fields.description)
    .bind(is_prerelease)
    .bind(&parsed.version_metadata)
    .bind(release.published_at)
    .bind(release.updated_at)
    .execute(state.db())
    .await
    {
        return npm_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Failed to create release");
    }

    // Upload tarball to artifact storage
    let sha256 = hex::encode(Sha256::digest(&parsed.tarball_bytes));
    let sha512 = hex::encode(Sha512::digest(&parsed.tarball_bytes));
    let size_bytes = parsed.tarball_bytes.len() as i64;
    let storage_key = format!(
        "releases/{}/artifacts/{}/{}",
        release.id, sha256, parsed.tarball_filename
    );

    if let Err(_) = state
        .artifact_put(
            storage_key.clone(),
            parsed.tarball_content_type.clone(),
            parsed.tarball_bytes,
        )
        .await
    {
        // Cleanup: delete the release record
        let _ = sqlx::query("DELETE FROM releases WHERE id = $1")
            .bind(release.id)
            .execute(state.db())
            .await;
        return npm_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to store tarball",
        );
    }

    // Create artifact record
    let artifact = Artifact::new(
        release.id,
        ArtifactKind::Tarball,
        parsed.tarball_filename.clone(),
        storage_key.clone(),
        parsed.tarball_content_type,
        size_bytes,
        sha256.clone(),
    );

    if let Err(_) = sqlx::query(
        "INSERT INTO artifacts (id, release_id, kind, filename, storage_key, content_type, \
         size_bytes, sha256, sha512, md5, is_signed, signature_key_id, uploaded_at) \
         VALUES ($1, $2, 'tarball', $3, $4, $5, $6, $7, $8, NULL, false, NULL, $9) \
         ON CONFLICT (release_id, filename) DO NOTHING",
    )
    .bind(artifact.id)
    .bind(release.id)
    .bind(&artifact.filename)
    .bind(&artifact.storage_key)
    .bind(&artifact.content_type)
    .bind(size_bytes)
    .bind(&sha256)
    .bind(&sha512)
    .bind(artifact.uploaded_at)
    .execute(state.db())
    .await
    {
        return npm_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to record artifact",
        );
    }

    // Finalize: move release to published
    if let Err(_) = sqlx::query(
        "UPDATE releases SET status = 'published', updated_at = NOW() WHERE id = $1",
    )
    .bind(release.id)
    .execute(state.db())
    .await
    {
        return npm_error_response(
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
        "ecosystem": "npm",
        "name": parsed.package_name,
        "version": parsed.version,
        "source": "npm_publish",
        "artifact_sha256": sha256,
    }))
    .execute(state.db())
    .await;

    // Set dist-tags
    for (tag, version) in &parsed.dist_tags {
        let _ = set_dist_tag_inner(
            state.db(),
            package_id,
            tag,
            version,
            identity.user_id,
        )
        .await;
    }

    // Increment download count is not needed here (it's publish not download)
    // Trigger search reindex (best-effort)
    let _ = reindex_package(state.db(), package_id).await;

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "ok": true,
            "success": true,
            "id": format!("{}@{}", parsed.package_name, parsed.version),
        })),
    )
        .into_response()
}

// ─── GET /:package/-/:filename — Tarball download ────────────────────────────

async fn download_tarball<S: NpmAppState>(
    State(state): State<S>,
    Path((scope, name, filename)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Response {
    let package_name = scoped_name(&scope, &name);
    download_tarball_inner(&state, &package_name, &filename, &headers).await
}

async fn download_tarball_unscoped<S: NpmAppState>(
    State(state): State<S>,
    Path((package, filename)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    download_tarball_inner(&state, &package, &filename, &headers).await
}

async fn download_tarball_inner<S: NpmAppState>(
    state: &S,
    package_name: &str,
    filename: &str,
    headers: &HeaderMap,
) -> Response {
    let normalized = normalize_package_name(package_name, &Ecosystem::Npm);
    let actor_user_id = authenticate(state, headers).await.ok().map(|id| id.user_id);

    // Find the package
    let package_row = match sqlx::query(
        "SELECT p.id, p.visibility, p.owner_user_id, p.owner_org_id, \
                r.visibility AS repo_visibility, \
                r.owner_user_id AS repo_owner_user_id, \
                r.owner_org_id AS repo_owner_org_id \
         FROM packages p \
         JOIN repositories r ON r.id = p.repository_id \
         WHERE p.ecosystem = 'npm' AND p.normalized_name = $1",
    )
    .bind(&normalized)
    .fetch_optional(state.db())
    .await
    {
        Ok(Some(row)) => row,
        Ok(None) => return npm_error_response(StatusCode::NOT_FOUND, "Package not found"),
        Err(_) => return npm_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error"),
    };

    // Visibility check
    if !can_read_package(
        state.db(),
        &package_row.try_get::<String, _>("visibility").unwrap_or_default(),
        &package_row.try_get::<String, _>("repo_visibility").unwrap_or_default(),
        package_row.try_get("owner_user_id").unwrap_or(None),
        package_row.try_get("owner_org_id").unwrap_or(None),
        package_row.try_get("repo_owner_user_id").unwrap_or(None),
        package_row.try_get("repo_owner_org_id").unwrap_or(None),
        actor_user_id,
    )
    .await
    {
        return npm_error_response(StatusCode::NOT_FOUND, "Package not found");
    }

    let package_id: Uuid = match package_row.try_get("id") {
        Ok(id) => id,
        Err(_) => return npm_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Internal error"),
    };

    // Find artifact by filename across published releases
    let artifact_row = match sqlx::query(
        "SELECT a.storage_key, a.content_type, a.size_bytes, a.sha256 \
         FROM artifacts a \
         JOIN releases r ON r.id = a.release_id \
         WHERE r.package_id = $1 \
           AND a.filename = $2 \
           AND a.kind = 'tarball' \
           AND r.status IN ('published', 'deprecated', 'yanked') \
         LIMIT 1",
    )
    .bind(package_id)
    .bind(filename)
    .fetch_optional(state.db())
    .await
    {
        Ok(Some(row)) => row,
        Ok(None) => return npm_error_response(StatusCode::NOT_FOUND, "Tarball not found"),
        Err(_) => return npm_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error"),
    };

    let storage_key: String = artifact_row.try_get("storage_key").unwrap_or_default();
    let sha256: String = artifact_row.try_get("sha256").unwrap_or_default();
    let content_type: String = artifact_row
        .try_get("content_type")
        .unwrap_or("application/octet-stream".into());

    let stored = match state.artifact_get(&storage_key).await {
        Ok(Some(obj)) => obj,
        Ok(None) => {
            return npm_error_response(
                StatusCode::NOT_FOUND,
                "Tarball not found in storage",
            )
        }
        Err(_) => {
            return npm_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Storage error",
            )
        }
    };

    // Increment download counter (fire-and-forget)
    let _ = sqlx::query(
        "UPDATE packages SET download_count = download_count + 1 WHERE id = $1",
    )
    .bind(package_id)
    .execute(state.db())
    .await;

    let disposition = format!(
        "attachment; filename=\"{}\"",
        filename.replace('"', "")
    );

    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, content_type)
        .header(CONTENT_LENGTH, stored.bytes.len().to_string())
        .header(CONTENT_DISPOSITION, disposition)
        .header("x-checksum-sha256", sha256)
        .body(Body::from(stored.bytes))
        .unwrap_or_else(|_| {
            npm_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Internal error")
        })
}

// ─── GET /-/v1/search ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct NpmSearchQuery {
    text: Option<String>,
    size: Option<u32>,
    from: Option<u32>,
}

async fn search_handler<S: NpmAppState>(
    State(state): State<S>,
    Query(params): Query<NpmSearchQuery>,
) -> Response {
    let text = params.text.unwrap_or_default();
    let size = params.size.unwrap_or(20).min(250);
    let from = params.from.unwrap_or(0);

    let hits = match state.search_packages(&text, size, from).await {
        Ok(h) => h,
        Err(_) => vec![],
    };

    let objects: Vec<serde_json::Value> = hits
        .into_iter()
        .map(|hit| {
            serde_json::json!({
                "package": {
                    "name": hit.name,
                    "description": hit.description,
                    "keywords": hit.keywords,
                    "version": hit.version,
                    "date": hit.date,
                },
            })
        })
        .collect();

    let total = objects.len();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "objects": objects,
            "total": total,
            "time": "0ms",
        })),
    )
        .into_response()
}

// ─── GET /-/package/:package/dist-tags ───────────────────────────────────────

async fn list_dist_tags<S: NpmAppState>(
    State(state): State<S>,
    Path(package): Path<String>,
    headers: HeaderMap,
) -> Response {
    // The package path here may be URL-encoded scoped name: @scope%2Fname
    let package_name = percent_decode(&package);
    let normalized = normalize_package_name(&package_name, &Ecosystem::Npm);
    let actor_user_id = authenticate(&state, &headers).await.ok().map(|id| id.user_id);

    let package_id = match resolve_npm_package_id(state.db(), &normalized, actor_user_id).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    let rows = match sqlx::query(
        "SELECT cr.name, r.version \
         FROM channel_refs cr \
         JOIN releases r ON r.id = cr.release_id \
         WHERE cr.package_id = $1 \
         ORDER BY cr.name",
    )
    .bind(package_id)
    .fetch_all(state.db())
    .await
    {
        Ok(rows) => rows,
        Err(_) => return npm_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error"),
    };

    let mut tags = serde_json::Map::new();
    for row in rows {
        if let (Ok(tag), Ok(ver)) = (
            row.try_get::<String, _>("name"),
            row.try_get::<String, _>("version"),
        ) {
            tags.insert(tag, serde_json::Value::String(ver));
        }
    }

    (StatusCode::OK, Json(serde_json::Value::Object(tags))).into_response()
}

// ─── PUT /-/package/:package/dist-tags/:tag ──────────────────────────────────

async fn set_dist_tag<S: NpmAppState>(
    State(state): State<S>,
    Path((package, tag)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let identity = match authenticate(&state, &headers).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    if !identity_has_scope(&identity, "packages:write") {
        return npm_error_response(
            StatusCode::FORBIDDEN,
            "Token does not have the packages:write scope",
        );
    }

    let package_name = percent_decode(&package);
    let normalized = normalize_package_name(&package_name, &Ecosystem::Npm);

    let package_id =
        match resolve_npm_package_id_for_write(state.db(), &normalized, identity.user_id).await {
            Ok(id) => id,
            Err(resp) => return resp,
        };

    // npm sends the version as a JSON string (quoted)
    let version = String::from_utf8_lossy(&body).trim().trim_matches('"').to_owned();
    if version.is_empty() {
        return npm_error_response(StatusCode::BAD_REQUEST, "Version must not be empty");
    }

    match set_dist_tag_inner(state.db(), package_id, &tag, &version, identity.user_id).await {
        Ok(_) => (
            StatusCode::OK,
            Json(serde_json::json!({"ok": true})),
        )
            .into_response(),
        Err(resp) => resp,
    }
}

// ─── DELETE /-/package/:package/dist-tags/:tag ───────────────────────────────

async fn delete_dist_tag<S: NpmAppState>(
    State(state): State<S>,
    Path((package, tag)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let identity = match authenticate(&state, &headers).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    if !identity_has_scope(&identity, "packages:write") {
        return npm_error_response(
            StatusCode::FORBIDDEN,
            "Token does not have the packages:write scope",
        );
    }

    if tag == "latest" {
        return npm_error_response(
            StatusCode::BAD_REQUEST,
            "The 'latest' dist-tag cannot be removed",
        );
    }

    let package_name = percent_decode(&package);
    let normalized = normalize_package_name(&package_name, &Ecosystem::Npm);

    let package_id =
        match resolve_npm_package_id_for_write(state.db(), &normalized, identity.user_id).await {
            Ok(id) => id,
            Err(resp) => return resp,
        };

    match sqlx::query("DELETE FROM channel_refs WHERE package_id = $1 AND name = $2")
        .bind(package_id)
        .bind(&tag)
        .execute(state.db())
        .await
    {
        Ok(result) => {
            if result.rows_affected() == 0 {
                return npm_error_response(StatusCode::NOT_FOUND, "Dist-tag not found");
            }
            (
                StatusCode::OK,
                Json(serde_json::json!({"ok": true})),
            )
                .into_response()
        }
        Err(_) => npm_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error"),
    }
}

// ─── Shared helpers ──────────────────────────────────────────────────────────

async fn set_dist_tag_inner(
    db: &PgPool,
    package_id: Uuid,
    tag: &str,
    version: &str,
    actor_user_id: Uuid,
) -> Result<(), Response> {
    let release_row = sqlx::query("SELECT id FROM releases WHERE package_id = $1 AND version = $2")
        .bind(package_id)
        .bind(version)
        .fetch_optional(db)
        .await
        .map_err(|_| npm_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error"))?
        .ok_or_else(|| {
            npm_error_response(
                StatusCode::NOT_FOUND,
                &format!("Version '{version}' not found"),
            )
        })?;

    let release_id: Uuid = release_row
        .try_get("id")
        .map_err(|_| npm_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Internal error"))?;

    sqlx::query(
        "INSERT INTO channel_refs (id, package_id, ecosystem, name, release_id, created_by, created_at, updated_at) \
         VALUES ($1, $2, 'npm', $3, $4, $5, NOW(), NOW()) \
         ON CONFLICT (package_id, name) \
         DO UPDATE SET release_id = EXCLUDED.release_id, updated_at = NOW()",
    )
    .bind(Uuid::new_v4())
    .bind(package_id)
    .bind(tag)
    .bind(release_id)
    .bind(actor_user_id)
    .execute(db)
    .await
    .map_err(|_| npm_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error"))?;

    Ok(())
}

async fn resolve_npm_package_id(
    db: &PgPool,
    normalized_name: &str,
    actor_user_id: Option<Uuid>,
) -> Result<Uuid, Response> {
    let row = sqlx::query(
        "SELECT p.id, p.visibility, p.owner_user_id, p.owner_org_id, \
                r.visibility AS repo_visibility, \
                r.owner_user_id AS repo_owner_user_id, \
                r.owner_org_id AS repo_owner_org_id \
         FROM packages p \
         JOIN repositories r ON r.id = p.repository_id \
         WHERE p.ecosystem = 'npm' AND p.normalized_name = $1",
    )
    .bind(normalized_name)
    .fetch_optional(db)
    .await
    .map_err(|_| npm_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error"))?
    .ok_or_else(|| npm_error_response(StatusCode::NOT_FOUND, "Package not found"))?;

    if !can_read_package(
        db,
        &row.try_get::<String, _>("visibility").unwrap_or_default(),
        &row.try_get::<String, _>("repo_visibility").unwrap_or_default(),
        row.try_get("owner_user_id").unwrap_or(None),
        row.try_get("owner_org_id").unwrap_or(None),
        row.try_get("repo_owner_user_id").unwrap_or(None),
        row.try_get("repo_owner_org_id").unwrap_or(None),
        actor_user_id,
    )
    .await
    {
        return Err(npm_error_response(StatusCode::NOT_FOUND, "Package not found"));
    }

    row.try_get("id")
        .map_err(|_| npm_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Internal error"))
}

async fn resolve_npm_package_id_for_write(
    db: &PgPool,
    normalized_name: &str,
    actor_user_id: Uuid,
) -> Result<Uuid, Response> {
    let row = sqlx::query(
        "SELECT id, owner_user_id, owner_org_id \
         FROM packages \
         WHERE ecosystem = 'npm' AND normalized_name = $1",
    )
    .bind(normalized_name)
    .fetch_optional(db)
    .await
    .map_err(|_| npm_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error"))?
    .ok_or_else(|| npm_error_response(StatusCode::NOT_FOUND, "Package not found"))?;

    let package_id: Uuid = row
        .try_get("id")
        .map_err(|_| npm_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Internal error"))?;

    if !has_package_write_access(
        db,
        package_id,
        row.try_get("owner_user_id").unwrap_or(None),
        row.try_get("owner_org_id").unwrap_or(None),
        actor_user_id,
    )
    .await
    {
        return Err(npm_error_response(
            StatusCode::FORBIDDEN,
            "You do not have permission to modify this package",
        ));
    }

    Ok(package_id)
}

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
        let delegated_result = sqlx::query_scalar::<_, bool>(
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

        return delegated_result.unwrap_or(false);
    }

    false
}

async fn can_read_package(
    db: &PgPool,
    pkg_visibility: &str,
    repo_visibility: &str,
    pkg_owner_user_id: Option<Uuid>,
    pkg_owner_org_id: Option<Uuid>,
    repo_owner_user_id: Option<Uuid>,
    repo_owner_org_id: Option<Uuid>,
    actor_user_id: Option<Uuid>,
) -> bool {
    let pkg_anonymous = matches!(pkg_visibility, "public" | "unlisted");
    let repo_anonymous = matches!(repo_visibility, "public" | "unlisted");

    if pkg_anonymous && repo_anonymous {
        return true;
    }

    let Some(actor) = actor_user_id else {
        return false;
    };

    // Check if actor owns or is a member of the owning org
    let pkg_access = is_owner_or_member(db, pkg_owner_user_id, pkg_owner_org_id, actor).await;
    let repo_access = is_owner_or_member(db, repo_owner_user_id, repo_owner_org_id, actor).await;

    (pkg_anonymous || pkg_access) && (repo_anonymous || repo_access)
}

async fn is_owner_or_member(
    db: &PgPool,
    owner_user_id: Option<Uuid>,
    owner_org_id: Option<Uuid>,
    actor_user_id: Uuid,
) -> bool {
    if owner_user_id == Some(actor_user_id) {
        return true;
    }

    if let Some(org_id) = owner_org_id {
        let result = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM org_memberships WHERE org_id = $1 AND user_id = $2)",
        )
        .bind(org_id)
        .bind(actor_user_id)
        .fetch_one(db)
        .await;
        return result.unwrap_or(false);
    }

    false
}

async fn reindex_package(db: &PgPool, package_id: Uuid) -> Result<(), Error> {
    // Best-effort reindex. The search module handles this asynchronously.
    // For now we just update the package's updated_at to signal staleness.
    sqlx::query("UPDATE packages SET updated_at = NOW() WHERE id = $1")
        .bind(package_id)
        .execute(db)
        .await
        .map_err(Error::Database)?;
    Ok(())
}

fn percent_decode(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.bytes();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let hi = chars.next().unwrap_or(b'0');
            let lo = chars.next().unwrap_or(b'0');
            let decoded = (hex_val(hi) << 4) | hex_val(lo);
            result.push(decoded as char);
        } else {
            result.push(b as char);
        }
    }
    result
}

fn hex_val(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        _ => 0,
    }
}

fn urlencoded_package_name(name: &str) -> String {
    // Encode `@scope/name` → `@scope%2fname` for URL paths
    name.replace('/', "%2f")
}
