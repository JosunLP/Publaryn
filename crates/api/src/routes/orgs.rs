use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{delete, get, patch, post},
    Json, Router,
};
use serde::Deserialize;
use sqlx::Row;
use uuid::Uuid;
use std::collections::HashMap;

use publaryn_core::{
    domain::organization::Organization,
    error::Error,
    validation,
};

use crate::{
    error::{ApiError, ApiResult},
    request_auth::{ensure_org_admin_by_slug, AuthenticatedIdentity},
    scopes::{ensure_scope, SCOPE_ORGS_WRITE},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/orgs", post(create_org))
        .route("/v1/orgs/:slug", get(get_org))
        .route("/v1/orgs/:slug", patch(update_org))
        .route("/v1/orgs/:slug/members", get(list_members))
        .route("/v1/orgs/:slug/members", post(add_member))
        .route("/v1/orgs/:slug/members/:username", delete(remove_member))
        .route("/v1/orgs/:slug/teams", get(list_teams))
        .route("/v1/orgs/:slug/teams", post(create_team))
        .route("/v1/orgs/:slug/packages", get(list_org_packages))
}

#[derive(Debug, Deserialize)]
struct CreateOrgRequest {
    name: String,
    slug: String,
    description: Option<String>,
    website: Option<String>,
    email: Option<String>,
}

async fn create_org(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Json(body): Json<CreateOrgRequest>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    ensure_scope(&identity, SCOPE_ORGS_WRITE)?;

    validation::validate_slug(&body.slug).map_err(ApiError::from)?;
    let org = Organization::new(body.name, body.slug);

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query(
        "INSERT INTO organizations (id, name, slug, description, website, email, \
         is_verified, mfa_required, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, false, false, $7, $7)",
    )
    .bind(org.id)
    .bind(&org.name)
    .bind(&org.slug)
    .bind(&body.description)
    .bind(&body.website)
    .bind(&body.email)
    .bind(org.created_at)
    .execute(&mut *tx)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db) if db.is_unique_violation() => {
            ApiError(Error::AlreadyExists("Organization slug already taken".into()))
        }
        _ => ApiError(Error::Database(e)),
    })?;

    sqlx::query(
        "INSERT INTO org_memberships (id, org_id, user_id, role, invited_by, joined_at) \
         VALUES ($1, $2, $3, 'owner', NULL, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(org.id)
    .bind(identity.user_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_org_id, metadata, occurred_at) \
         VALUES ($1, 'org_create', $2, $3, $4, $5, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(org.id)
    .bind(serde_json::json!({ "name": &org.name, "slug": &org.slug }))
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    tx.commit()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    Ok((StatusCode::CREATED, Json(serde_json::json!({ "id": org.id, "slug": org.slug }))))
}

async fn get_org(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let row = sqlx::query(
        "SELECT id, name, slug, description, website, email, is_verified, created_at \
         FROM organizations WHERE slug = $1",
    )
    .bind(&slug)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| ApiError(Error::NotFound(format!("Organization '{slug}' not found"))))?;

    Ok(Json(serde_json::json!({
        "id": row.try_get::<Uuid, _>("id").ok(),
        "name": row.try_get::<String, _>("name").ok(),
        "slug": row.try_get::<String, _>("slug").ok(),
        "description": row.try_get::<Option<String>, _>("description").ok().flatten(),
        "website": row.try_get::<Option<String>, _>("website").ok().flatten(),
        "email": row.try_get::<Option<String>, _>("email").ok().flatten(),
        "is_verified": row.try_get::<bool, _>("is_verified").ok(),
        "created_at": row.try_get::<chrono::DateTime<chrono::Utc>, _>("created_at").ok(),
    })))
}

#[derive(Debug, Deserialize)]
struct UpdateOrgRequest {
    description: Option<String>,
    website: Option<String>,
    email: Option<String>,
}

async fn update_org(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path(slug): Path<String>,
    Json(body): Json<UpdateOrgRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_ORGS_WRITE)?;
    ensure_org_admin_by_slug(&state.db, &slug, identity.user_id).await?;

    sqlx::query(
        "UPDATE organizations \
         SET description = COALESCE($1, description), \
             website     = COALESCE($2, website), \
             email       = COALESCE($3, email), \
             updated_at  = NOW() \
         WHERE slug = $4",
    )
    .bind(&body.description)
    .bind(&body.website)
    .bind(&body.email)
    .bind(&slug)
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(Json(serde_json::json!({ "message": "Organization updated" })))
}

async fn list_members(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let rows = sqlx::query(
        "SELECT u.username, u.display_name, om.role, om.joined_at \
         FROM org_memberships om \
         JOIN users u ON u.id = om.user_id \
         JOIN organizations o ON o.id = om.org_id \
         WHERE o.slug = $1 \
         ORDER BY om.joined_at ASC",
    )
    .bind(&slug)
    .fetch_all(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    let members: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "username": r.try_get::<String, _>("username").ok(),
                "display_name": r.try_get::<Option<String>, _>("display_name").ok().flatten(),
                "role": r.try_get::<String, _>("role").ok(),
                "joined_at": r.try_get::<chrono::DateTime<chrono::Utc>, _>("joined_at").ok(),
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "members": members })))
}

