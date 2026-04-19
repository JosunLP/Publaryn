//! Axum route handlers for the Composer package metadata surface.
//!
//! This adapter implements a Packagist-style read + publish API:
//! - `GET /packages.json`
//! - `GET /p/{vendor}/{package}.json`
//! - `GET /files/{artifact_id}/{filename}`
//! - `PUT /packages/{vendor}/{package}`
//! - `DELETE /packages/{vendor}/{package}/versions/{version}`
//!
//! Private package reads support optional Bearer or HTTP Basic authentication
//! using Publaryn API tokens / JWTs.

use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Path, State},
    http::{
        header::{AUTHORIZATION, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE},
        HeaderMap, StatusCode,
    },
    response::{IntoResponse, Response},
    routing::{delete, get, put},
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
    name::{
        build_composer_package_name, normalize_composer_version, validate_composer_package_name,
    },
    publish::{self, ParsedComposerPublish},
};

pub trait ComposerAppState: Clone + Send + Sync + 'static {
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
}

#[derive(Debug, Clone)]
pub struct StoredObject {
    pub content_type: String,
    pub bytes: Bytes,
}

#[derive(Debug, Clone)]
struct ComposerIdentity {
    user_id: Uuid,
    token_id: Option<Uuid>,
    scopes: Vec<String>,
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
        .route(
            "/packages/{vendor}/{package}",
            put(publish_package::<S>).layer(DefaultBodyLimit::disable()),
        )
        .route(
            "/packages/{vendor}/{package}/versions/{version}",
            delete(yank_package_version::<S>),
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
                  r.visibility::text AS repo_visibility, r.owner_user_id AS repo_owner_user_id, \
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
                     AND rel.status IN ('published', 'deprecated') \
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
                  r.visibility::text AS repository_visibility, r.owner_user_id AS repository_owner_user_id, \
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

async fn publish_package<S: ComposerAppState>(
    State(state): State<S>,
    Path((vendor, package)): Path<(String, String)>,
    headers: HeaderMap,
    multipart: Multipart,
) -> Response {
    let expected_name = format!("{vendor}/{package}");
    if let Err(error) = validate_composer_package_name(&expected_name) {
        return composer_error_response(StatusCode::BAD_REQUEST, &error.to_string());
    }

    let identity = match authenticate_required(&state, &headers).await {
        Ok(identity) => identity,
        Err(response) => return response,
    };

    if !has_scope(&identity, "packages:write") {
        return composer_error_response(
            StatusCode::FORBIDDEN,
            "Token does not have the packages:write scope",
        );
    }

    let parsed = match parse_publish_multipart(&expected_name, multipart).await {
        Ok(parsed) => parsed,
        Err(response) => return response,
    };

    let package_id = match resolve_or_create_package(&state, &identity, &parsed).await {
        Ok(package_id) => package_id,
        Err(response) => return response,
    };

    let existing_release =
        sqlx::query("SELECT id FROM releases WHERE package_id = $1 AND version = $2")
            .bind(package_id)
            .bind(&parsed.version)
            .fetch_optional(state.db())
            .await;

    if matches!(existing_release, Ok(Some(_))) {
        return composer_error_response(
            StatusCode::CONFLICT,
            &format!(
                "Version {} of package {} already exists",
                parsed.version, parsed.name
            ),
        );
    }

    let release = publish::make_release(package_id, &parsed, identity.user_id);
    let insert_release = sqlx::query(
        "INSERT INTO releases (id, package_id, version, status, published_by, description, \
         changelog, is_prerelease, is_yanked, yank_reason, is_deprecated, deprecation_message, \
         source_ref, provenance, published_at, updated_at) \
         VALUES ($1, $2, $3, 'quarantine', $4, $5, NULL, $6, false, NULL, false, NULL, NULL, $7, $8, $9)",
    )
    .bind(release.id)
    .bind(package_id)
    .bind(&release.version)
    .bind(release.published_by)
    .bind(&release.description)
    .bind(release.is_prerelease)
    .bind(&release.provenance)
    .bind(release.published_at)
    .bind(release.updated_at)
    .execute(state.db())
    .await;

    match insert_release {
        Ok(_) => {}
        Err(sqlx::Error::Database(db_error)) if db_error.is_unique_violation() => {
            return composer_error_response(
                StatusCode::CONFLICT,
                &format!(
                    "Version {} of package {} already exists",
                    parsed.version, parsed.name
                ),
            )
        }
        Err(_) => {
            return composer_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to create release",
            )
        }
    }

    let artifact = publish::make_artifact(release.id, &parsed);
    if state
        .artifact_put(
            artifact.storage_key.clone(),
            artifact.content_type.clone(),
            parsed.zip_bytes.clone(),
        )
        .await
        .is_err()
    {
        let _ = sqlx::query("DELETE FROM releases WHERE id = $1")
            .bind(release.id)
            .execute(state.db())
            .await;
        return composer_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to store Composer distribution",
        );
    }

