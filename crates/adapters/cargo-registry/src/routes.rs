//! Axum route handlers for the Cargo alternative registry protocol.
//!
//! This module implements both the **sparse index** (served under the index
//! mount, e.g. `/cargo/index/`) and the **Web API** (served under the API
//! mount, e.g. `/cargo/api/v1/`).
//!
//! The handlers translate Cargo wire-format requests into operations on the
//! shared Publaryn domain model (PostgreSQL metadata + S3 artifact storage).

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{
        header::{AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE},
        HeaderMap, StatusCode,
    },
    response::{IntoResponse, Response},
    routing::{delete, get, put},
    Json, Router,
};
use bytes::Bytes;
use chrono::Utc;
use serde::Deserialize;
use sha2::{Digest, Sha512};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use publaryn_core::error::Error;

use crate::{
    metadata::{self, VersionIndexInput},
    name::{normalize_crate_name, strip_build_metadata, validate_crate_name},
    publish::{self, CargoIndexDep},
};

// ─── Shared state trait ──────────────────────────────────────────────────────

/// Trait abstracting the application state needed by Cargo adapter routes.
///
/// The API crate's `AppState` implements this via a bridge, keeping the
/// adapter free from circular dependencies.
pub trait CargoAppState: Clone + Send + Sync + 'static {
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
    fn search_crates(
        &self,
        query: &str,
        per_page: u32,
        offset: u32,
    ) -> impl std::future::Future<Output = Result<Vec<CargoSearchHit>, Error>> + Send;
}

/// A retrieved object from artifact storage.
#[derive(Debug, Clone)]
pub struct StoredObject {
    pub content_type: String,
    pub bytes: Bytes,
}

/// A search result projected for the Cargo search response.
#[derive(Debug, Clone)]
pub struct CargoSearchHit {
    pub name: String,
    pub max_version: String,
    pub description: Option<String>,
}

/// Identity extracted from a bearer token.
#[derive(Debug, Clone)]
pub struct CargoIdentity {
    pub user_id: Uuid,
    pub token_id: Option<Uuid>,
    pub scopes: Vec<String>,
}

// ─── Routers ─────────────────────────────────────────────────────────────────

/// Sparse index router — mount under `/cargo/index` (or wherever the
/// `sparse+` URL points).
pub fn index_router<S: CargoAppState>() -> Router<S> {
    Router::new()
        .route("/config.json", get(config_json::<S>))
        // 1-char crate names
        .route("/1/:name", get(index_entry_1::<S>))
        // 2-char crate names
        .route("/2/:name", get(index_entry_2::<S>))
        // 3-char crate names
        .route("/3/:prefix/:name", get(index_entry_3::<S>))
        // 4+ char crate names
        .route("/:ab/:cd/:name", get(index_entry_4::<S>))
}

/// Web API router — mount under `/cargo/api/v1`.
pub fn api_router<S: CargoAppState>() -> Router<S> {
    Router::new()
        .route("/crates/new", put(publish_crate::<S>))
        .route("/crates/:name/:version/yank", delete(yank_version::<S>))
        .route("/crates/:name/:version/unyank", put(unyank_version::<S>))
        .route("/crates/:name/owners", get(list_owners::<S>))
        .route(
            "/crates/:name/owners",
            put(add_owners::<S>).delete(remove_owners::<S>),
        )
        .route("/crates", get(search_crates::<S>))
        .route("/crates/:name/:version/download", get(download_crate::<S>))
}

// ─── Auth ────────────────────────────────────────────────────────────────────

fn extract_bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers.get(AUTHORIZATION)?.to_str().ok().and_then(|val| {
        // Cargo sends the token value directly (no "Bearer " prefix) or with it.
        let trimmed = val.trim();
        if trimmed.is_empty() {
            return None;
        }
        // Support both `Bearer <token>` and raw `<token>` (Cargo default)
        if let Some(rest) = trimmed.strip_prefix("Bearer ") {
            let rest = rest.trim();
            if rest.is_empty() {
                None
            } else {
                Some(rest)
            }
        } else if let Some(rest) = trimmed.strip_prefix("bearer ") {
            let rest = rest.trim();
            if rest.is_empty() {
                None
            } else {
                Some(rest)
            }
        } else {
            // Raw token (Cargo default behaviour)
            Some(trimmed)
        }
    })
}

