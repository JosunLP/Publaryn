//! Axum route handlers for a RubyGems-compatible read surface.
//!
//! This adapter implements:
//! - `GET /api/v1/gems/:name.json`        — gem metadata
//! - `GET /api/v1/versions/:name.json`    — version listing
//! - `GET /gems/:filename`                — download
//! - `POST /api/v1/gems`                  — `gem push`
//! - `DELETE /api/v1/gems/yank`           — `gem yank`
//! - `POST /api/v1/api_key`               — compat endpoint for `gem signin`

use axum::{
    body::{to_bytes, Body},
    extract::{Path, Request, State},
    http::{
        header::{AUTHORIZATION, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE},
        HeaderMap, StatusCode,
    },
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Form, Json, Router,
};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use bytes::Bytes;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use publaryn_core::{
    domain::{namespace::Ecosystem, package::normalize_package_name},
    error::Error,
};

use crate::{
    metadata::{build_gem_metadata, build_versions_list, GemMetadataInput, GemVersionListItem},
    name::validate_rubygems_package_name,
    publish::{self, ParsedGemPush},
};

pub trait RubyGemsAppState: Clone + Send + Sync + 'static {
    fn db(&self) -> &PgPool;
    fn artifact_get(
        &self,
        key: &str,
    ) -> impl std::future::Future<Output = Result<Option<StoredObject>, Error>> + Send;
    fn artifact_put(
        &self,
        key: String,
        content_type: String,
        bytes: Bytes,
    ) -> impl std::future::Future<Output = Result<(), Error>> + Send;
    fn base_url(&self) -> &str;
    fn jwt_secret(&self) -> &str;
    fn jwt_issuer(&self) -> &str;
}

#[derive(Debug, Clone)]
pub struct StoredObject {
    pub content_type: String,
    pub bytes: Bytes,
}

#[derive(Debug, Clone)]
struct RubyGemsIdentity {
    user_id: Uuid,
    token_id: Option<Uuid>,
    scopes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ErrorDocument<'a> {
    error: &'a str,
}

pub fn router<S: RubyGemsAppState>() -> Router<S> {
    Router::new()
        .route("/api/v1/gems/{name}", get(gem_metadata::<S>))
        .route("/api/v1/versions/{name}", get(gem_versions::<S>))
        .route("/gems/{filename}", get(download_gem::<S>))
        // Publish surface
        .route("/api/v1/gems", post(push_gem::<S>))
        .route("/api/v1/gems/yank", delete(yank_gem::<S>))
        .route("/api/v1/api_key", post(echo_api_key::<S>))
}