    if sqlx::query(
        "INSERT INTO artifacts (id, release_id, kind, filename, storage_key, content_type, \
         size_bytes, sha256, sha512, md5, is_signed, signature_key_id, uploaded_at) \
         VALUES ($1, $2, 'composer_zip', $3, $4, $5, $6, $7, $8, NULL, false, NULL, $9) \
         ON CONFLICT (release_id, filename) DO NOTHING",
    )
    .bind(artifact.id)
    .bind(release.id)
    .bind(&artifact.filename)
    .bind(&artifact.storage_key)
    .bind(&artifact.content_type)
    .bind(artifact.size_bytes)
    .bind(&artifact.sha256)
    .bind(&artifact.sha512)
    .bind(artifact.uploaded_at)
    .execute(state.db())
    .await
    .is_err()
    {
        return composer_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to record Composer distribution",
        );
    }

    if sqlx::query("UPDATE releases SET status = 'published', updated_at = NOW() WHERE id = $1")
        .bind(release.id)
        .execute(state.db())
        .await
        .is_err()
    {
        return composer_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to publish release",
        );
    }

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
        "ecosystem": "composer",
        "name": parsed.name,
        "version": parsed.version,
        "source": "composer_publish",
        "artifact_sha256": parsed.sha256,
    }))
    .execute(state.db())
    .await;

    let _ = sqlx::query("UPDATE packages SET updated_at = NOW() WHERE id = $1")
        .bind(package_id)
        .execute(state.db())
        .await;

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "ok": true,
            "name": parsed.name,
            "version": parsed.version,
        })),
    )
        .into_response()
}

async fn yank_package_version<S: ComposerAppState>(
    State(state): State<S>,
    Path((vendor, package, version)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Response {
    let expected_name = format!("{vendor}/{package}");
    if let Err(error) = validate_composer_package_name(&expected_name) {
        return composer_error_response(StatusCode::BAD_REQUEST, &error.to_string());
    }

    let identity = match authenticate_required(&state, &headers).await {
        Ok(identity) => identity,
        Err(response) => return response,
    };

    if !has_scope(&identity, "packages:write") {
        return composer_error_response(
            StatusCode::FORBIDDEN,
            "Token does not have the packages:write scope",
        );
    }

    let normalized_name = normalize_package_name(&expected_name, &Ecosystem::Composer);
    let package_row = match sqlx::query(
        "SELECT id, repository_id, owner_user_id, owner_org_id \
         FROM packages \
         WHERE ecosystem = 'composer' AND normalized_name = $1",
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

    let package_id: Uuid = package_row.try_get("id").unwrap_or_default();
    let repository_id: Uuid = package_row.try_get("repository_id").unwrap_or_default();
    if !has_package_write_access(
        state.db(),
        package_id,
        repository_id,
        package_row.try_get("owner_user_id").unwrap_or(None),
        package_row.try_get("owner_org_id").unwrap_or(None),
        identity.user_id,
    )
    .await
    {
        return composer_error_response(
            StatusCode::FORBIDDEN,
            "You do not have permission to modify this package",
        );
    }

    let update_result = sqlx::query(
        "UPDATE releases \
         SET status = 'yanked', is_yanked = true, updated_at = NOW() \
         WHERE package_id = $1 AND version = $2 \
           AND status IN ('published', 'deprecated', 'yanked')",
    )
    .bind(package_id)
    .bind(&version)
    .execute(state.db())
    .await;

    match update_result {
        Ok(result) if result.rows_affected() > 0 => {
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
                "ecosystem": "composer",
                "name": expected_name,
                "version": version,
                "source": "composer_yank",
            }))
            .execute(state.db())
            .await;

            let _ = sqlx::query("UPDATE packages SET updated_at = NOW() WHERE id = $1")
                .bind(package_id)
                .execute(state.db())
                .await;

            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "ok": true,
                    "name": expected_name,
                    "version": version,
                })),
            )
                .into_response()
        }
        Ok(_) => composer_error_response(StatusCode::NOT_FOUND, "Version not found"),
        Err(_) => composer_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error"),
    }
}