async fn authenticate<S: CargoAppState>(
    state: &S,
    headers: &HeaderMap,
) -> Result<CargoIdentity, Response> {
    let token = extract_bearer_token(headers).ok_or_else(|| {
        cargo_error_response(
            StatusCode::UNAUTHORIZED,
            "Authentication required. Run `cargo login --registry <name>` to authenticate.",
        )
    })?;

    // Try API token (prefixed with "pub_")
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
        .map_err(|_| cargo_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Internal error"))?
        .ok_or_else(|| {
            cargo_error_response(StatusCode::UNAUTHORIZED, "Invalid or revoked token")
        })?;

        let expires_at = row
            .try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("expires_at")
            .unwrap_or(None);
        if expires_at.is_some_and(|exp| exp <= Utc::now()) {
            return Err(cargo_error_response(
                StatusCode::UNAUTHORIZED,
                "Token has expired",
            ));
        }

        let token_kind: String = row.try_get("kind").map_err(|_| {
            cargo_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Internal error")
        })?;
        if token_kind == "oidc_derived" {
            return Err(cargo_error_response(
                StatusCode::UNAUTHORIZED,
                "OIDC-derived tokens are not valid for Cargo operations",
            ));
        }

        let token_id: Uuid = row.try_get("id").map_err(|_| {
            cargo_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Internal error")
        })?;
        let user_id: Option<Uuid> = row.try_get("user_id").unwrap_or(None);
        let user_id = user_id.ok_or_else(|| {
            cargo_error_response(
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

        return Ok(CargoIdentity {
            user_id,
            token_id: Some(token_id),
            scopes,
        });
    }

    // Try JWT
    let claims = publaryn_auth::validate_token(token, state.jwt_secret(), state.jwt_issuer())
        .map_err(|_| cargo_error_response(StatusCode::UNAUTHORIZED, "Invalid or expired token"))?;

    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| cargo_error_response(StatusCode::UNAUTHORIZED, "Invalid token subject"))?;
    let token_id = Uuid::parse_str(&claims.jti).ok();

    Ok(CargoIdentity {
        user_id,
        token_id,
        scopes: claims.scopes,
    })
}

fn identity_has_scope(identity: &CargoIdentity, scope: &str) -> bool {
    identity.scopes.iter().any(|s| s == scope)
}

// ─── Error format ────────────────────────────────────────────────────────────

/// Cargo expects errors as `{ "errors": [{ "detail": "..." }] }`.
fn cargo_error_response(status: StatusCode, message: &str) -> Response {
    let body = serde_json::json!({
        "errors": [{ "detail": message }]
    });
    (status, Json(body)).into_response()
}

// ─── Sparse Index: config.json ───────────────────────────────────────────────

async fn config_json<S: CargoAppState>(State(state): State<S>) -> Response {
    let base = state.base_url().trim_end_matches('/');
    let body = serde_json::json!({
        "dl": format!("{base}/cargo/api/v1/crates/{{crate}}/{{version}}/download"),
        "api": format!("{base}/cargo"),
        "auth-required": false
    });
    (StatusCode::OK, Json(body)).into_response()
}

// ─── Sparse Index: index entry ───────────────────────────────────────────────

async fn index_entry_1<S: CargoAppState>(
    State(state): State<S>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Response {
    serve_index_entry(&state, &name, &headers).await
}

async fn index_entry_2<S: CargoAppState>(
    State(state): State<S>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Response {
    serve_index_entry(&state, &name, &headers).await
}

async fn index_entry_3<S: CargoAppState>(
    State(state): State<S>,
    Path((_prefix, name)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    serve_index_entry(&state, &name, &headers).await
}

async fn index_entry_4<S: CargoAppState>(
    State(state): State<S>,
    Path((_ab, _cd, name)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Response {
    serve_index_entry(&state, &name, &headers).await
}

async fn serve_index_entry<S: CargoAppState>(
    state: &S,
    name: &str,
    headers: &HeaderMap,
) -> Response {
    let normalized = normalize_crate_name(name);

    // Find the package
    let package_row = match sqlx::query(
        "SELECT p.id, p.name, p.visibility, p.owner_user_id, p.owner_org_id, \
                r.visibility AS repo_visibility, \
                r.owner_user_id AS repo_owner_user_id, \
                r.owner_org_id AS repo_owner_org_id \
         FROM packages p \
         JOIN repositories r ON r.id = p.repository_id \
         WHERE p.ecosystem = 'cargo' AND p.normalized_name = $1",
    )
    .bind(&normalized)
    .fetch_optional(state.db())
    .await
    {
        Ok(Some(row)) => row,
        Ok(None) => return (StatusCode::NOT_FOUND, "").into_response(),
        Err(_) => return cargo_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error"),
    };

    // Visibility check — anonymous reads for public packages
    let pkg_vis: String = package_row.try_get("visibility").unwrap_or_default();
    let repo_vis: String = package_row.try_get("repo_visibility").unwrap_or_default();
    let actor_user_id = authenticate(state, headers).await.ok().map(|id| id.user_id);
    if !can_read_package(
        state.db(),
        &pkg_vis,
        &repo_vis,
        package_row.try_get("owner_user_id").unwrap_or(None),
        package_row.try_get("owner_org_id").unwrap_or(None),
        package_row.try_get("repo_owner_user_id").unwrap_or(None),
        package_row.try_get("repo_owner_org_id").unwrap_or(None),
        actor_user_id,
    )
    .await
    {
        return (StatusCode::NOT_FOUND, "").into_response();
    }

    let package_id: Uuid = match package_row.try_get("id") {
        Ok(id) => id,
        Err(_) => return cargo_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Internal error"),
    };
    let db_name: String = package_row.try_get("name").unwrap_or_default();

    // Load all published/yanked releases + their cargo metadata + artifact checksums
    let rows = match sqlx::query(
        "SELECT r.version, r.is_yanked, \
                a.sha256 AS cksum, \
                cm.deps, cm.features, cm.features2, cm.links, cm.rust_version \
         FROM releases r \
         JOIN artifacts a ON a.release_id = r.id AND a.kind = 'crate' \
         LEFT JOIN cargo_release_metadata cm ON cm.release_id = r.id \
         WHERE r.package_id = $1 \
           AND r.status IN ('published', 'deprecated', 'yanked') \
         ORDER BY r.published_at ASC",
    )
    .bind(package_id)
    .fetch_all(state.db())
    .await
    {
        Ok(rows) => rows,
        Err(_) => return cargo_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error"),
    };

    if rows.is_empty() {
        return (StatusCode::NOT_FOUND, "").into_response();
    }

    let mut versions = Vec::with_capacity(rows.len());
    for row in &rows {
        let deps_json: serde_json::Value = row.try_get("deps").unwrap_or(serde_json::json!([]));
        let deps: Vec<CargoIndexDep> = serde_json::from_value(deps_json).unwrap_or_default();

        let features: serde_json::Map<String, serde_json::Value> = row
            .try_get::<serde_json::Value, _>("features")
            .ok()
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();

        let features2: Option<serde_json::Map<String, serde_json::Value>> = row
            .try_get::<Option<serde_json::Value>, _>("features2")
            .ok()
            .flatten()
            .and_then(|v| v.as_object().cloned());

        versions.push(VersionIndexInput {
            name: db_name.clone(),
            version: row.try_get("version").unwrap_or_default(),
            deps,
            features,
            features2,
            cksum: row.try_get("cksum").unwrap_or_default(),
            yanked: row.try_get("is_yanked").unwrap_or(false),
            links: row.try_get("links").unwrap_or(None),
            rust_version: row.try_get("rust_version").unwrap_or(None),
        });
    }

    let (content, etag) = metadata::build_index_content(&versions);

    // ETag / conditional request support
    if let Some(if_none_match) = headers.get("if-none-match") {
        if let Ok(val) = if_none_match.to_str() {
            let quoted_etag = format!("\"{etag}\"");
            if val == quoted_etag || val == etag {
                return Response::builder()
                    .status(StatusCode::NOT_MODIFIED)
                    .header("etag", &quoted_etag)
                    .body(Body::empty())
                    .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
            }
        }
    }

    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "text/plain")
        .header("etag", format!("\"{etag}\""))
        .header("cache-control", "public, max-age=60")
        .body(Body::from(content))
        .unwrap_or_else(|_| {
            cargo_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Internal error")
        })
}

// ─── PUT /crates/new — Publish ───────────────────────────────────────────────

async fn publish_crate<S: CargoAppState>(
    State(state): State<S>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    // Auth
    let identity = match authenticate(&state, &headers).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    if !identity_has_scope(&identity, "packages:write") {
        return cargo_error_response(
            StatusCode::FORBIDDEN,
            "Token does not have the packages:write scope",
        );
    }

    // Parse binary wire format
    let parsed = match publish::parse_cargo_publish(&body) {
        Ok(p) => p,
        Err(e) => return cargo_error_response(StatusCode::BAD_REQUEST, &e.to_string()),
    };

    // Validate crate name
    if let Err(e) = validate_crate_name(&parsed.metadata.name) {
        return cargo_error_response(StatusCode::BAD_REQUEST, &e.to_string());
    }

    let normalized = normalize_crate_name(&parsed.metadata.name);
    let version = strip_build_metadata(&parsed.metadata.vers).to_owned();

    // Check if package exists
    let existing = sqlx::query(
        "SELECT id, owner_user_id, owner_org_id \
         FROM packages \
         WHERE ecosystem = 'cargo' AND normalized_name = $1",
    )
    .bind(&normalized)
    .fetch_optional(state.db())
    .await;

    let package_id = match existing {
        Ok(Some(row)) => {
            let pkg_id: Uuid = match row.try_get("id") {
                Ok(id) => id,
                Err(_) => {
                    return cargo_error_response(
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
                return cargo_error_response(
                    StatusCode::FORBIDDEN,
                    "You do not have permission to publish to this crate",
                );
            }

            // Update metadata from publish
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
            .bind(&parsed.metadata.description)
            .bind(&parsed.metadata.license)
            .bind(&parsed.metadata.homepage)
            .bind(&parsed.metadata.repository)
            .bind(if parsed.metadata.keywords.is_empty() {
                None
            } else {
                Some(&parsed.metadata.keywords)
            })
            .bind(&parsed.metadata.readme)
            .bind(pkg_id)
            .execute(state.db())
            .await;

            pkg_id
        }
        Ok(None) => {
            // Auto-create: find user's first writable repository
            let repo_row = match sqlx::query(
                "SELECT id, visibility \
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
                    return cargo_error_response(
                        StatusCode::FORBIDDEN,
                        "You have no repository to publish into. Create a repository first via the Publaryn API.",
                    )
                }
                Err(_) => {
                    return cargo_error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Database error",
                    )
                }
            };

            let repo_id: Uuid = repo_row.try_get("id").unwrap();
            let repo_visibility: String = repo_row.try_get("visibility").unwrap_or("public".into());

            let visibility = match repo_visibility.as_str() {
                "private" => "private",
                "unlisted" => "unlisted",
                _ => "public",
            };

            let pkg_id = Uuid::new_v4();
            let now = Utc::now();

            let insert_result = sqlx::query(
                "INSERT INTO packages (id, repository_id, ecosystem, name, normalized_name, \
                 display_name, description, readme, homepage, repository_url, license, keywords, \
                 visibility, owner_user_id, owner_org_id, is_deprecated, deprecation_message, \
                 is_archived, download_count, created_at, updated_at) \
                 VALUES ($1, $2, 'cargo', $3, $4, NULL, $5, $6, $7, $8, $9, $10, $11, $12, NULL, \
                 false, NULL, false, 0, $13, $14)",
            )
            .bind(pkg_id)
            .bind(repo_id)
            .bind(&parsed.metadata.name)
            .bind(&normalized)
            .bind(&parsed.metadata.description)
            .bind(&parsed.metadata.readme)
            .bind(&parsed.metadata.homepage)
            .bind(&parsed.metadata.repository)
            .bind(&parsed.metadata.license)
            .bind(&parsed.metadata.keywords)
            .bind(visibility)
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
                        "ecosystem": "cargo",
                        "name": parsed.metadata.name,
                        "source": "cargo_publish",
                    }))
                    .execute(state.db())
                    .await;

                    pkg_id
                }
                Err(e) => {
                    if let sqlx::Error::Database(ref db_err) = e {
                        if db_err.is_unique_violation() {
                            // Race: another request created it; fetch the winner
                            match sqlx::query(
                                "SELECT id FROM packages \
                                 WHERE ecosystem = 'cargo' AND normalized_name = $1",
                            )
                            .bind(&normalized)
                            .fetch_optional(state.db())
                            .await
                            {
                                Ok(Some(row)) => row.try_get("id").unwrap(),
                                _ => {
                                    return cargo_error_response(
                                        StatusCode::INTERNAL_SERVER_ERROR,
                                        "Failed to create crate",
                                    )
                                }
                            }
                        } else {
                            return cargo_error_response(
                                StatusCode::INTERNAL_SERVER_ERROR,
                                "Database error",
                            );
                        }
                    } else {
                        return cargo_error_response(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "Database error",
                        );
                    }
                }
            }
        }
        Err(_) => return cargo_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error"),
    };

    // Check version uniqueness (build metadata stripped)
    let existing_release =
        sqlx::query("SELECT id FROM releases WHERE package_id = $1 AND version = $2")
            .bind(package_id)
            .bind(&version)
            .fetch_optional(state.db())
            .await;

    if matches!(existing_release, Ok(Some(_))) {
        return cargo_error_response(
            StatusCode::CONFLICT,
            &format!(
                "Crate version {version} already exists. You cannot publish over an existing version."
            ),
        );
    }

    // Create release in quarantine
    let is_prerelease = version.contains('-');
    let release_id = Uuid::new_v4();
    let now = Utc::now();

    if sqlx::query(
        "INSERT INTO releases (id, package_id, version, status, published_by, description, \
         changelog, is_prerelease, is_yanked, yank_reason, is_deprecated, deprecation_message, \
         source_ref, provenance, published_at, updated_at) \
         VALUES ($1, $2, $3, 'quarantine', $4, $5, NULL, $6, false, NULL, false, NULL, NULL, NULL, $7, $8)",
    )
    .bind(release_id)
    .bind(package_id)
    .bind(&version)
    .bind(identity.user_id)
    .bind(&parsed.metadata.description)
    .bind(is_prerelease)
    .bind(now)
    .bind(now)
    .execute(state.db())
    .await
    .is_err()
    {
        return cargo_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to create release",
        );
    }

    // Upload .crate to artifact storage
    let sha512 = hex::encode(Sha512::digest(&parsed.crate_bytes));
    let size_bytes = parsed.crate_bytes.len() as i64;
    let filename = format!("{}-{}.crate", parsed.metadata.name, version);
    let storage_key = format!(
        "releases/{}/artifacts/{}/{}",
        release_id, parsed.sha256, filename
    );

    if state
        .artifact_put(
            storage_key.clone(),
            "application/gzip".into(),
            parsed.crate_bytes,
        )
        .await
        .is_err()
    {
        // Cleanup release record
        let _ = sqlx::query("DELETE FROM releases WHERE id = $1")
            .bind(release_id)
            .execute(state.db())
            .await;
        return cargo_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to store .crate file",
        );
    }

    // Create artifact record
    let artifact_id = Uuid::new_v4();
    if sqlx::query(
        "INSERT INTO artifacts (id, release_id, kind, filename, storage_key, content_type, \
         size_bytes, sha256, sha512, md5, is_signed, signature_key_id, uploaded_at) \
         VALUES ($1, $2, 'crate', $3, $4, 'application/gzip', $5, $6, $7, NULL, false, NULL, $8) \
         ON CONFLICT (release_id, filename) DO NOTHING",
    )
    .bind(artifact_id)
    .bind(release_id)
    .bind(&filename)
    .bind(&storage_key)
    .bind(size_bytes)
    .bind(&parsed.sha256)
    .bind(&sha512)
    .bind(now)
    .execute(state.db())
    .await
    .is_err()
    {
        return cargo_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to record artifact",
        );
    }

    // Store Cargo-specific release metadata (deps, features, etc.)
    let deps_json = serde_json::to_value(&parsed.index_deps).unwrap_or_default();
    let features_json = serde_json::Value::Object(parsed.metadata.features.clone());
    // features2: currently we store the same as features for v2 compat if features
    // contain dep: prefixed entries; otherwise None.
    let features2_json: Option<serde_json::Value> = {
        let has_v2 = parsed.metadata.features.values().any(|v| {
            v.as_array().map_or(false, |arr| {
                arr.iter()
                    .any(|item| item.as_str().map_or(false, |s| s.starts_with("dep:")))
            })
        });
        if has_v2 {
            Some(serde_json::Value::Object(parsed.metadata.features.clone()))
        } else {
            None
        }
    };

    let _ = sqlx::query(
        "INSERT INTO cargo_release_metadata (release_id, deps, features, features2, links, rust_version, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6, NOW()) \
         ON CONFLICT (release_id) DO NOTHING",
    )
    .bind(release_id)
    .bind(&deps_json)
    .bind(&features_json)
    .bind(&features2_json)
    .bind(&parsed.metadata.links)
    .bind(&parsed.metadata.rust_version)
    .execute(state.db())
    .await;

    // Finalize: move release to published
    if sqlx::query("UPDATE releases SET status = 'published', updated_at = NOW() WHERE id = $1")
        .bind(release_id)
        .execute(state.db())
        .await
        .is_err()
    {
        return cargo_error_response(
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
    .bind(release_id)
    .bind(serde_json::json!({
        "ecosystem": "cargo",
        "name": parsed.metadata.name,
        "version": version,
        "source": "cargo_publish",
        "artifact_sha256": parsed.sha256,
    }))
    .execute(state.db())
    .await;

    // Trigger search reindex (best-effort)
    let _ = reindex_package(state.db(), package_id).await;

    let body = serde_json::json!({
        "warnings": {
            "invalid_categories": [],
            "invalid_badges": [],
            "other": []
        }
    });

    (StatusCode::OK, Json(body)).into_response()
}

// ─── DELETE /crates/:name/:version/yank ──────────────────────────────────────

async fn yank_version<S: CargoAppState>(
    State(state): State<S>,
    Path((name, version)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let identity = match authenticate(&state, &headers).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    if !identity_has_scope(&identity, "packages:write") {
        return cargo_error_response(
            StatusCode::FORBIDDEN,
            "Token does not have the packages:write scope",
        );
    }

    let normalized = normalize_crate_name(&name);
    let (package_id, release_id) =
        match resolve_cargo_release_for_write(&state, &normalized, &version, &identity).await {
            Ok(ids) => ids,
            Err(resp) => return resp,
        };

    if sqlx::query(
        "UPDATE releases SET is_yanked = true, status = 'yanked', updated_at = NOW() \
         WHERE id = $1",
    )
    .bind(release_id)
    .execute(state.db())
    .await
    .is_err()
    {
        return cargo_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Failed to yank version");
    }

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
        "ecosystem": "cargo",
        "name": name,
        "version": version,
    }))
    .execute(state.db())
    .await;

    (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
}