async fn gem_metadata<S: RubyGemsAppState>(
    State(state): State<S>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Response {
    let identity = match authenticate_optional(&state, &headers).await {
        Ok(identity) => identity,
        Err(response) => return response,
    };

    let package_name = match name.strip_suffix(".json") {
        Some(name) => name.to_owned(),
        None => name,
    };
    if validate_rubygems_package_name(&package_name).is_err() {
        return not_found_response("Gem not found");
    }

    let actor_user_id = identity.as_ref().map(|identity| identity.user_id);
    let package_row = match load_package_row(state.db(), &package_name).await {
        Ok(row) => row,
        Err(response) => return response,
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
        return not_found_response("Gem not found");
    }

    let package_id: Uuid = match package_row.try_get("id") {
        Ok(id) => id,
        Err(_) => return internal_error_response("Internal error"),
    };

    let latest_release = match sqlx::query(
        "SELECT rel.id, rel.version, rel.description, rel.is_prerelease, rel.provenance, rel.published_at, \
                a.filename, a.sha256 \
         FROM releases rel \
         LEFT JOIN LATERAL (\
             SELECT filename, sha256 \
             FROM artifacts \
             WHERE release_id = rel.id AND kind = 'gem' \
             ORDER BY uploaded_at DESC \
             LIMIT 1\
         ) a ON TRUE \
         WHERE rel.package_id = $1 \
           AND rel.status IN ('published', 'deprecated', 'yanked') \
         ORDER BY rel.published_at DESC, rel.version DESC \
         LIMIT 1",
    )
    .bind(package_id)
    .fetch_optional(state.db())
    .await
    {
        Ok(Some(row)) => row,
        Ok(None) => return not_found_response("Gem not found"),
        Err(_) => return internal_error_response("Database error"),
    };

    let name_value: String = package_row.try_get("name").unwrap_or(package_name);
    let provenance: Option<Value> = latest_release.try_get("provenance").ok().flatten();
    let authors = metadata_string_list(&provenance, &["authors", "author"]);
    let licenses = metadata_string_list(&provenance, &["licenses", "license"])
        .or_else(|| {
            package_row
                .try_get::<Option<String>, _>("license")
                .ok()
                .flatten()
                .map(|license| vec![license])
        })
        .unwrap_or_default();

    let filename: Option<String> = latest_release.try_get("filename").ok().flatten();
    let gem_uri = filename.as_ref().map(|filename| {
        format!(
            "{}/rubygems/gems/{}",
            state.base_url().trim_end_matches('/'),
            filename,
        )
    });

    let document = build_gem_metadata(&GemMetadataInput {
        name: name_value,
        version: latest_release.try_get("version").unwrap_or_default(),
        version_downloads: 0,
        total_downloads: package_row.try_get("download_count").unwrap_or(0_i64),
        platform: metadata_string(&provenance, &["platform"]).unwrap_or_else(|| "ruby".into()),
        authors: authors.unwrap_or_default(),
        info: latest_release
            .try_get::<Option<String>, _>("description")
            .ok()
            .flatten()
            .or_else(|| {
                package_row
                    .try_get::<Option<String>, _>("description")
                    .ok()
                    .flatten()
            }),
        licenses,
        project_uri: package_row.try_get("homepage").ok().flatten(),
        homepage_uri: package_row.try_get("homepage").ok().flatten(),
        source_code_uri: package_row.try_get("repository_url").ok().flatten(),
        bug_tracker_uri: metadata_string(&provenance, &["bug_tracker_uri"]),
        documentation_uri: metadata_string(&provenance, &["documentation_uri"]),
        sha: latest_release.try_get("sha256").ok().flatten(),
        gem_uri,
        version_created_at: latest_release
            .try_get("published_at")
            .unwrap_or_else(|_| Utc::now()),
        prerelease: latest_release.try_get("is_prerelease").unwrap_or(false),
        metadata: provenance
            .as_ref()
            .and_then(|value| value.get("metadata").cloned()),
    });

    (StatusCode::OK, Json(document)).into_response()
}

async fn gem_versions<S: RubyGemsAppState>(
    State(state): State<S>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Response {
    let identity = match authenticate_optional(&state, &headers).await {
        Ok(identity) => identity,
        Err(response) => return response,
    };

    let package_name = match name.strip_suffix(".json") {
        Some(name) => name.to_owned(),
        None => name,
    };
    if validate_rubygems_package_name(&package_name).is_err() {
        return not_found_response("Gem not found");
    }

    let actor_user_id = identity.as_ref().map(|identity| identity.user_id);
    let package_row = match load_package_row(state.db(), &package_name).await {
        Ok(row) => row,
        Err(response) => return response,
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
        return not_found_response("Gem not found");
    }

    let package_id: Uuid = match package_row.try_get("id") {
        Ok(id) => id,
        Err(_) => return internal_error_response("Internal error"),
    };

    let rows = match sqlx::query(
        "SELECT rel.version, rel.is_prerelease, rel.provenance, rel.published_at, a.filename, a.sha256 \
         FROM releases rel \
         LEFT JOIN LATERAL (\
             SELECT filename, sha256 \
             FROM artifacts \
             WHERE release_id = rel.id AND kind = 'gem' \
             ORDER BY uploaded_at DESC \
             LIMIT 1\
         ) a ON TRUE \
         WHERE rel.package_id = $1 \
           AND rel.status IN ('published', 'deprecated', 'yanked') \
         ORDER BY rel.published_at DESC, rel.version DESC",
    )
    .bind(package_id)
    .fetch_all(state.db())
    .await
    {
        Ok(rows) => rows,
        Err(_) => return internal_error_response("Database error"),
    };

    let items = rows
        .into_iter()
        .map(|row| {
            let provenance: Option<Value> = row.try_get("provenance").ok().flatten();
            let filename: Option<String> = row.try_get("filename").ok().flatten();
            GemVersionListItem {
                number: row.try_get("version").unwrap_or_default(),
                prerelease: row.try_get("is_prerelease").unwrap_or(false),
                created_at: row.try_get("published_at").unwrap_or_else(|_| Utc::now()),
                platform: metadata_string(&provenance, &["platform"])
                    .unwrap_or_else(|| "ruby".into()),
                sha: row.try_get("sha256").ok().flatten(),
                gem_uri: filename.map(|filename| {
                    format!(
                        "{}/rubygems/gems/{}",
                        state.base_url().trim_end_matches('/'),
                        filename,
                    )
                }),
            }
        })
        .collect::<Vec<_>>();

    (StatusCode::OK, Json(build_versions_list(&items))).into_response()
}

async fn download_gem<S: RubyGemsAppState>(
    State(state): State<S>,
    Path(filename): Path<String>,
    headers: HeaderMap,
) -> Response {
    let identity = match authenticate_optional(&state, &headers).await {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let actor_user_id = identity.as_ref().map(|identity| identity.user_id);

    let artifact_row = match sqlx::query(
        "SELECT a.storage_key, a.content_type, a.sha256, p.id AS package_id, \
                p.visibility AS package_visibility, p.owner_user_id AS package_owner_user_id, \
                p.owner_org_id AS package_owner_org_id, \
                r.visibility AS repository_visibility, r.owner_user_id AS repository_owner_user_id, \
                r.owner_org_id AS repository_owner_org_id \
         FROM artifacts a \
         JOIN releases rel ON rel.id = a.release_id \
         JOIN packages p ON p.id = rel.package_id \
         JOIN repositories r ON r.id = p.repository_id \
         WHERE a.filename = $1 \
           AND a.kind = 'gem' \
           AND rel.status IN ('published', 'deprecated', 'yanked') \
         ORDER BY rel.published_at DESC \
         LIMIT 1",
    )
    .bind(&filename)
    .fetch_optional(state.db())
    .await
    {
        Ok(Some(row)) => row,
        Ok(None) => return not_found_response("Gem file not found"),
        Err(_) => return internal_error_response("Database error"),
    };

    if !can_read_package(
        state.db(),
        &artifact_row
            .try_get::<String, _>("package_visibility")
            .unwrap_or_default(),
        &artifact_row
            .try_get::<String, _>("repository_visibility")
            .unwrap_or_default(),
        artifact_row
            .try_get("package_owner_user_id")
            .unwrap_or(None),
        artifact_row.try_get("package_owner_org_id").unwrap_or(None),
        artifact_row
            .try_get("repository_owner_user_id")
            .unwrap_or(None),
        artifact_row
            .try_get("repository_owner_org_id")
            .unwrap_or(None),
        actor_user_id,
    )
    .await
    {
        return not_found_response("Gem file not found");
    }

    let storage_key: String = artifact_row.try_get("storage_key").unwrap_or_default();
    let stored = match state.artifact_get(&storage_key).await {
        Ok(Some(stored)) => stored,
        Ok(None) => return not_found_response("Gem file not found"),
        Err(_) => return internal_error_response("Artifact storage error"),
    };

    let package_id: Uuid = artifact_row.try_get("package_id").unwrap_or(Uuid::nil());
    let _ = sqlx::query("UPDATE packages SET download_count = download_count + 1 WHERE id = $1")
        .bind(package_id)
        .execute(state.db())
        .await;

    let content_type = if stored.content_type.is_empty() {
        artifact_row
            .try_get::<String, _>("content_type")
            .unwrap_or_else(|_| "application/octet-stream".into())
    } else {
        stored.content_type
    };

    let sha256: String = artifact_row.try_get("sha256").unwrap_or_default();
    let disposition = format!("attachment; filename=\"{}\"", filename.replace('"', ""));

    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, content_type)
        .header(CONTENT_LENGTH, stored.bytes.len().to_string())
        .header(CONTENT_DISPOSITION, disposition)
        .header("x-checksum-sha256", sha256)
        .body(Body::from(stored.bytes))
        .unwrap_or_else(|_| internal_error_response("Internal error"))
}

async fn load_package_row(
    db: &PgPool,
    package_name: &str,
) -> Result<sqlx::postgres::PgRow, Response> {
    let normalized = normalize_package_name(package_name, &Ecosystem::Rubygems);
    sqlx::query(
        "SELECT p.id, p.name, p.description, p.homepage, p.repository_url, p.license, p.download_count, \
                p.visibility, p.owner_user_id, p.owner_org_id, \
                r.visibility AS repo_visibility, r.owner_user_id AS repo_owner_user_id, \
                r.owner_org_id AS repo_owner_org_id \
         FROM packages p \
         JOIN repositories r ON r.id = p.repository_id \
         WHERE p.ecosystem = 'rubygems' AND p.normalized_name = $1",
    )
    .bind(&normalized)
    .fetch_optional(db)
    .await
    .map_err(|_| internal_error_response("Database error"))?
    .ok_or_else(|| not_found_response("Gem not found"))
}

async fn authenticate_optional<S: RubyGemsAppState>(
    state: &S,
    headers: &HeaderMap,
) -> Result<Option<RubyGemsIdentity>, Response> {
    let token = extract_token(headers);
    let Some(token) = token else {
        return Ok(None);
    };
    authenticate_token(state, &token).await.map(Some)
}

fn extract_token(headers: &HeaderMap) -> Option<String> {
    if let Some(api_key) = headers
        .get("x-gem-api-key")
        .and_then(|value| value.to_str().ok())
    {
        let trimmed = api_key.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_owned());
        }
    }

    let authorization = headers.get(AUTHORIZATION)?.to_str().ok()?;
    if let Some(token) = authorization.strip_prefix("Bearer ") {
        let trimmed = token.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_owned());
        }
    }

    if let Some(encoded) = authorization.strip_prefix("Basic ") {
        let decoded = BASE64_STANDARD.decode(encoded.trim()).ok()?;
        let decoded = String::from_utf8(decoded).ok()?;
        let (username, password) = decoded.split_once(':').unwrap_or((decoded.as_str(), ""));
        if password.starts_with("pub_") {
            return Some(password.to_owned());
        }
        if username.starts_with("pub_") {
            return Some(username.to_owned());
        }
    }

    // `gem push` sends `Authorization: <api-key>` with no scheme prefix.
    // Accept that form for `pub_*` tokens.
    let trimmed = authorization.trim();
    if trimmed.starts_with("pub_") {
        return Some(trimmed.to_owned());
    }

    None
}