async fn parse_publish_multipart(
    expected_name: &str,
    mut multipart: Multipart,
) -> Result<ParsedComposerPublish, Response> {
    let mut composer_json_bytes = None;
    let mut dist_zip_bytes = None;

    loop {
        let field = multipart.next_field().await.map_err(|_| {
            composer_error_response(
                StatusCode::BAD_REQUEST,
                "The multipart request body could not be parsed",
            )
        })?;

        let Some(field) = field else {
            break;
        };

        let Some(name) = field.name().map(str::to_owned) else {
            return Err(composer_error_response(
                StatusCode::BAD_REQUEST,
                "Every multipart field must include a field name",
            ));
        };

        let bytes = field.bytes().await.map_err(|_| {
            composer_error_response(
                StatusCode::BAD_REQUEST,
                "A multipart field could not be read",
            )
        })?;

        match name.as_str() {
            "composer.json" => {
                if composer_json_bytes.is_some() {
                    return Err(composer_error_response(
                        StatusCode::BAD_REQUEST,
                        "The multipart field 'composer.json' may only be provided once",
                    ));
                }
                composer_json_bytes = Some(bytes);
            }
            "dist.zip" => {
                if dist_zip_bytes.is_some() {
                    return Err(composer_error_response(
                        StatusCode::BAD_REQUEST,
                        "The multipart field 'dist.zip' may only be provided once",
                    ));
                }
                dist_zip_bytes = Some(bytes);
            }
            _ => {
                return Err(composer_error_response(
                    StatusCode::BAD_REQUEST,
                    &format!("Unsupported multipart field '{name}'"),
                ));
            }
        }
    }

    let composer_json_bytes = composer_json_bytes.ok_or_else(|| {
        composer_error_response(
            StatusCode::BAD_REQUEST,
            "The multipart field 'composer.json' is required",
        )
    })?;
    let dist_zip_bytes = dist_zip_bytes.ok_or_else(|| {
        composer_error_response(
            StatusCode::BAD_REQUEST,
            "The multipart field 'dist.zip' is required",
        )
    })?;

    publish::parse_composer_publish(expected_name, composer_json_bytes, dist_zip_bytes)
        .map_err(|error| composer_error_response(StatusCode::BAD_REQUEST, &error.to_string()))
}

async fn load_visible_packages(
    db: &PgPool,
    actor_user_id: Option<Uuid>,
) -> Result<Vec<String>, Response> {
    let rows = sqlx::query(
        "SELECT DISTINCT p.name, p.visibility AS package_visibility, p.owner_user_id AS package_owner_user_id, \
                  p.owner_org_id AS package_owner_org_id, r.visibility::text AS repository_visibility, \
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
            "SELECT id, user_id, scopes, expires_at, is_revoked, kind FROM tokens WHERE token_hash = $1",
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

        let token_id: Option<Uuid> = row.try_get("id").ok();
        let scopes: Vec<String> = row.try_get("scopes").unwrap_or_default();

        if let Some(token_id) = token_id {
            let _ = sqlx::query("UPDATE tokens SET last_used_at = NOW() WHERE id = $1")
                .bind(token_id)
                .execute(state.db())
                .await;
        }

        return Ok(ComposerIdentity {
            user_id,
            token_id,
            scopes,
        });
    }

    let claims = publaryn_auth::validate_token(token, state.jwt_secret(), state.jwt_issuer())
        .map_err(|_| {
            composer_error_response(StatusCode::UNAUTHORIZED, "Invalid or expired token")
        })?;

    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| composer_error_response(StatusCode::UNAUTHORIZED, "Invalid token subject"))?;
    let token_id = Uuid::parse_str(&claims.jti).ok();

    Ok(ComposerIdentity {
        user_id,
        token_id,
        scopes: claims.scopes,
    })
}

