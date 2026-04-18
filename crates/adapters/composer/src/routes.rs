//! Axum route handlers for the Composer package metadata surface.
//!
//! This MVP implements a read-focused Packagist-style API:
//! - `GET /packages.json`
//! - `GET /p/{vendor}/{package}.json`
//! - `GET /files/{artifact_id}/{filename}`
//!
//! Private package reads support optional Bearer or HTTP Basic authentication
//! using Publaryn API tokens / JWTs.

use axum::{
    body::Body,
    extract::{Path, State},
    http::{
        header::{AUTHORIZATION, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE},
        HeaderMap, StatusCode,
    },
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use bytes::Bytes;
use chrono::Utc;
use serde::Serialize;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use publaryn_core::{
    domain::{namespace::Ecosystem, package::normalize_package_name},
    error::Error,
};

use crate::{
    metadata::{
        build_package_metadata, build_packages_index, ComposerPackageInput, ComposerVersionInput,
        PackagesIndexInput,
    },
    name::{build_composer_package_name, normalize_composer_version},
};

pub trait ComposerAppState: Clone + Send + Sync + 'static {
    fn db(&self) -> &PgPool;
    fn artifact_get(
        &self,
        key: &str,
    ) -> impl std::future::Future<Output = Result<Option<StoredObject>, Error>> + Send;
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
struct ComposerIdentity {
    user_id: Uuid,
}

#[derive(Debug, Clone, Serialize)]
struct ComposerErrorDocument<'a> {
    error: &'a str,
}

pub fn router<S: ComposerAppState>() -> Router<S> {
    Router::new()
        .route("/packages.json", get(packages_index::<S>))
        .route("/p/{vendor}/{package}", get(package_metadata::<S>))
        .route(
            "/files/{artifact_id}/{filename}",
            get(download_distribution::<S>),
        )
}

async fn packages_index<S: ComposerAppState>(
    State(state): State<S>,
    headers: HeaderMap,
) -> Response {
    let identity = match authenticate_optional(&state, &headers).await {
        Ok(identity) => identity,
        Err(response) => return response,
    };

    let actor_user_id = identity.as_ref().map(|identity| identity.user_id);
    let package_names = match load_visible_packages(state.db(), actor_user_id).await {
        Ok(names) => names,
        Err(response) => return response,
    };

    let document = build_packages_index(&PackagesIndexInput { package_names }, state.base_url());

    (StatusCode::OK, Json(document)).into_response()
}