async fn authenticate_token<S: RubyGemsAppState>(
    state: &S,
    token: &str,
) -> Result<RubyGemsIdentity, Response> {
    if token.starts_with("pub_") {
        let token_hash = publaryn_core::security::hash_token(token);
        let row = sqlx::query(
            "SELECT id, user_id, scopes, expires_at, kind FROM tokens WHERE token_hash = $1 AND is_revoked = false",
        )
        .bind(&token_hash)
        .fetch_optional(state.db())
        .await
        .map_err(|_| internal_error_response("Database error"))?
        .ok_or_else(|| unauthorized_response("Invalid or revoked token"))?;

        let token_kind: String = row.try_get("kind").unwrap_or_default();
        if token_kind == "oidc_derived" {
            return Err(unauthorized_response(
                "OIDC-derived tokens are not valid for RubyGems access",
            ));
        }

        let expires_at = row
            .try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("expires_at")
            .unwrap_or(None);
        if expires_at.is_some_and(|expires_at| expires_at <= Utc::now()) {
            return Err(unauthorized_response("Token has expired"));
        }

        let user_id = row
            .try_get::<Option<Uuid>, _>("user_id")
            .unwrap_or(None)
            .ok_or_else(|| unauthorized_response("Token is not associated with a user"))?;
        let token_id: Option<Uuid> = row.try_get("id").ok();
        let scopes: Vec<String> = row.try_get("scopes").unwrap_or_default();

        // Update last_used_at (fire-and-forget).
        if let Some(tid) = token_id {
            let _ = sqlx::query("UPDATE tokens SET last_used_at = NOW() WHERE id = $1")
                .bind(tid)
                .execute(state.db())
                .await;
        }

        return Ok(RubyGemsIdentity {
            user_id,
            token_id,
            scopes,
        });
    }

    let claims = publaryn_auth::validate_token(token, state.jwt_secret(), state.jwt_issuer())
        .map_err(|_| unauthorized_response("Invalid or expired token"))?;
    let user_id =
        Uuid::parse_str(&claims.sub).map_err(|_| unauthorized_response("Invalid token subject"))?;
    let token_id = Uuid::parse_str(&claims.jti).ok();
    Ok(RubyGemsIdentity {
        user_id,
        token_id,
        scopes: claims.scopes,
    })
}