// ─── PUT /crates/:name/:version/unyank ───────────────────────────────────────

async fn unyank_version<S: CargoAppState>(
    State(state): State<S>,
    Path((name, version)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let identity = match authenticate(&state, &headers).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    if !identity_has_scope(&identity, "packages:write") {
        return cargo_error_response(
            StatusCode::FORBIDDEN,
            "Token does not have the packages:write scope",
        );
    }

    let normalized = normalize_crate_name(&name);
    let (package_id, release_id) =
        match resolve_cargo_release_for_write(&state, &normalized, &version, &identity).await {
            Ok(ids) => ids,
            Err(resp) => return resp,
        };

    if sqlx::query(
        "UPDATE releases SET is_yanked = false, status = 'published', updated_at = NOW() \
         WHERE id = $1",
    )
    .bind(release_id)
    .execute(state.db())
    .await
    .is_err()
    {
        return cargo_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to unyank version",
        );
    }

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
        "ecosystem": "cargo",
        "name": name,
        "version": version,
    }))
    .execute(state.db())
    .await;

    (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
}

// ─── GET /crates/:name/owners ────────────────────────────────────────────────

async fn list_owners<S: CargoAppState>(
    State(state): State<S>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Response {
    let _identity = match authenticate(&state, &headers).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    let normalized = normalize_crate_name(&name);
    let pkg_row = match sqlx::query(
        "SELECT id, owner_user_id, owner_org_id FROM packages \
         WHERE ecosystem = 'cargo' AND normalized_name = $1",
    )
    .bind(&normalized)
    .fetch_optional(state.db())
    .await
    {
        Ok(Some(r)) => r,
        Ok(None) => return cargo_error_response(StatusCode::NOT_FOUND, "Crate not found"),
        Err(_) => return cargo_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error"),
    };

    let mut users = Vec::new();

    // Add the direct owner
    if let Ok(Some(uid)) = pkg_row.try_get::<Option<Uuid>, _>("owner_user_id") {
        if let Ok(Some(row)) = sqlx::query("SELECT id, username FROM users WHERE id = $1")
            .bind(uid)
            .fetch_optional(state.db())
            .await
        {
            let id: i64 = row
                .try_get::<Uuid, _>("id")
                .map(|u| u.as_u128() as i64)
                .unwrap_or(0);
            users.push(serde_json::json!({
                "id": id,
                "login": row.try_get::<String, _>("username").unwrap_or_default(),
                "name": row.try_get::<String, _>("username").unwrap_or_default(),
            }));
        }
    }

    // For org-owned crates, list org admins as owners
    if let Ok(Some(org_id)) = pkg_row.try_get::<Option<Uuid>, _>("owner_org_id") {
        let admin_rows = sqlx::query(
            "SELECT u.id, u.username \
             FROM org_memberships om \
             JOIN users u ON u.id = om.user_id \
             WHERE om.org_id = $1 AND om.role::text IN ('owner', 'admin')",
        )
        .bind(org_id)
        .fetch_all(state.db())
        .await
        .unwrap_or_default();

        for row in admin_rows {
            let id: i64 = row
                .try_get::<Uuid, _>("id")
                .map(|u| u.as_u128() as i64)
                .unwrap_or(0);
            users.push(serde_json::json!({
                "id": id,
                "login": row.try_get::<String, _>("username").unwrap_or_default(),
                "name": row.try_get::<String, _>("username").unwrap_or_default(),
            }));
        }
    }

    (StatusCode::OK, Json(serde_json::json!({ "users": users }))).into_response()
}

// ─── PUT /crates/:name/owners — Add ─────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct OwnersPayload {
    users: Vec<String>,
}

