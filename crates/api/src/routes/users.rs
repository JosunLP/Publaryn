use axum::{
    extract::{Path, Query, State},
    routing::{get, patch},
    Json, Router,
};
use serde::Deserialize;
use sqlx::Row;
use uuid::Uuid;

use publaryn_core::error::Error;

use crate::{
    error::{ApiError, ApiResult},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/users/:username", get(get_user))
        .route("/v1/users/:username", patch(update_user))
        .route("/v1/users/:username/packages", get(list_user_packages))
}

async fn get_user(
    State(state): State<AppState>,
    Path(username): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let row = sqlx::query(
        "SELECT id, username, display_name, avatar_url, bio, website, created_at \
         FROM users WHERE username = $1 AND is_active = true",
    )
    .bind(&username)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| ApiError(Error::NotFound(format!("User '{username}' not found"))))?;

    let id: Uuid = row.try_get("id").map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let uname: String = row.try_get("username").unwrap_or_default();
    let display_name: Option<String> = row.try_get("display_name").ok().flatten();
    let avatar_url: Option<String> = row.try_get("avatar_url").ok().flatten();
    let bio: Option<String> = row.try_get("bio").ok().flatten();
    let website: Option<String> = row.try_get("website").ok().flatten();
    let created_at: chrono::DateTime<chrono::Utc> = row.try_get("created_at").unwrap_or_default();

    Ok(Json(serde_json::json!({
        "id": id,
        "username": uname,
        "display_name": display_name,
        "avatar_url": avatar_url,
        "bio": bio,
        "website": website,
        "created_at": created_at,
    })))
}

#[derive(Debug, Deserialize)]
struct UpdateUserRequest {
    display_name: Option<String>,
    bio: Option<String>,
    website: Option<String>,
    avatar_url: Option<String>,
}

async fn update_user(
    State(state): State<AppState>,
    Path(username): Path<String>,
    Json(body): Json<UpdateUserRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    sqlx::query(
        "UPDATE users \
         SET display_name = COALESCE($1, display_name), \
             bio          = COALESCE($2, bio), \
             website      = COALESCE($3, website), \
             avatar_url   = COALESCE($4, avatar_url), \
             updated_at   = NOW() \
         WHERE username = $5 AND is_active = true",
    )
    .bind(&body.display_name)
    .bind(&body.bio)
    .bind(&body.website)
    .bind(&body.avatar_url)
    .bind(&username)
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(Json(serde_json::json!({ "message": "User updated" })))
}

#[derive(Debug, Deserialize)]
struct PackageListQuery {
    page: Option<u32>,
    per_page: Option<u32>,
}

async fn list_user_packages(
    State(state): State<AppState>,
    Path(username): Path<String>,
    Query(q): Query<PackageListQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let limit = q.per_page.unwrap_or(20).min(100) as i64;
    let offset = ((q.page.unwrap_or(1).saturating_sub(1)) as i64) * limit;

    let rows = sqlx::query(
        "SELECT p.id, p.name, p.ecosystem, p.description, p.visibility, \
                p.download_count, p.created_at \
         FROM packages p \
         JOIN users u ON u.id = p.owner_user_id \
         WHERE u.username = $1 AND p.visibility != 'private' \
         ORDER BY p.download_count DESC \
         LIMIT $2 OFFSET $3",
    )
    .bind(&username)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    let packages: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.try_get::<Uuid, _>("id").ok(),
                "name": r.try_get::<String, _>("name").ok(),
                "ecosystem": r.try_get::<String, _>("ecosystem").ok(),
                "description": r.try_get::<Option<String>, _>("description").ok().flatten(),
                "visibility": r.try_get::<String, _>("visibility").ok(),
                "download_count": r.try_get::<i64, _>("download_count").ok(),
                "created_at": r.try_get::<chrono::DateTime<chrono::Utc>, _>("created_at").ok(),
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "packages": packages })))
}