#[allow(clippy::too_many_arguments)]
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

fn metadata_string(metadata: &Option<Value>, keys: &[&str]) -> Option<String> {
    let value = metadata.as_ref()?;
    for key in keys {
        if let Some(text) = value.get(*key).and_then(|value| value.as_str()) {
            return Some(text.to_owned());
        }
    }
    None
}

fn metadata_string_list(metadata: &Option<Value>, keys: &[&str]) -> Option<Vec<String>> {
    let value = metadata.as_ref()?;
    for key in keys {
        if let Some(array) = value.get(*key).and_then(|value| value.as_array()) {
            let items = array
                .iter()
                .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                .collect::<Vec<_>>();
            if !items.is_empty() {
                return Some(items);
            }
        }
        if let Some(text) = value.get(*key).and_then(|value| value.as_str()) {
            return Some(
                text.split(',')
                    .map(str::trim)
                    .filter(|segment| !segment.is_empty())
                    .map(ToOwned::to_owned)
                    .collect(),
            );
        }
    }
    None
}

fn not_found_response(message: &str) -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorDocument { error: message }),
    )
        .into_response()
}

fn unauthorized_response(message: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(ErrorDocument { error: message }),
    )
        .into_response()
}

fn internal_error_response(message: &str) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorDocument { error: message }),
    )
        .into_response()
}