async fn add_owners<S: CargoAppState>(
    State(state): State<S>,
    Path(name): Path<String>,
    headers: HeaderMap,
    Json(payload): Json<OwnersPayload>,
) -> Response {
    let identity = match authenticate(&state, &headers).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    if !identity_has_scope(&identity, "packages:write") {
        return cargo_error_response(
            StatusCode::FORBIDDEN,
            "Token does not have the packages:write scope",
        );
    }

    let normalized = normalize_crate_name(&name);
    let _pkg_id =
        match resolve_cargo_package_for_write(state.db(), &normalized, identity.user_id).await {
            Ok(id) => id,
            Err(resp) => return resp,
        };

    // NOTE: Full co-owner model is deferred. For now we acknowledge the request
    // and return a message. Org-level membership changes should go through the
    // Publaryn control-plane API.
    let msg = format!(
        "Owner management for Cargo crates is handled via the Publaryn API. \
         Requested users: {:?}",
        payload.users
    );
    (
        StatusCode::OK,
        Json(serde_json::json!({ "ok": true, "msg": msg })),
    )
        .into_response()
}

// ─── DELETE /crates/:name/owners — Remove ────────────────────────────────────

async fn remove_owners<S: CargoAppState>(
    State(state): State<S>,
    Path(name): Path<String>,
    headers: HeaderMap,
    Json(payload): Json<OwnersPayload>,
) -> Response {
    let identity = match authenticate(&state, &headers).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    if !identity_has_scope(&identity, "packages:write") {
        return cargo_error_response(
            StatusCode::FORBIDDEN,
            "Token does not have the packages:write scope",
        );
    }

    let normalized = normalize_crate_name(&name);
    let _pkg_id =
        match resolve_cargo_package_for_write(state.db(), &normalized, identity.user_id).await {
            Ok(id) => id,
            Err(resp) => return resp,
        };

    let msg = format!(
        "Owner management for Cargo crates is handled via the Publaryn API. \
         Requested removal: {:?}",
        payload.users
    );
    (
        StatusCode::OK,
        Json(serde_json::json!({ "ok": true, "msg": msg })),
    )
        .into_response()
}