async fn package_metadata<S: ComposerAppState>(
    State(state): State<S>,
    Path((vendor, package)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let identity = match authenticate_optional(&state, &headers).await {
        Ok(identity) => identity,
        Err(response) => return response,
    };

    let package_name = match build_composer_package_name(&vendor, &package) {
        Ok(name) => name,
        Err(_) => return composer_error_response(StatusCode::NOT_FOUND, "Package not found"),
    };
    let normalized_name = normalize_package_name(&package_name, &Ecosystem::Composer);
    let actor_user_id = identity.as_ref().map(|identity| identity.user_id);

    let package_row = match sqlx::query(
        "SELECT p.id, p.name, p.description, p.homepage, p.repository_url, p.license, p.keywords, \
                p.visibility, p.owner_user_id, p.owner_org_id, \
                r.visibility AS repo_visibility, r.owner_user_id AS repo_owner_user_id, \
                r.owner_org_id AS repo_owner_org_id \
         FROM packages p \
         JOIN repositories r ON r.id = p.repository_id \
         WHERE p.ecosystem = 'composer' AND p.normalized_name = $1",
    )
    .bind(&normalized_name)
    .fetch_optional(state.db())
    .await
    {
        Ok(Some(row)) => row,
        Ok(None) => return composer_error_response(StatusCode::NOT_FOUND, "Package not found"),
        Err(_) => {
            return composer_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error")
        }
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
        return composer_error_response(StatusCode::NOT_FOUND, "Package not found");
    }

    let package_id: Uuid = match package_row.try_get("id") {
        Ok(id) => id,
        Err(_) => {
            return composer_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Internal error")
        }
    };

    let release_rows = match sqlx::query(
        "SELECT rel.version, rel.description, rel.is_deprecated, rel.deprecation_message, \
                rel.provenance, rel.published_at, \
                art.id AS artifact_id, art.filename, art.sha256 \
         FROM releases rel \
         LEFT JOIN LATERAL (\
             SELECT id, filename, sha256 \
             FROM artifacts \
             WHERE release_id = rel.id AND kind IN ('composer_zip', 'source_zip') \
             ORDER BY uploaded_at DESC \
             LIMIT 1\
         ) art ON TRUE \
         WHERE rel.package_id = $1 \
           AND rel.status IN ('published', 'deprecated', 'yanked') \
         ORDER BY rel.published_at DESC, rel.version DESC",
    )
    .bind(package_id)
    .fetch_all(state.db())
    .await
    {
        Ok(rows) => rows,
        Err(_) => {
            return composer_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error")
        }
    };

    let versions = release_rows
        .into_iter()
        .map(|row| {
            let artifact_id = row.try_get::<Option<Uuid>, _>("artifact_id").ok().flatten();
            let filename = row.try_get::<Option<String>, _>("filename").ok().flatten();
            let dist_url = artifact_id.zip(filename).map(|(artifact_id, filename)| {
                format!(
                    "{}/composer/files/{}/{}",
                    state.base_url().trim_end_matches('/'),
                    artifact_id,
                    filename,
                )
            });
            let licenses = package_row
                .try_get::<Option<String>, _>("license")
                .ok()
                .flatten()
                .map(|license| vec![license])
                .unwrap_or_default();

            ComposerVersionInput {
                version: row.try_get("version").unwrap_or_default(),
                version_normalized: normalize_composer_version(
                    &row.try_get::<String, _>("version").unwrap_or_default(),
                ),
                description: row
                    .try_get::<Option<String>, _>("description")
                    .ok()
                    .flatten()
                    .or_else(|| {
                        package_row
                            .try_get::<Option<String>, _>("description")
                            .ok()
                            .flatten()
                    }),
                homepage: package_row.try_get("homepage").ok().flatten(),
                repository_url: package_row.try_get("repository_url").ok().flatten(),
                licenses,
                keywords: package_row
                    .try_get::<Vec<String>, _>("keywords")
                    .unwrap_or_default(),
                dist_url,
                dist_reference: row.try_get::<Option<String>, _>("sha256").ok().flatten(),
                published_at: row.try_get("published_at").unwrap_or_else(|_| Utc::now()),
                extra_metadata: row.try_get("provenance").ok().flatten(),
            }
        })
        .collect::<Vec<_>>();

    let document = build_package_metadata(&ComposerPackageInput {
        name: package_row.try_get("name").unwrap_or(package_name),
        description: package_row.try_get("description").ok().flatten(),
        homepage: package_row.try_get("homepage").ok().flatten(),
        repository_url: package_row.try_get("repository_url").ok().flatten(),
        licenses: package_row
            .try_get::<Option<String>, _>("license")
            .ok()
            .flatten()
            .map(|license| vec![license])
            .unwrap_or_default(),
        keywords: package_row
            .try_get::<Vec<String>, _>("keywords")
            .unwrap_or_default(),
        versions,
    });

    (StatusCode::OK, Json(document)).into_response()
}

async fn download_distribution<S: ComposerAppState>(
    State(state): State<S>,
    Path((artifact_id, filename)): Path<(Uuid, String)>,
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
         WHERE a.id = $1 \
           AND a.filename = $2 \
           AND a.kind IN ('composer_zip', 'source_zip') \
           AND rel.status IN ('published', 'deprecated', 'yanked')",
    )
    .bind(artifact_id)
    .bind(&filename)
    .fetch_optional(state.db())
    .await
    {
        Ok(Some(row)) => row,
        Ok(None) => {
            return composer_error_response(
                StatusCode::NOT_FOUND,
                "Distribution file not found",
            )
        }
        Err(_) => {
            return composer_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Database error",
            )
        }
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
        return composer_error_response(StatusCode::NOT_FOUND, "Distribution file not found");
    }

    let storage_key: String = artifact_row.try_get("storage_key").unwrap_or_default();
    let stored = match state.artifact_get(&storage_key).await {
        Ok(Some(stored)) => stored,
        Ok(None) => {
            return composer_error_response(StatusCode::NOT_FOUND, "Distribution file not found")
        }
        Err(_) => {
            return composer_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Artifact storage error",
            )
        }
    };

    let package_id: Uuid = artifact_row.try_get("package_id").unwrap_or(Uuid::nil());
    let _ = sqlx::query("UPDATE packages SET download_count = download_count + 1 WHERE id = $1")
        .bind(package_id)
        .execute(state.db())
        .await;

    let content_type = if stored.content_type.is_empty() {
        artifact_row
            .try_get::<String, _>("content_type")
            .unwrap_or_else(|_| "application/zip".into())
    } else {
        stored.content_type
    };

    let disposition = format!("attachment; filename=\"{}\"", filename.replace('"', ""));
    let sha256: String = artifact_row.try_get("sha256").unwrap_or_default();

    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, content_type)
        .header(CONTENT_LENGTH, stored.bytes.len().to_string())
        .header(CONTENT_DISPOSITION, disposition)
        .header("x-checksum-sha256", sha256)
        .body(Body::from(stored.bytes))
        .unwrap_or_else(|_| {
            composer_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Internal error")
        })
}