// ─── Publish: POST /api/v1/gems ─────────────────────────────────────────────

/// Maximum body size accepted for `gem push` (128 MiB).
const MAX_GEM_BODY_BYTES: usize = 128 * 1024 * 1024;

async fn push_gem<S: RubyGemsAppState>(
    State(state): State<S>,
    headers: HeaderMap,
    request: Request,
) -> Response {
    // Auth
    let identity = match authenticate_required(&state, &headers).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    if !has_scope(&identity, "packages:write") {
        return forbidden_response("API key does not have the packages:write scope");
    }

    // Read raw body
    let body_bytes = match to_bytes(request.into_body(), MAX_GEM_BODY_BYTES).await {
        Ok(b) => b,
        Err(_) => return bad_request_response("Failed to read request body"),
    };

    let parsed = match publish::parse_gem_push(body_bytes) {
        Ok(p) => p,
        Err(e) => return bad_request_response(&e.to_string()),
    };

    // Validate the gem name
    if let Err(e) = validate_rubygems_package_name(&parsed.metadata.name) {
        return bad_request_response(&e.to_string());
    }

    let normalized = normalize_package_name(&parsed.metadata.name, &Ecosystem::Rubygems);

    // Look up or create the package.
    let package_id = match resolve_or_create_package(&state, &identity, &parsed, &normalized).await
    {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    // Reject duplicate version
    let duplicate = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM releases WHERE package_id = $1 AND version = $2)",
    )
    .bind(package_id)
    .bind(&parsed.release_version)
    .fetch_one(state.db())
    .await
    .unwrap_or(false);

    if duplicate {
        return conflict_response(&format!(
            "Version {} of gem {} already exists",
            parsed.release_version, parsed.metadata.name
        ));
    }

    // Create release in quarantine
    let release = publish::make_release(package_id, &parsed, identity.user_id);
    let provenance = publish::build_provenance(&parsed.metadata);

    if sqlx::query(
        "INSERT INTO releases (id, package_id, version, status, published_by, description, \
         is_prerelease, is_yanked, is_deprecated, provenance, published_at, updated_at) \
         VALUES ($1, $2, $3, 'quarantine', $4, $5, $6, false, false, $7, $8, $9)",
    )
    .bind(release.id)
    .bind(release.package_id)
    .bind(&release.version)
    .bind(release.published_by)
    .bind(&release.description)
    .bind(release.is_prerelease)
    .bind(&provenance)
    .bind(release.published_at)
    .bind(release.updated_at)
    .execute(state.db())
    .await
    .is_err()
    {
        return internal_error_response("Failed to create release");
    }

    // Upload .gem to artifact storage
    let artifact = publish::make_artifact(release.id, &parsed);
    if state
        .artifact_put(
            artifact.storage_key.clone(),
            "application/octet-stream".into(),
            parsed.bytes.clone(),
        )
        .await
        .is_err()
    {
        let _ = sqlx::query("DELETE FROM releases WHERE id = $1")
            .bind(release.id)
            .execute(state.db())
            .await;
        return internal_error_response("Failed to store gem artifact");
    }

    if sqlx::query(
        "INSERT INTO artifacts (id, release_id, kind, filename, storage_key, content_type, \
         size_bytes, sha256, sha512, md5, is_signed, signature_key_id, uploaded_at) \
         VALUES ($1, $2, 'gem', $3, $4, $5, $6, $7, $8, NULL, false, NULL, $9) \
         ON CONFLICT (release_id, filename) DO NOTHING",
    )
    .bind(artifact.id)
    .bind(release.id)
    .bind(&artifact.filename)
    .bind(&artifact.storage_key)
    .bind(&artifact.content_type)
    .bind(artifact.size_bytes)
    .bind(&artifact.sha256)
    .bind(&parsed.sha512)
    .bind(artifact.uploaded_at)
    .execute(state.db())
    .await
    .is_err()
    {
        return internal_error_response("Failed to record artifact");
    }

    // RubyGems-specific release metadata.
    let runtime_deps = serde_json::to_value(&parsed.metadata.runtime_dependencies)
        .unwrap_or(serde_json::Value::Array(Vec::new()));
    let development_deps = serde_json::to_value(&parsed.metadata.development_dependencies)
        .unwrap_or(serde_json::Value::Array(Vec::new()));

    let _ = sqlx::query(
        "INSERT INTO rubygems_release_metadata \
         (release_id, platform, summary, authors, licenses, required_ruby_version, \
          required_rubygems_version, runtime_dependencies, development_dependencies) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
         ON CONFLICT (release_id) DO NOTHING",
    )
    .bind(release.id)
    .bind(&parsed.metadata.platform)
    .bind(&parsed.metadata.summary)
    .bind(&parsed.metadata.authors)
    .bind(&parsed.metadata.licenses)
    .bind(&parsed.metadata.required_ruby_version)
    .bind(&parsed.metadata.required_rubygems_version)
    .bind(&runtime_deps)
    .bind(&development_deps)
    .execute(state.db())
    .await;

    // Finalize to published
    if sqlx::query("UPDATE releases SET status = 'published', updated_at = NOW() WHERE id = $1")
        .bind(release.id)
        .execute(state.db())
        .await
        .is_err()
    {
        return internal_error_response("Failed to publish release");
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
        "ecosystem": "rubygems",
        "name": parsed.metadata.name,
        "version": parsed.release_version,
        "platform": parsed.metadata.platform,
        "source": "rubygems_push",
        "artifact_sha256": parsed.sha256,
    }))
    .execute(state.db())
    .await;

    // Bump search projection
    let _ = sqlx::query("UPDATE packages SET updated_at = NOW() WHERE id = $1")
        .bind(package_id)
        .execute(state.db())
        .await;

    (
        StatusCode::CREATED,
        format!(
            "Successfully registered gem: {} ({})",
            parsed.metadata.name, parsed.release_version
        ),
    )
        .into_response()
}

