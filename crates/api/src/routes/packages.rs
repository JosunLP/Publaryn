use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, patch, post, put},
    Json, Router,
};
use serde::Deserialize;
use sqlx::Row;
use uuid::Uuid;

use publaryn_core::{
    domain::{
        namespace::Ecosystem,
        package::{normalize_package_name, Package},
        release::Release,
        repository::Visibility,
    },
    error::Error,
    policy, validation,
};

use crate::{
    error::{ApiError, ApiResult},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/packages/:ecosystem/:name", get(get_package))
        .route("/v1/packages/:ecosystem/:name", patch(update_package))
        .route("/v1/packages/:ecosystem/:name", delete(delete_package))
        .route("/v1/packages/:ecosystem/:name/releases", get(list_releases))
        .route("/v1/packages/:ecosystem/:name/releases/:version", get(get_release))
        .route("/v1/packages/:ecosystem/:name/releases/:version/yank", put(yank_release))
        .route("/v1/packages/:ecosystem/:name/releases/:version/deprecate", put(deprecate_release))
        .route("/v1/packages/:ecosystem/:name/tags", get(list_tags))
        .route("/v1/packages/:ecosystem/:name/tags/:tag", put(upsert_tag))
}

fn parse_ecosystem(s: &str) -> ApiResult<Ecosystem> {
    match s.to_lowercase().as_str() {
        "npm" | "bun" => Ok(Ecosystem::Npm),
        "pypi" => Ok(Ecosystem::Pypi),
        "cargo" => Ok(Ecosystem::Cargo),
        "nuget" => Ok(Ecosystem::Nuget),
        "rubygems" => Ok(Ecosystem::Rubygems),
        "maven" => Ok(Ecosystem::Maven),
        "composer" => Ok(Ecosystem::Composer),
        "oci" => Ok(Ecosystem::Oci),
        other => Err(ApiError(Error::Validation(format!("Unknown ecosystem: {other}")))),
    }
}