async fn load_visible_packages(
    db: &PgPool,
    actor_user_id: Option<Uuid>,
) -> Result<Vec<String>, Response> {
    let rows = sqlx::query(
        "SELECT DISTINCT p.name, p.visibility AS package_visibility, p.owner_user_id AS package_owner_user_id, \
                p.owner_org_id AS package_owner_org_id, r.visibility AS repository_visibility, \
                r.owner_user_id AS repository_owner_user_id, r.owner_org_id AS repository_owner_org_id \
         FROM packages p \
         JOIN repositories r ON r.id = p.repository_id \
         WHERE p.ecosystem = 'composer' \
         ORDER BY LOWER(p.name) ASC",
    )
    .fetch_all(db)
    .await
    .map_err(|_| composer_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error"))?;

    let mut visible = Vec::new();
    for row in rows {
        if can_read_package(
            db,
            &row.try_get::<String, _>("package_visibility")
                .unwrap_or_default(),
            &row.try_get::<String, _>("repository_visibility")
                .unwrap_or_default(),
            row.try_get("package_owner_user_id").unwrap_or(None),
            row.try_get("package_owner_org_id").unwrap_or(None),
            row.try_get("repository_owner_user_id").unwrap_or(None),
            row.try_get("repository_owner_org_id").unwrap_or(None),
            actor_user_id,
        )
        .await
        {
            if let Ok(name) = row.try_get::<String, _>("name") {
                visible.push(name);
            }
        }
    }

    Ok(visible)
}

async fn authenticate_optional<S: ComposerAppState>(
    state: &S,
    headers: &HeaderMap,
) -> Result<Option<ComposerIdentity>, Response> {
    let Some(header_value) = headers.get(AUTHORIZATION) else {
        return Ok(None);
    };

    let authorization = header_value.to_str().map_err(|_| {
        composer_error_response(StatusCode::UNAUTHORIZED, "Invalid Authorization header")
    })?;

    if let Some(token) = authorization.strip_prefix("Bearer ") {
        return authenticate_token(state, token.trim()).await.map(Some);
    }

    if let Some(encoded) = authorization.strip_prefix("Basic ") {
        let decoded = BASE64_STANDARD.decode(encoded.trim()).map_err(|_| {
            composer_error_response(StatusCode::UNAUTHORIZED, "Invalid Basic credentials")
        })?;
        let decoded = String::from_utf8(decoded).map_err(|_| {
            composer_error_response(StatusCode::UNAUTHORIZED, "Invalid Basic credentials")
        })?;
        let (username, password) = decoded.split_once(':').unwrap_or((decoded.as_str(), ""));
        let token = if password.starts_with("pub_") {
            password
        } else if username.starts_with("pub_") {
            username
        } else {
            return Err(composer_error_response(
                StatusCode::UNAUTHORIZED,
                "Basic authentication must provide a Publaryn API token",
            ));
        };
        return authenticate_token(state, token).await.map(Some);
    }

    Err(composer_error_response(
        StatusCode::UNAUTHORIZED,
        "Unsupported authorization scheme",
    ))
}

async fn authenticate_token<S: ComposerAppState>(
    state: &S,
    token: &str,
) -> Result<ComposerIdentity, Response> {
    if token.is_empty() {
        return Err(composer_error_response(
            StatusCode::UNAUTHORIZED,
            "Authentication token must not be empty",
        ));
    }

    if token.starts_with("pub_") {
        let token_hash = publaryn_core::security::hash_token(token);
        let row = sqlx::query(
            "SELECT user_id, expires_at, is_revoked, kind FROM tokens WHERE token_hash = $1",
        )
        .bind(&token_hash)
        .fetch_optional(state.db())
        .await
        .map_err(|_| composer_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error"))?
        .ok_or_else(|| {
            composer_error_response(StatusCode::UNAUTHORIZED, "Invalid or revoked token")
        })?;

        let revoked = row.try_get::<bool, _>("is_revoked").unwrap_or(false);
        if revoked {
            return Err(composer_error_response(
                StatusCode::UNAUTHORIZED,
                "Invalid or revoked token",
            ));
        }

        let token_kind: String = row.try_get("kind").unwrap_or_default();
        if token_kind == "oidc_derived" {
            return Err(composer_error_response(
                StatusCode::UNAUTHORIZED,
                "OIDC-derived tokens are not valid for Composer access",
            ));
        }

        let expires_at = row
            .try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("expires_at")
            .unwrap_or(None);
        if expires_at.is_some_and(|expires_at| expires_at <= Utc::now()) {
            return Err(composer_error_response(
                StatusCode::UNAUTHORIZED,
                "Token has expired",
            ));
        }

        let user_id = row
            .try_get::<Option<Uuid>, _>("user_id")
            .unwrap_or(None)
            .ok_or_else(|| {
                composer_error_response(
                    StatusCode::UNAUTHORIZED,
                    "Token is not associated with a user",
                )
            })?;

        return Ok(ComposerIdentity { user_id });
    }

    let claims = publaryn_auth::validate_token(token, state.jwt_secret(), state.jwt_issuer())
        .map_err(|_| {
            composer_error_response(StatusCode::UNAUTHORIZED, "Invalid or expired token")
        })?;

    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| composer_error_response(StatusCode::UNAUTHORIZED, "Invalid token subject"))?;

    Ok(ComposerIdentity { user_id })
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

fn composer_error_response(status: StatusCode, message: &str) -> Response {
    (status, Json(ComposerErrorDocument { error: message })).into_response()
}