// ─── Yank: DELETE /api/v1/gems/yank ─────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct YankForm {
    gem_name: Option<String>,
    #[serde(alias = "gem_version")]
    version: Option<String>,
    platform: Option<String>,
}

async fn yank_gem<S: RubyGemsAppState>(
    State(state): State<S>,
    headers: HeaderMap,
    Form(form): Form<YankForm>,
) -> Response {
    let identity = match authenticate_required(&state, &headers).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    if !has_scope(&identity, "packages:write") {
        return forbidden_response("API key does not have the packages:write scope");
    }

    let name = match form.gem_name.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(name) => name.to_owned(),
        None => return bad_request_response("Missing required form field: gem_name"),
    };
    let version = match form.version.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(version) => version.to_owned(),
        None => return bad_request_response("Missing required form field: version"),
    };
    let platform = form
        .platform
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != "ruby")
        .map(str::to_owned);

    // Compute the storage-level version qualifier.
    let release_version = match &platform {
        Some(p) => format!("{version}-{p}"),
        None => version.clone(),
    };

    let normalized = normalize_package_name(&name, &Ecosystem::Rubygems);

    let pkg_row = sqlx::query(
        "SELECT id, owner_user_id, owner_org_id FROM packages \
         WHERE ecosystem = 'rubygems' AND normalized_name = $1",
    )
    .bind(&normalized)
    .fetch_optional(state.db())
    .await;

    let pkg_row = match pkg_row {
        Ok(Some(row)) => row,
        Ok(None) => return not_found_response("Gem not found"),
        Err(_) => return internal_error_response("Database error"),
    };

    let package_id: Uuid = pkg_row.try_get("id").unwrap_or_default();
    if !has_package_write_access(
        state.db(),
        package_id,
        pkg_row.try_get("owner_user_id").unwrap_or(None),
        pkg_row.try_get("owner_org_id").unwrap_or(None),
        identity.user_id,
    )
    .await
    {
        return forbidden_response("You do not have permission to yank this gem");
    }

    let updated = sqlx::query(
        "UPDATE releases \
         SET status = 'yanked', is_yanked = true, updated_at = NOW() \
         WHERE package_id = $1 AND version = $2",
    )
    .bind(package_id)
    .bind(&release_version)
    .execute(state.db())
    .await;

    match updated {
        Ok(res) if res.rows_affected() > 0 => {
            let _ = sqlx::query(
                "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, \
                 target_package_id, metadata, occurred_at) \
                 VALUES ($1, 'release_yank', $2, $3, $4, $5, NOW())",
            )
            .bind(Uuid::new_v4())
            .bind(identity.user_id)
            .bind(identity.token_id)
            .bind(package_id)
            .bind(serde_json::json!({
                "ecosystem": "rubygems",
                "name": name,
                "version": release_version,
                "source": "rubygems_yank",
            }))
            .execute(state.db())
            .await;
            (
                StatusCode::OK,
                format!("Successfully yanked gem: {name} ({release_version})"),
            )
                .into_response()
        }
        Ok(_) => not_found_response("Gem version not found"),
        Err(_) => internal_error_response("Database error"),
    }
}