// ─── GET /crates — Search ────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct CargoSearchQuery {
    q: Option<String>,
    per_page: Option<u32>,
}

async fn search_crates<S: CargoAppState>(
    State(state): State<S>,
    Query(params): Query<CargoSearchQuery>,
) -> Response {
    let q = params.q.unwrap_or_default();
    let per_page = params.per_page.unwrap_or(10).min(100);

    let hits = match state.search_crates(&q, per_page, 0).await {
        Ok(h) => h,
        Err(_) => vec![],
    };

    let total = hits.len();
    let crates: Vec<serde_json::Value> = hits
        .into_iter()
        .map(|hit| {
            serde_json::json!({
                "name": hit.name,
                "max_version": hit.max_version,
                "description": hit.description,
            })
        })
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "crates": crates,
            "meta": { "total": total },
        })),
    )
        .into_response()
}

// ─── GET /crates/:name/:version/download ─────────────────────────────────────

async fn download_crate<S: CargoAppState>(
    State(state): State<S>,
    Path((name, version)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let normalized = normalize_crate_name(&name);
    let actor_user_id = authenticate(&state, &headers)
        .await
        .ok()
        .map(|id| id.user_id);

    // Find package with visibility check
    let package_row = match sqlx::query(
        "SELECT p.id, p.visibility, p.owner_user_id, p.owner_org_id, \
                r.visibility AS repo_visibility, \
                r.owner_user_id AS repo_owner_user_id, \
                r.owner_org_id AS repo_owner_org_id \
         FROM packages p \
         JOIN repositories r ON r.id = p.repository_id \
         WHERE p.ecosystem = 'cargo' AND p.normalized_name = $1",
    )
    .bind(&normalized)
    .fetch_optional(state.db())
    .await
    {
        Ok(Some(row)) => row,
        Ok(None) => return cargo_error_response(StatusCode::NOT_FOUND, "Crate not found"),
        Err(_) => return cargo_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error"),
    };

    if !can_read_package(
        state.db(),
        &package_row
            .try_get::<String, _>("visibility")
            .unwrap_or_default(),
        &package_row
            .try_get::<String, _>("repo_visibility")
            .unwrap_or_default(),
        package_row.try_get("owner_user_id").unwrap_or(None),
        package_row.try_get("owner_org_id").unwrap_or(None),
        package_row.try_get("repo_owner_user_id").unwrap_or(None),
        package_row.try_get("repo_owner_org_id").unwrap_or(None),
        actor_user_id,
    )
    .await
    {
        return cargo_error_response(StatusCode::NOT_FOUND, "Crate not found");
    }

    let package_id: Uuid = match package_row.try_get("id") {
        Ok(id) => id,
        Err(_) => return cargo_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Internal error"),
    };

    // Find the .crate artifact for this version
    let artifact_row = match sqlx::query(
        "SELECT a.storage_key, a.sha256, a.size_bytes \
         FROM artifacts a \
         JOIN releases r ON r.id = a.release_id \
         WHERE r.package_id = $1 \
           AND r.version = $2 \
           AND a.kind = 'crate' \
           AND r.status IN ('published', 'deprecated', 'yanked') \
         LIMIT 1",
    )
    .bind(package_id)
    .bind(&version)
    .fetch_optional(state.db())
    .await
    {
        Ok(Some(row)) => row,
        Ok(None) => return cargo_error_response(StatusCode::NOT_FOUND, "Version not found"),
        Err(_) => return cargo_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error"),
    };

    let storage_key: String = artifact_row.try_get("storage_key").unwrap_or_default();
    let sha256: String = artifact_row.try_get("sha256").unwrap_or_default();

    let stored = match state.artifact_get(&storage_key).await {
        Ok(Some(obj)) => obj,
        Ok(None) => {
            return cargo_error_response(StatusCode::NOT_FOUND, ".crate file not found in storage")
        }
        Err(_) => return cargo_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Storage error"),
    };

    // Increment download counter (fire-and-forget)
    let _ = sqlx::query("UPDATE packages SET download_count = download_count + 1 WHERE id = $1")
        .bind(package_id)
        .execute(state.db())
        .await;

    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "application/gzip")
        .header(CONTENT_LENGTH, stored.bytes.len().to_string())
        .header("etag", format!("\"{sha256}\""))
        .body(Body::from(stored.bytes))
        .unwrap_or_else(|_| {
            cargo_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Internal error")
        })
}

