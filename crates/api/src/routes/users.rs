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
    request_auth::{actor_org_capabilities_by_id, AuthenticatedIdentity, OptionalAuthenticatedIdentity},
    scopes::{ensure_scope, SCOPE_PROFILE_WRITE},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/users/me", get(get_current_user))
        .route("/v1/users/me", patch(update_current_user))
        .route(
            "/v1/users/me/organizations",
            get(list_current_user_organizations),
        )
        .route("/v1/users/{username}", get(get_user))
        .route("/v1/users/{username}", patch(update_user))
        .route("/v1/users/{username}/packages", get(list_user_packages))
}

async fn get_current_user(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
) -> ApiResult<Json<serde_json::Value>> {
    let row = sqlx::query(
        "SELECT id, username, email, display_name, avatar_url, bio, website, mfa_enabled, created_at \
         FROM users WHERE id = $1 AND is_active = true",
    )
    .bind(identity.user_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| ApiError(Error::NotFound("Current user not found".into())))?;

    Ok(Json(serde_json::json!({
        "id": row.try_get::<Uuid, _>("id").ok(),
        "username": row.try_get::<String, _>("username").ok(),
        "email": row.try_get::<String, _>("email").ok(),
        "display_name": row.try_get::<Option<String>, _>("display_name").ok().flatten(),
        "avatar_url": row.try_get::<Option<String>, _>("avatar_url").ok().flatten(),
        "bio": row.try_get::<Option<String>, _>("bio").ok().flatten(),
        "website": row.try_get::<Option<String>, _>("website").ok().flatten(),
        "mfa_enabled": row.try_get::<bool, _>("mfa_enabled").unwrap_or(false),
        "created_at": row.try_get::<chrono::DateTime<chrono::Utc>, _>("created_at").ok(),
    })))
}

async fn list_current_user_organizations(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
) -> ApiResult<Json<serde_json::Value>> {
    let rows = sqlx::query(
        "SELECT o.id, o.name, o.slug, o.description, o.website, o.is_verified, \
                om.role::text AS role, om.joined_at, \
                (SELECT COUNT(*)::BIGINT FROM teams t WHERE t.org_id = o.id) AS team_count, \
                (SELECT COUNT(*)::BIGINT FROM packages p WHERE p.owner_org_id = o.id) AS package_count \
         FROM org_memberships om \
         JOIN organizations o ON o.id = om.org_id \
         WHERE om.user_id = $1 \
         ORDER BY o.name ASC",
    )
    .bind(identity.user_id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    let mut organizations = Vec::with_capacity(rows.len());
    for row in &rows {
        let org_id: Uuid = row
            .try_get("id")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
        let capabilities =
            actor_org_capabilities_by_id(&state.db, org_id, Some(identity.user_id)).await?;
        organizations.push(serde_json::json!({
            "id": Some(org_id),
            "name": row.try_get::<String, _>("name").ok(),
            "slug": row.try_get::<String, _>("slug").ok(),
            "description": row.try_get::<Option<String>, _>("description").ok().flatten(),
            "website": row.try_get::<Option<String>, _>("website").ok().flatten(),
            "is_verified": row.try_get::<bool, _>("is_verified").ok(),
            "role": row.try_get::<String, _>("role").ok(),
            "joined_at": row.try_get::<chrono::DateTime<chrono::Utc>, _>("joined_at").ok(),
            "team_count": row.try_get::<i64, _>("team_count").ok(),
            "package_count": row.try_get::<i64, _>("package_count").ok(),
            "capabilities": capabilities,
        }));
    }

    Ok(Json(serde_json::json!({ "organizations": organizations })))
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

    let id: Uuid = row
        .try_get("id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
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
    identity: AuthenticatedIdentity,
    Path(username): Path<String>,
    Json(body): Json<UpdateUserRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_PROFILE_WRITE)?;

    let row = sqlx::query("SELECT id FROM users WHERE username = $1 AND is_active = true")
        .bind(&username)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?
        .ok_or_else(|| ApiError(Error::NotFound(format!("User '{username}' not found"))))?;

    let target_user_id: Uuid = row
        .try_get("id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    if target_user_id != identity.user_id {
        return Err(ApiError(Error::Forbidden(
            "You can only update your own profile".into(),
        )));
    }

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

async fn update_current_user(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Json(body): Json<UpdateUserRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_PROFILE_WRITE)?;

    sqlx::query(
        "UPDATE users \
         SET display_name = COALESCE($1, display_name), \
             bio          = COALESCE($2, bio), \
             website      = COALESCE($3, website), \
             avatar_url   = COALESCE($4, avatar_url), \
             updated_at   = NOW() \
         WHERE id = $5 AND is_active = true",
    )
    .bind(&body.display_name)
    .bind(&body.bio)
    .bind(&body.website)
    .bind(&body.avatar_url)
    .bind(identity.user_id)
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    get_current_user(State(state), identity).await
}

#[derive(Debug, Deserialize)]
struct PackageListQuery {
    page: Option<u32>,
    per_page: Option<u32>,
}

async fn list_user_packages(
    State(state): State<AppState>,
    identity: OptionalAuthenticatedIdentity,
    Path(username): Path<String>,
    Query(q): Query<PackageListQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let limit = q.per_page.unwrap_or(20).min(100) as i64;
    let offset = ((q.page.unwrap_or(1).saturating_sub(1)) as i64) * limit;
    let user_row = sqlx::query("SELECT id FROM users WHERE username = $1 AND is_active = true")
        .bind(&username)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?
        .ok_or_else(|| ApiError(Error::NotFound(format!("User '{username}' not found"))))?;

    let target_user_id: Uuid = user_row
        .try_get("id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let can_view_non_public = identity.user_id() == Some(target_user_id);

    let rows = sqlx::query(
        "SELECT p.id, p.name, p.ecosystem, p.description, p.visibility, \
                p.download_count, p.created_at \
         FROM packages p \
         JOIN repositories r ON r.id = p.repository_id \
         WHERE p.owner_user_id = $1 \
           AND ($2::bool = true OR (p.visibility = 'public' AND r.visibility = 'public')) \
         ORDER BY p.download_count DESC \
         LIMIT $3 OFFSET $4",
    )
    .bind(target_user_id)
    .bind(can_view_non_public)
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