#[derive(Debug, Deserialize)]
struct AddMemberRequest {
    username: String,
    role: String,
}

async fn add_member(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path(slug): Path<String>,
    Json(body): Json<AddMemberRequest>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    ensure_scope(&identity, SCOPE_ORGS_WRITE)?;
    let org_id = ensure_org_admin_by_slug(&state.db, &slug, identity.user_id).await?;

    let user_row = sqlx::query("SELECT id FROM users WHERE username = $1")
        .bind(&body.username)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?
        .ok_or_else(|| ApiError(Error::NotFound(format!("User '{}' not found", body.username))))?;

    let user_id: Uuid = user_row.try_get("id").map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    sqlx::query(
           "INSERT INTO org_memberships (id, org_id, user_id, role, invited_by, joined_at) \
            VALUES ($1, $2, $3, $4, $5, NOW()) \
            ON CONFLICT (org_id, user_id) DO UPDATE \
            SET role = EXCLUDED.role, invited_by = EXCLUDED.invited_by",
    )
    .bind(Uuid::new_v4())
    .bind(org_id)
    .bind(user_id)
    .bind(&body.role)
        .bind(identity.user_id)
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    Ok((StatusCode::CREATED, Json(serde_json::json!({ "message": "Member added" }))))
}

async fn remove_member(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path((slug, username)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_ORGS_WRITE)?;
    let org_id = ensure_org_admin_by_slug(&state.db, &slug, identity.user_id).await?;

    let user_row = sqlx::query("SELECT id FROM users WHERE username = $1")
        .bind(&username)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?
        .ok_or_else(|| ApiError(Error::NotFound(format!("User '{username}' not found"))))?;

    let user_id: Uuid = user_row
        .try_get("id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    let result = sqlx::query("DELETE FROM org_memberships WHERE org_id = $1 AND user_id = $2")
    .bind(org_id)
    .bind(user_id)
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    if result.rows_affected() == 0 {
        return Err(ApiError(Error::NotFound("Membership not found".into())));
    }

    Ok(Json(serde_json::json!({ "message": "Member removed" })))
}

async fn list_teams(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let rows = sqlx::query(
        "SELECT t.id, t.name, t.slug, t.description, t.created_at \
         FROM teams t \
         JOIN organizations o ON o.id = t.org_id \
         WHERE o.slug = $1 \
         ORDER BY t.name ASC",
    )
    .bind(&slug)
    .fetch_all(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    let teams: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.try_get::<Uuid, _>("id").ok(),
                "name": r.try_get::<String, _>("name").ok(),
                "slug": r.try_get::<String, _>("slug").ok(),
                "description": r.try_get::<Option<String>, _>("description").ok().flatten(),
                "created_at": r.try_get::<chrono::DateTime<chrono::Utc>, _>("created_at").ok(),
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "teams": teams })))
}

#[derive(Debug, Deserialize)]
struct CreateTeamRequest {
    name: String,
    slug: String,
    description: Option<String>,
}

async fn create_team(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path(slug): Path<String>,
    Json(body): Json<CreateTeamRequest>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    ensure_scope(&identity, SCOPE_ORGS_WRITE)?;
    validation::validate_slug(&body.slug).map_err(ApiError::from)?;

    let org_id = ensure_org_admin_by_slug(&state.db, &slug, identity.user_id).await?;
    let team_id = Uuid::new_v4();

    sqlx::query(
        "INSERT INTO teams (id, org_id, name, slug, description, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, NOW(), NOW())",
    )
    .bind(team_id)
    .bind(org_id)
    .bind(&body.name)
    .bind(&body.slug)
    .bind(&body.description)
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    Ok((StatusCode::CREATED, Json(serde_json::json!({ "id": team_id, "slug": body.slug }))))
}

async fn list_org_packages(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Json<serde_json::Value>> {
    let limit: i64 = q.get("per_page").and_then(|s| s.parse().ok()).unwrap_or(20_i64).min(100);
    let page: i64 = q.get("page").and_then(|s| s.parse().ok()).unwrap_or(1_i64);
    let offset = (page - 1) * limit;

    let rows = sqlx::query(
        "SELECT p.id, p.name, p.ecosystem, p.description, p.download_count, p.created_at \
         FROM packages p \
         JOIN organizations o ON o.id = p.owner_org_id \
         WHERE o.slug = $1 AND p.visibility = 'public' \
         ORDER BY p.download_count DESC \
         LIMIT $2 OFFSET $3",
    )
    .bind(&slug)
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
                "download_count": r.try_get::<i64, _>("download_count").ok(),
                "created_at": r.try_get::<chrono::DateTime<chrono::Utc>, _>("created_at").ok(),
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "packages": packages })))
}
