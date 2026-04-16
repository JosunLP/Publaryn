//! Axum route handlers for a minimal Maven repository read surface.
//!
//! Supported GET operations:
//! - `.../maven-metadata.xml`
//! - `.../*.sha256`
//! - artifact download (e.g. `.jar`, `.pom`)

use axum::{
    body::Body,
    extract::{Path, State},
    http::{
        header::{AUTHORIZATION, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE},
        HeaderMap, StatusCode,
    },
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use bytes::Bytes;
use chrono::Utc;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use publaryn_core::{
    domain::{namespace::Ecosystem, package::normalize_package_name},
    error::Error,
};

use crate::{
    metadata::{build_maven_metadata_xml, MavenMetadataInput},
    name::{package_name, parse_artifact_path, parse_metadata_path},
};

pub trait MavenAppState: Clone + Send + Sync + 'static {
    fn db(&self) -> &PgPool;
    fn artifact_get(
        &self,
        key: &str,
    ) -> impl std::future::Future<Output = Result<Option<StoredObject>, Error>> + Send;
    fn jwt_secret(&self) -> &str;
    fn jwt_issuer(&self) -> &str;
}

#[derive(Debug, Clone)]
pub struct StoredObject {
    pub content_type: String,
    pub bytes: Bytes,
}

#[derive(Debug, Clone)]
struct MavenIdentity {
    user_id: Uuid,
}

pub fn router<S: MavenAppState>() -> Router<S> {
    Router::new().route("/*path", get(repository_get::<S>))
}

async fn repository_get<S: MavenAppState>(
    State(state): State<S>,
    Path(path): Path<String>,
    headers: HeaderMap,
) -> Response {
    if path.ends_with("/maven-metadata.xml") {
        return metadata_get(state, path, headers).await;
    }

    if let Some(stripped) = path.strip_suffix(".sha256") {
        return checksum_get(state, stripped.to_owned(), headers).await;
    }

    artifact_get(state, path, headers).await
}