// ─── Shared helpers ──────────────────────────────────────────────────────────

async fn resolve_cargo_release_for_write<S: CargoAppState>(
    state: &S,
    normalized_name: &str,
    version: &str,
    identity: &CargoIdentity,
) -> Result<(Uuid, Uuid), Response> {
    let pkg_row = sqlx::query(
        "SELECT id, owner_user_id, owner_org_id FROM packages \
         WHERE ecosystem = 'cargo' AND normalized_name = $1",
    )
    .bind(normalized_name)
    .fetch_optional(state.db())
    .await
    .map_err(|_| cargo_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error"))?
    .ok_or_else(|| cargo_error_response(StatusCode::NOT_FOUND, "Crate not found"))?;

    let package_id: Uuid = pkg_row
        .try_get("id")
        .map_err(|_| cargo_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Internal error"))?;

    if !has_package_write_access(
        state.db(),
        package_id,
        pkg_row.try_get("owner_user_id").unwrap_or(None),
        pkg_row.try_get("owner_org_id").unwrap_or(None),
        identity.user_id,
    )
    .await
    {
        return Err(cargo_error_response(
            StatusCode::FORBIDDEN,
            "You do not have permission to modify this crate",
        ));
    }

    let release_row = sqlx::query(
        "SELECT id FROM releases \
         WHERE package_id = $1 AND version = $2 \
           AND status IN ('published', 'deprecated', 'yanked')",
    )
    .bind(package_id)
    .bind(version)
    .fetch_optional(state.db())
    .await
    .map_err(|_| cargo_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error"))?
    .ok_or_else(|| cargo_error_response(StatusCode::NOT_FOUND, "Version not found"))?;

    let release_id: Uuid = release_row
        .try_get("id")
        .map_err(|_| cargo_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Internal error"))?;

    Ok((package_id, release_id))
}

async fn resolve_cargo_package_for_write(
    db: &PgPool,
    normalized_name: &str,
    actor_user_id: Uuid,
) -> Result<Uuid, Response> {
    let row = sqlx::query(
        "SELECT id, owner_user_id, owner_org_id FROM packages \
         WHERE ecosystem = 'cargo' AND normalized_name = $1",
    )
    .bind(normalized_name)
    .fetch_optional(db)
    .await
    .map_err(|_| cargo_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error"))?
    .ok_or_else(|| cargo_error_response(StatusCode::NOT_FOUND, "Crate not found"))?;

    let package_id: Uuid = row
        .try_get("id")
        .map_err(|_| cargo_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Internal error"))?;

    if !has_package_write_access(
        db,
        package_id,
        row.try_get("owner_user_id").unwrap_or(None),
        row.try_get("owner_org_id").unwrap_or(None),
        actor_user_id,
    )
    .await
    {
        return Err(cargo_error_response(
            StatusCode::FORBIDDEN,
            "You do not have permission to modify this crate",
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
    sqlx::query("UPDATE packages SET updated_at = NOW() WHERE id = $1")
        .bind(package_id)
        .execute(db)
        .await
        .map_err(Error::Database)?;
    Ok(())
}