async fn authenticate_required<S: ComposerAppState>(
    state: &S,
    headers: &HeaderMap,
) -> Result<ComposerIdentity, Response> {
    authenticate_optional(state, headers).await?.ok_or_else(|| {
        composer_error_response(
            StatusCode::UNAUTHORIZED,
            "Authentication required for Composer publish operations",
        )
    })
}

fn has_scope(identity: &ComposerIdentity, scope: &str) -> bool {
    identity.scopes.iter().any(|candidate| candidate == scope)
}

async fn resolve_or_create_package<S: ComposerAppState>(
    state: &S,
    identity: &ComposerIdentity,
    parsed: &ParsedComposerPublish,
) -> Result<Uuid, Response> {
    let normalized_name = normalize_package_name(&parsed.name, &Ecosystem::Composer);
    let existing = sqlx::query(
        "SELECT id, repository_id, owner_user_id, owner_org_id \
         FROM packages \
         WHERE ecosystem = 'composer' AND normalized_name = $1",
    )
    .bind(&normalized_name)
    .fetch_optional(state.db())
    .await;

    match existing {
        Ok(Some(row)) => {
            let package_id: Uuid = row.try_get("id").unwrap_or_default();
            let repository_id: Uuid = row.try_get("repository_id").unwrap_or_default();

            if !has_package_write_access(
                state.db(),
                package_id,
                repository_id,
                row.try_get("owner_user_id").unwrap_or(None),
                row.try_get("owner_org_id").unwrap_or(None),
                identity.user_id,
            )
            .await
            {
                return Err(composer_error_response(
                    StatusCode::FORBIDDEN,
                    "You do not have permission to publish to this package",
                ));
            }

            let _ = sqlx::query(
                "UPDATE packages \
                 SET description = COALESCE($1, description), \
                     homepage = COALESCE($2, homepage), \
                     repository_url = COALESCE($3, repository_url), \
                     license = COALESCE($4, license), \
                     keywords = CASE WHEN $5 THEN $6 ELSE keywords END, \
                     updated_at = NOW() \
                 WHERE id = $7",
            )
            .bind(&parsed.description)
            .bind(&parsed.homepage)
            .bind(&parsed.repository_url)
            .bind(parsed.licenses.first())
            .bind(!parsed.keywords.is_empty())
            .bind(&parsed.keywords)
            .bind(package_id)
            .execute(state.db())
            .await;

            Ok(package_id)
        }
        Ok(None) => auto_create_package(state, identity, parsed).await,
        Err(_) => Err(composer_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Database error",
        )),
    }
}