async fn metadata_get<S: MavenAppState>(state: S, path: String, headers: HeaderMap) -> Response {
    let identity = match authenticate_optional(&state, &headers).await {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let actor_user_id = identity.as_ref().map(|identity| identity.user_id);

    let (group_id, artifact_id) = match parse_metadata_path(&path) {
        Ok(parsed) => parsed,
        Err(_) => return not_found_response("Artifact metadata not found"),
    };
    let package_name = match package_name(&group_id, &artifact_id) {
        Ok(name) => name,
        Err(_) => return not_found_response("Artifact metadata not found"),
    };

    let package_row = match load_package_row(state.db(), &package_name).await {
        Ok(row) => row,
        Err(response) => return response,
    };

    if !package_readable(&package_row, state.db(), actor_user_id).await {
        return not_found_response("Artifact metadata not found");
    }

    let package_id: Uuid = match package_row.try_get("id") {
        Ok(id) => id,
        Err(_) => return internal_error_response("Internal error"),
    };

    let release_rows = match sqlx::query(
        "SELECT version, published_at \
         FROM releases \
         WHERE package_id = $1 AND status IN ('published', 'deprecated', 'yanked') \
         ORDER BY published_at DESC, version DESC",
    )
    .bind(package_id)
    .fetch_all(state.db())
    .await
    {
        Ok(rows) => rows,
        Err(_) => return internal_error_response("Database error"),
    };

    if release_rows.is_empty() {
        return not_found_response("Artifact metadata not found");
    }

    let versions = release_rows
        .iter()
        .filter_map(|row| row.try_get::<String, _>("version").ok())
        .collect::<Vec<_>>();
    let latest = versions.first().cloned();
    let last_updated = release_rows
        .first()
        .and_then(|row| row.try_get("published_at").ok())
        .unwrap_or_else(Utc::now);

    let xml = build_maven_metadata_xml(&MavenMetadataInput {
        group_id,
        artifact_id,
        latest: latest.clone(),
        release: latest,
        versions,
        last_updated,
    });

    (
        StatusCode::OK,
        [(CONTENT_TYPE, "application/xml; charset=utf-8")],
        xml,
    )
        .into_response()
}

async fn checksum_get<S: MavenAppState>(state: S, path: String, headers: HeaderMap) -> Response {
    let identity = match authenticate_optional(&state, &headers).await {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let actor_user_id = identity.as_ref().map(|identity| identity.user_id);

    let (group_id, artifact_id, version, filename) = match parse_artifact_path(&path) {
        Ok(parsed) => parsed,
        Err(_) => return not_found_response("Checksum not found"),
    };

    let package_name = match package_name(&group_id, &artifact_id) {
        Ok(name) => name,
        Err(_) => return not_found_response("Checksum not found"),
    };

    let artifact_row = match load_artifact_row(state.db(), &package_name, &version, &filename).await
    {
        Ok(row) => row,
        Err(response) => return response,
    };

    if !artifact_readable(&artifact_row, state.db(), actor_user_id).await {
        return not_found_response("Checksum not found");
    }

    let sha256: String = artifact_row.try_get("sha256").unwrap_or_default();
    (
        StatusCode::OK,
        [(CONTENT_TYPE, "text/plain; charset=utf-8")],
        format!("{sha256}\n"),
    )
        .into_response()
}

async fn artifact_get<S: MavenAppState>(state: S, path: String, headers: HeaderMap) -> Response {
    let identity = match authenticate_optional(&state, &headers).await {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let actor_user_id = identity.as_ref().map(|identity| identity.user_id);

    let (group_id, artifact_id, version, filename) = match parse_artifact_path(&path) {
        Ok(parsed) => parsed,
        Err(_) => return not_found_response("Artifact not found"),
    };

    let package_name = match package_name(&group_id, &artifact_id) {
        Ok(name) => name,
        Err(_) => return not_found_response("Artifact not found"),
    };

    let artifact_row = match load_artifact_row(state.db(), &package_name, &version, &filename).await
    {
        Ok(row) => row,
        Err(response) => return response,
    };

    if !artifact_readable(&artifact_row, state.db(), actor_user_id).await {
        return not_found_response("Artifact not found");
    }

    let storage_key: String = artifact_row.try_get("storage_key").unwrap_or_default();
    let stored = match state.artifact_get(&storage_key).await {
        Ok(Some(stored)) => stored,
        Ok(None) => return not_found_response("Artifact not found"),
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
    let disposition = format!("attachment; filename=\"{}\"", filename.replace('"', ""));

    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, content_type)
        .header(CONTENT_LENGTH, stored.bytes.len().to_string())
        .header(CONTENT_DISPOSITION, disposition)
        .body(Body::from(stored.bytes))
        .unwrap_or_else(|_| internal_error_response("Internal error"))
}

async fn load_package_row(
    db: &PgPool,
    package_name: &str,
) -> Result<sqlx::postgres::PgRow, Response> {
    let normalized = normalize_package_name(package_name, &Ecosystem::Maven);
    sqlx::query(
        "SELECT p.id, p.visibility, p.owner_user_id, p.owner_org_id, \
                r.visibility AS repo_visibility, r.owner_user_id AS repo_owner_user_id, \
                r.owner_org_id AS repo_owner_org_id \
         FROM packages p \
         JOIN repositories r ON r.id = p.repository_id \
         WHERE p.ecosystem = 'maven' AND p.normalized_name = $1",
    )
    .bind(&normalized)
    .fetch_optional(db)
    .await
    .map_err(|_| internal_error_response("Database error"))?
    .ok_or_else(|| not_found_response("Artifact not found"))
}

async fn load_artifact_row(
    db: &PgPool,
    package_name: &str,
    version: &str,
    filename: &str,
) -> Result<sqlx::postgres::PgRow, Response> {
    let normalized = normalize_package_name(package_name, &Ecosystem::Maven);
    sqlx::query(
        "SELECT a.storage_key, a.content_type, a.sha256, p.id AS package_id, \
                p.visibility AS package_visibility, p.owner_user_id AS package_owner_user_id, \
                p.owner_org_id AS package_owner_org_id, \
                r.visibility AS repo_visibility, r.owner_user_id AS repo_owner_user_id, \
                r.owner_org_id AS repo_owner_org_id \
         FROM artifacts a \
         JOIN releases rel ON rel.id = a.release_id \
         JOIN packages p ON p.id = rel.package_id \
         JOIN repositories r ON r.id = p.repository_id \
         WHERE p.ecosystem = 'maven' AND p.normalized_name = $1 \
           AND rel.version = $2 \
           AND a.filename = $3 \
           AND rel.status IN ('published', 'deprecated', 'yanked') \
         ORDER BY rel.published_at DESC \
         LIMIT 1",
    )
    .bind(&normalized)
    .bind(version)
    .bind(filename)
    .fetch_optional(db)
    .await
    .map_err(|_| internal_error_response("Database error"))?
    .ok_or_else(|| not_found_response("Artifact not found"))
}

async fn authenticate_optional<S: MavenAppState>(
    state: &S,
    headers: &HeaderMap,
) -> Result<Option<MavenIdentity>, Response> {
    let token = extract_token(headers);
    let Some(token) = token else {
        return Ok(None);
    };
    authenticate_token(state, &token).await.map(Some)
}

fn extract_token(headers: &HeaderMap) -> Option<String> {
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

    None
}

async fn authenticate_token<S: MavenAppState>(
    state: &S,
    token: &str,
) -> Result<MavenIdentity, Response> {
    if token.starts_with("pub_") {
        let token_hash = publaryn_core::security::hash_token(token);
        let row = sqlx::query(
            "SELECT user_id, expires_at, kind FROM tokens WHERE token_hash = $1 AND is_revoked = false",
        )
        .bind(&token_hash)
        .fetch_optional(state.db())
        .await
        .map_err(|_| internal_error_response("Database error"))?
        .ok_or_else(|| unauthorized_response("Invalid or revoked token"))?;

        let token_kind: String = row.try_get("kind").unwrap_or_default();
        if token_kind == "oidc_derived" {
            return Err(unauthorized_response(
                "OIDC-derived tokens are not valid for Maven access",
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

        return Ok(MavenIdentity { user_id });
    }

    let claims = publaryn_auth::validate_token(token, state.jwt_secret(), state.jwt_issuer())
        .map_err(|_| unauthorized_response("Invalid or expired token"))?;
    let user_id =
        Uuid::parse_str(&claims.sub).map_err(|_| unauthorized_response("Invalid token subject"))?;
    Ok(MavenIdentity { user_id })
}

async fn package_readable(
    package_row: &sqlx::postgres::PgRow,
    db: &PgPool,
    actor_user_id: Option<Uuid>,
) -> bool {
    can_read_package(
        db,
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
}

async fn artifact_readable(
    artifact_row: &sqlx::postgres::PgRow,
    db: &PgPool,
    actor_user_id: Option<Uuid>,
) -> bool {
    can_read_package(
        db,
        &artifact_row
            .try_get::<String, _>("package_visibility")
            .unwrap_or_default(),
        &artifact_row
            .try_get::<String, _>("repo_visibility")
            .unwrap_or_default(),
        artifact_row
            .try_get("package_owner_user_id")
            .unwrap_or(None),
        artifact_row.try_get("package_owner_org_id").unwrap_or(None),
        artifact_row.try_get("repo_owner_user_id").unwrap_or(None),
        artifact_row.try_get("repo_owner_org_id").unwrap_or(None),
        actor_user_id,
    )
    .await
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

fn not_found_response(message: &str) -> Response {
    (StatusCode::NOT_FOUND, message.to_owned()).into_response()
}

fn unauthorized_response(message: &str) -> Response {
    (StatusCode::UNAUTHORIZED, message.to_owned()).into_response()
}

fn internal_error_response(message: &str) -> Response {
    (StatusCode::INTERNAL_SERVER_ERROR, message.to_owned()).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sha256_route_uses_artifact_parser() {
        let parsed = parse_artifact_path("com/example/demo/1.0.0/demo-1.0.0.jar").unwrap();
        assert_eq!(parsed.1, "demo");
        assert_eq!(parsed.2, "1.0.0");
    }
}