// ─── POST /api/v1/api_key — compat echo ─────────────────────────────────────

async fn echo_api_key<S: RubyGemsAppState>(State(state): State<S>, headers: HeaderMap) -> Response {
    let identity = match authenticate_required(&state, &headers).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    let key = extract_token(&headers).unwrap_or_default();
    tracing::debug!(user_id = %identity.user_id, "api_key echo");
    let _ = state; // state used for auth only
    (StatusCode::OK, key).into_response()
}

// ─── Auth helpers for write endpoints ────────────────────────────────────────

async fn authenticate_required<S: RubyGemsAppState>(
    state: &S,
    headers: &HeaderMap,
) -> Result<RubyGemsIdentity, Response> {
    let token = extract_token(headers).ok_or_else(|| {
        unauthorized_response("Authentication required. Provide your Publaryn API key.")
    })?;
    authenticate_token(state, &token).await
}

fn has_scope(identity: &RubyGemsIdentity, needed: &str) -> bool {
    identity.scopes.iter().any(|s| s == needed)
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
        let direct = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM org_memberships \
             WHERE org_id = $1 AND user_id = $2 AND role::text = ANY($3))",
        )
        .bind(org_id)
        .bind(actor_user_id)
        .bind(&roles)
        .fetch_one(db)
        .await
        .unwrap_or(false);
        if direct {
            return true;
        }
        let perms: Vec<String> = vec!["admin".into(), "publish".into()];
        return sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS ( \
                 SELECT 1 FROM team_package_access tpa \
                 JOIN team_memberships tm ON tm.team_id = tpa.team_id \
                 JOIN teams t ON t.id = tpa.team_id \
                 JOIN packages p ON p.id = tpa.package_id \
                 WHERE tpa.package_id = $1 \
                   AND tm.user_id = $2 \
                   AND t.org_id = p.owner_org_id \
                   AND tpa.permission::text = ANY($3) \
             )",
        )
        .bind(package_id)
        .bind(actor_user_id)
        .bind(&perms)
        .fetch_one(db)
        .await
        .unwrap_or(false);
    }
    false
}

async fn resolve_or_create_package<S: RubyGemsAppState>(
    state: &S,
    identity: &RubyGemsIdentity,
    parsed: &ParsedGemPush,
    normalized: &str,
) -> Result<Uuid, Response> {
    let existing = sqlx::query(
        "SELECT id, owner_user_id, owner_org_id FROM packages \
         WHERE ecosystem = 'rubygems' AND normalized_name = $1",
    )
    .bind(normalized)
    .fetch_optional(state.db())
    .await;

    match existing {
        Ok(Some(row)) => {
            let pkg_id: Uuid = row.try_get("id").unwrap_or_default();
            if !has_package_write_access(
                state.db(),
                pkg_id,
                row.try_get("owner_user_id").unwrap_or(None),
                row.try_get("owner_org_id").unwrap_or(None),
                identity.user_id,
            )
            .await
            {
                return Err(forbidden_response(
                    "You do not have permission to publish to this gem",
                ));
            }

            // Update package metadata from latest push (non-destructive).
            let keywords: Vec<String> = parsed
                .metadata
                .metadata
                .get("keywords")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();

            let _ = sqlx::query(
                "UPDATE packages SET \
                   description = COALESCE($1, description), \
                   homepage    = COALESCE($2, homepage), \
                   license     = COALESCE($3, license), \
                   keywords    = CASE WHEN $4 THEN $5 ELSE keywords END, \
                   updated_at  = NOW() \
                 WHERE id = $6",
            )
            .bind(&parsed.metadata.description)
            .bind(&parsed.metadata.homepage)
            .bind(parsed.metadata.licenses.first())
            .bind(!keywords.is_empty())
            .bind(&keywords)
            .bind(pkg_id)
            .execute(state.db())
            .await;

            Ok(pkg_id)
        }
        Ok(None) => auto_create_package(state, identity, parsed).await,
        Err(_) => Err(internal_error_response("Database error")),
    }
}