async fn get_package(
    State(state): State<AppState>,
    Path((ecosystem_str, name)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let eco = parse_ecosystem(&ecosystem_str)?;
    let normalized = normalize_package_name(&name, &eco);

    let row = sqlx::query(
        "SELECT id, name, ecosystem, description, homepage, repository_url, license, keywords, \
                visibility, is_deprecated, deprecation_message, is_archived, download_count, \
                created_at, updated_at \
         FROM packages \
         WHERE ecosystem = $1 AND normalized_name = $2 AND visibility != 'private'",
    )
    .bind(eco.as_str())
    .bind(&normalized)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| {
        ApiError(Error::NotFound(format!("Package '{name}' not found in {ecosystem_str}")))
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

async fn update_package(
    State(state): State<AppState>,
    Path((ecosystem_str, name)): Path<(String, String)>,
    Json(body): Json<UpdatePackageRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let eco = parse_ecosystem(&ecosystem_str)?;
    let normalized = normalize_package_name(&name, &eco);

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
    Path((ecosystem_str, name)): Path<(String, String)>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    let eco = parse_ecosystem(&ecosystem_str)?;
    let normalized = normalize_package_name(&name, &eco);

    sqlx::query(
        "UPDATE packages SET is_archived = true, updated_at = NOW() \
         WHERE ecosystem = $1 AND normalized_name = $2",
    )
    .bind(eco.as_str())
    .bind(&normalized)
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    Ok((StatusCode::OK, Json(serde_json::json!({ "message": "Package archived" }))))
}

async fn list_releases(
    State(state): State<AppState>,
    Path((ecosystem_str, name)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let eco = parse_ecosystem(&ecosystem_str)?;
    let normalized = normalize_package_name(&name, &eco);

    let pkg_row = sqlx::query(
        "SELECT id FROM packages WHERE ecosystem = $1 AND normalized_name = $2",
    )
    .bind(eco.as_str())
    .bind(&normalized)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| ApiError(Error::NotFound(format!("Package '{name}' not found"))))?;

    let pkg_id: Uuid = pkg_row.try_get("id").map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    let rows = sqlx::query(
        "SELECT version, status, is_yanked, is_deprecated, is_prerelease, published_at \
         FROM releases \
         WHERE package_id = $1 AND status = 'published' \
         ORDER BY published_at DESC",
    )
    .bind(pkg_id)
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
    Path((ecosystem_str, name, version)): Path<(String, String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let eco = parse_ecosystem(&ecosystem_str)?;
    let normalized = normalize_package_name(&name, &eco);

    let row = sqlx::query(
        "SELECT r.id, r.version, r.status, r.is_yanked, r.yank_reason, r.is_deprecated, \
                r.deprecation_message, r.is_prerelease, r.changelog, r.source_ref, r.published_at \
         FROM releases r \
         JOIN packages p ON p.id = r.package_id \
         WHERE p.ecosystem = $1 AND p.normalized_name = $2 AND r.version = $3",
    )
    .bind(eco.as_str())
    .bind(&normalized)
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
        "status": row.try_get::<String, _>("status").ok(),
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
    Path((ecosystem_str, name, version)): Path<(String, String, String)>,
    Json(body): Json<YankRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let eco = parse_ecosystem(&ecosystem_str)?;
    let normalized = normalize_package_name(&name, &eco);

    sqlx::query(
        "UPDATE releases r \
         SET is_yanked = true, yank_reason = $1, status = 'yanked', updated_at = NOW() \
         FROM packages p \
         WHERE r.package_id = p.id \
           AND p.ecosystem = $2 AND p.normalized_name = $3 AND r.version = $4",
    )
    .bind(&body.reason)
    .bind(eco.as_str())
    .bind(&normalized)
    .bind(&version)
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(Json(serde_json::json!({ "message": "Release yanked" })))
}

#[derive(Debug, Deserialize)]
struct DeprecateRequest {
    message: Option<String>,
}

async fn deprecate_release(
    State(state): State<AppState>,
    Path((ecosystem_str, name, version)): Path<(String, String, String)>,
    Json(body): Json<DeprecateRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let eco = parse_ecosystem(&ecosystem_str)?;
    let normalized = normalize_package_name(&name, &eco);

    sqlx::query(
        "UPDATE releases r \
         SET is_deprecated = true, deprecation_message = $1, status = 'deprecated', updated_at = NOW() \
         FROM packages p \
         WHERE r.package_id = p.id \
           AND p.ecosystem = $2 AND p.normalized_name = $3 AND r.version = $4",
    )
    .bind(&body.message)
    .bind(eco.as_str())
    .bind(&normalized)
    .bind(&version)
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(Json(serde_json::json!({ "message": "Release deprecated" })))
}

async fn list_tags(
    State(state): State<AppState>,
    Path((ecosystem_str, name)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let eco = parse_ecosystem(&ecosystem_str)?;
    let normalized = normalize_package_name(&name, &eco);

    let rows = sqlx::query(
        "SELECT cr.name, r.version, cr.updated_at \
         FROM channel_refs cr \
         JOIN releases r ON r.id = cr.release_id \
         JOIN packages p ON p.id = cr.package_id \
         WHERE p.ecosystem = $1 AND p.normalized_name = $2 \
         ORDER BY cr.name",
    )
    .bind(eco.as_str())
    .bind(&normalized)
    .fetch_all(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    let mut tags = serde_json::Map::new();
    for r in &rows {
        let tag_name: String = r.try_get("name").unwrap_or_default();
        let version: String = r.try_get("version").unwrap_or_default();
        let updated_at: Option<chrono::DateTime<chrono::Utc>> = r.try_get("updated_at").ok();
        tags.insert(tag_name, serde_json::json!({ "version": version, "updated_at": updated_at }));
    }

    Ok(Json(serde_json::json!({ "tags": tags })))
}

#[derive(Debug, Deserialize)]
struct UpsertTagRequest {
    version: String,
}

async fn upsert_tag(
    State(state): State<AppState>,
    Path((ecosystem_str, name, tag)): Path<(String, String, String)>,
    Json(body): Json<UpsertTagRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let eco = parse_ecosystem(&ecosystem_str)?;
    let normalized = normalize_package_name(&name, &eco);

    let pkg_row = sqlx::query(
        "SELECT id FROM packages WHERE ecosystem = $1 AND normalized_name = $2",
    )
    .bind(eco.as_str())
    .bind(&normalized)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| ApiError(Error::NotFound(format!("Package '{name}' not found"))))?;

    let pkg_id: Uuid = pkg_row.try_get("id").map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    let rel_row = sqlx::query(
        "SELECT id FROM releases WHERE package_id = $1 AND version = $2",
    )
    .bind(pkg_id)
    .bind(&body.version)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| ApiError(Error::NotFound(format!("Release '{}' not found", body.version))))?;

    let release_id: Uuid = rel_row.try_get("id").map_err(|e| ApiError(Error::Internal(e.to_string())))?;

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
    .bind(Uuid::nil()) // TODO: use authenticated user ID
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(Json(serde_json::json!({
        "message": "Tag updated",
        "tag": tag,
        "version": body.version,
    })))
}