async fn auto_create_package<S: ComposerAppState>(
    state: &S,
    identity: &ComposerIdentity,
    parsed: &ParsedComposerPublish,
) -> Result<Uuid, Response> {
    let repository = match publish::select_default_repository(state.db(), identity.user_id).await {
        Ok(repository) => repository,
        Err(Error::Forbidden(message)) => {
            return Err(composer_error_response(StatusCode::FORBIDDEN, &message))
        }
        Err(_) => {
            return Err(composer_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to resolve a repository for Composer publish",
            ))
        }
    };

    let visibility = match repository.visibility.as_str() {
        "private" => "private",
        "internal_org" => "internal_org",
        "unlisted" => "unlisted",
        "quarantined" => "quarantined",
        _ => "public",
    };

    let normalized_name = normalize_package_name(&parsed.name, &Ecosystem::Composer);
    let package_id = Uuid::new_v4();
    let now = Utc::now();
    let insert_result = sqlx::query(
        "INSERT INTO packages (id, repository_id, ecosystem, name, normalized_name, display_name, \
         description, readme, homepage, repository_url, license, keywords, visibility, \
         owner_user_id, owner_org_id, is_deprecated, deprecation_message, is_archived, \
         download_count, created_at, updated_at) \
         VALUES ($1, $2, 'composer', $3, $4, NULL, $5, NULL, $6, $7, $8, $9, $10, $11, $12, \
                 false, NULL, false, 0, $13, $14)",
    )
    .bind(package_id)
    .bind(repository.id)
    .bind(&parsed.name)
    .bind(&normalized_name)
    .bind(&parsed.description)
    .bind(&parsed.homepage)
    .bind(&parsed.repository_url)
    .bind(parsed.licenses.first())
    .bind(&parsed.keywords)
    .bind(visibility)
    .bind(repository.owner_user_id)
    .bind(repository.owner_org_id)
    .bind(now)
    .bind(now)
    .execute(state.db())
    .await;

    match insert_result {
        Ok(_) => {
            let _ = sqlx::query(
                "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, \
                 target_package_id, metadata, occurred_at) \
                 VALUES ($1, 'package_create', $2, $3, $4, $5, NOW())",
            )
            .bind(Uuid::new_v4())
            .bind(identity.user_id)
            .bind(identity.token_id)
            .bind(package_id)
            .bind(serde_json::json!({
                "ecosystem": "composer",
                "name": parsed.name,
                "source": "composer_publish",
            }))
            .execute(state.db())
            .await;
            Ok(package_id)
        }
        Err(sqlx::Error::Database(db_error)) if db_error.is_unique_violation() => {
            let existing = sqlx::query(
                "SELECT id, repository_id, owner_user_id, owner_org_id \
                 FROM packages \
                 WHERE ecosystem = 'composer' AND normalized_name = $1",
            )
            .bind(&normalized_name)
            .fetch_optional(state.db())
            .await
            .map_err(|_| {
                composer_error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to reload package",
                )
            })?
            .ok_or_else(|| {
                composer_error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to reload package",
                )
            })?;

            let package_id: Uuid = existing.try_get("id").unwrap_or_default();
            let repository_id: Uuid = existing.try_get("repository_id").unwrap_or_default();
            if !has_package_write_access(
                state.db(),
                package_id,
                repository_id,
                existing.try_get("owner_user_id").unwrap_or(None),
                existing.try_get("owner_org_id").unwrap_or(None),
                identity.user_id,
            )
            .await
            {
                return Err(composer_error_response(
                    StatusCode::FORBIDDEN,
                    "You do not have permission to publish to this package",
                ));
            }
            Ok(package_id)
        }
        Err(_) => Err(composer_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to create package",
        )),
    }
}

async fn has_package_write_access(
    db: &PgPool,
    package_id: Uuid,
    repository_id: Uuid,
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
            "SELECT EXISTS (\
                 SELECT 1 FROM org_memberships \
                 WHERE org_id = $1 AND user_id = $2 AND role::text = ANY($3)\
             )",
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

        let package_permissions: Vec<String> = vec!["admin".into(), "publish".into()];
        let team_package = sqlx::query_scalar::<_, bool>(
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
        .bind(&package_permissions)
        .fetch_one(db)
        .await
        .unwrap_or(false);

        if team_package {
            return true;
        }

        let repository_permissions: Vec<String> = vec!["admin".into(), "publish".into()];
        return sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (\
                 SELECT 1 \
                 FROM team_repository_access tra \
                 JOIN team_memberships tm ON tm.team_id = tra.team_id \
                 JOIN teams t ON t.id = tra.team_id \
                 JOIN repositories r ON r.id = tra.repository_id \
                 WHERE tra.repository_id = $1 \
                   AND tm.user_id = $2 \
                   AND t.org_id = r.owner_org_id \
                   AND tra.permission::text = ANY($3)\
             )",
        )
        .bind(repository_id)
        .bind(actor_user_id)
        .bind(&repository_permissions)
        .fetch_one(db)
        .await
        .unwrap_or(false);
    }

    false
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

fn composer_error_response(status: StatusCode, message: &str) -> Response {
    (status, Json(ComposerErrorDocument { error: message })).into_response()
}