async fn auto_create_package<S: RubyGemsAppState>(
    state: &S,
    identity: &RubyGemsIdentity,
    parsed: &ParsedGemPush,
) -> Result<Uuid, Response> {
    let repo = match publish::select_default_repository(state.db(), identity.user_id).await {
        Ok(repo) => repo,
        Err(Error::Forbidden(msg)) => return Err(forbidden_response(&msg)),
        Err(_) => return Err(internal_error_response("Failed to resolve repository")),
    };

    let normalized = normalize_package_name(&parsed.metadata.name, &Ecosystem::Rubygems);
    let keywords: Vec<String> = parsed
        .metadata
        .metadata
        .get("keywords")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let visibility = match repo.visibility.as_str() {
        "private" => "private",
        "unlisted" => "unlisted",
        _ => "public",
    };

    let pkg_id = Uuid::new_v4();
    let now = Utc::now();
    let license = parsed.metadata.licenses.first().cloned();
    let insert = sqlx::query(
        "INSERT INTO packages (id, repository_id, ecosystem, name, normalized_name, \
         description, homepage, license, keywords, visibility, owner_user_id, \
         is_deprecated, is_archived, download_count, created_at, updated_at) \
         VALUES ($1, $2, 'rubygems', $3, $4, $5, $6, $7, $8, $9::visibility, $10, \
                 false, false, 0, $11, $12)",
    )
    .bind(pkg_id)
    .bind(repo.id)
    .bind(&parsed.metadata.name)
    .bind(&normalized)
    .bind(&parsed.metadata.description)
    .bind(&parsed.metadata.homepage)
    .bind(&license)
    .bind(&keywords)
    .bind(visibility)
    .bind(identity.user_id)
    .bind(now)
    .bind(now)
    .execute(state.db())
    .await;

    match insert {
        Ok(_) => {
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
                "ecosystem": "rubygems",
                "name": parsed.metadata.name,
                "source": "rubygems_push",
            }))
            .execute(state.db())
            .await;
            Ok(pkg_id)
        }
        Err(sqlx::Error::Database(ref db_err)) if db_err.is_unique_violation() => {
            let row = sqlx::query(
                "SELECT id FROM packages WHERE ecosystem = 'rubygems' AND normalized_name = $1",
            )
            .bind(&normalized)
            .fetch_optional(state.db())
            .await
            .map_err(|_| internal_error_response("Failed to resolve package"))?
            .ok_or_else(|| internal_error_response("Failed to resolve package"))?;
            Ok(row.try_get("id").unwrap_or_default())
        }
        Err(_) => Err(internal_error_response("Failed to create package")),
    }
}

fn bad_request_response(message: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorDocument { error: message }),
    )
        .into_response()
}

fn forbidden_response(message: &str) -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(ErrorDocument { error: message }),
    )
        .into_response()
}

fn conflict_response(message: &str) -> Response {
    (
        StatusCode::CONFLICT,
        Json(ErrorDocument { error: message }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::name::normalize_rubygems_name;

    #[test]
    fn metadata_string_reads_value() {
        let metadata = serde_json::json!({ "platform": "ruby" });
        assert_eq!(
            metadata_string(&Some(metadata), &["platform"]),
            Some("ruby".into())
        );
    }

    #[test]
    fn metadata_string_list_reads_array() {
        let metadata = serde_json::json!({ "authors": ["A", "B"] });
        assert_eq!(
            metadata_string_list(&Some(metadata), &["authors"]),
            Some(vec!["A".into(), "B".into()])
        );
    }

    #[test]
    fn normalization_matches_core_behavior() {
        assert_eq!(normalize_rubygems_name("demo-gem"), "demo_gem");
    }
}
