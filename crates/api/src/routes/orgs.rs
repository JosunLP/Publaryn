use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{delete, get, patch, post, put},
    Json, Router,
};
use serde::Deserialize;
use sqlx::Row;
use std::collections::{BTreeSet, HashMap};
use std::str::FromStr;
use uuid::Uuid;

use publaryn_core::{
    domain::{
        organization::{OrgRole, Organization},
        package::normalize_package_name,
        team::TeamPermission,
    },
    error::Error,
    validation,
};

use crate::{
    error::{ApiError, ApiResult},
    request_auth::{
        ensure_org_admin_by_slug, is_org_member, AuthenticatedIdentity,
        OptionalAuthenticatedIdentity,
    },
    scopes::{ensure_scope, SCOPE_ORGS_TRANSFER, SCOPE_ORGS_WRITE},
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
        .route("/v1/orgs/:slug/ownership-transfer", post(transfer_ownership))
        .route("/v1/orgs/:slug/teams", get(list_teams))
        .route("/v1/orgs/:slug/teams", post(create_team))
        .route("/v1/orgs/:slug/teams/:team_slug", patch(update_team))
        .route("/v1/orgs/:slug/teams/:team_slug", delete(delete_team))
        .route(
            "/v1/orgs/:slug/teams/:team_slug/members",
            get(list_team_members).post(add_team_member),
        )
        .route(
            "/v1/orgs/:slug/teams/:team_slug/members/:username",
            delete(remove_team_member),
        )
        .route(
            "/v1/orgs/:slug/teams/:team_slug/package-access",
            get(list_team_package_access),
        )
        .route(
            "/v1/orgs/:slug/teams/:team_slug/package-access/:ecosystem/:name",
            put(replace_team_package_access).delete(remove_team_package_access),
        )
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

#[derive(Debug, Deserialize)]
struct TransferOwnershipRequest {
    username: String,
}

async fn add_member(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path(slug): Path<String>,
    Json(body): Json<AddMemberRequest>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    ensure_scope(&identity, SCOPE_ORGS_WRITE)?;
    let org_id = ensure_org_admin_by_slug(&state.db, &slug, identity.user_id).await?;
    let role = OrgRole::from_str(&body.role).map_err(ApiError::from)?;

    if role.is_owner() {
        return Err(ApiError(Error::Validation(
            "Owner assignment is not supported through member management; use a dedicated ownership transfer flow".into(),
        )));
    }

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
    .bind(role.as_str())
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

    let membership_row = sqlx::query("SELECT role::text AS role FROM org_memberships WHERE org_id = $1 AND user_id = $2")
        .bind(org_id)
        .bind(user_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?
        .ok_or_else(|| ApiError(Error::NotFound("Membership not found".into())))?;

    let role: String = membership_row
        .try_get("role")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    if OrgRole::from_str(&role).map_err(ApiError::from)?.is_owner() {
        return Err(ApiError(Error::Forbidden(
            "Owner membership cannot be removed through this endpoint".into(),
        )));
    }

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

async fn transfer_ownership(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path(slug): Path<String>,
    Json(body): Json<TransferOwnershipRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_ORGS_TRANSFER)?;

    let target_username = body.username.trim();
    if target_username.is_empty() {
        return Err(ApiError(Error::Validation(
            "Ownership transfer target must not be empty".into(),
        )));
    }

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    let org_row = sqlx::query(
        "SELECT id, name \
         FROM organizations \
         WHERE slug = $1",
    )
    .bind(&slug)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| ApiError(Error::NotFound(format!("Organization '{slug}' not found"))))?;

    let org_id: Uuid = org_row
        .try_get("id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let org_name: String = org_row
        .try_get("name")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    let actor_membership_row = sqlx::query(
        "SELECT role::text AS role \
         FROM org_memberships \
         WHERE org_id = $1 AND user_id = $2 \
         FOR UPDATE",
    )
    .bind(org_id)
    .bind(identity.user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| {
        ApiError(Error::Forbidden(
            "Transferring organization ownership requires an owner membership".into(),
        ))
    })?;

    let actor_role = actor_membership_row
        .try_get::<String, _>("role")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let actor_role = OrgRole::from_str(&actor_role).map_err(ApiError::from)?;

    let target_membership_row = sqlx::query(
        "SELECT om.user_id, om.role::text AS role, u.username \
         FROM org_memberships om \
         JOIN users u ON u.id = om.user_id \
         WHERE om.org_id = $1 AND u.username = $2 AND u.is_active = true \
         FOR UPDATE OF om",
    )
    .bind(org_id)
    .bind(target_username)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| {
        ApiError(Error::NotFound(
            "The target user must already be an active organization member".into(),
        ))
    })?;

    let target_user_id: Uuid = target_membership_row
        .try_get("user_id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let target_username: String = target_membership_row
        .try_get("username")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let target_role = target_membership_row
        .try_get::<String, _>("role")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let target_role = OrgRole::from_str(&target_role).map_err(ApiError::from)?;

    validate_ownership_transfer(identity.user_id, &actor_role, target_user_id, &target_role)?;

    sqlx::query(
        "UPDATE org_memberships \
         SET role = 'owner' \
         WHERE org_id = $1 AND user_id = $2",
    )
    .bind(org_id)
    .bind(target_user_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query(
        "UPDATE org_memberships \
         SET role = 'admin' \
         WHERE org_id = $1 AND user_id = $2",
    )
    .bind(org_id)
    .bind(identity.user_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_user_id, target_org_id, metadata, occurred_at) \
         VALUES ($1, 'org_ownership_transfer', $2, $3, $4, $5, $6, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(target_user_id)
    .bind(org_id)
    .bind(serde_json::json!({
        "org_slug": slug,
        "org_name": org_name,
        "former_owner_user_id": identity.user_id,
        "former_owner_new_role": OrgRole::Admin.as_str(),
        "new_owner_user_id": target_user_id,
        "new_owner_username": target_username,
        "new_owner_previous_role": target_role.as_str(),
    }))
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    tx.commit()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(Json(serde_json::json!({
        "message": "Organization ownership transferred",
        "org": {
            "id": org_id,
            "slug": slug,
            "name": org_name,
        },
        "previous_owner": {
            "id": identity.user_id,
            "new_role": OrgRole::Admin.as_str(),
        },
        "new_owner": {
            "id": target_user_id,
            "username": target_username,
            "role": OrgRole::Owner.as_str(),
        },
    })))
}

fn validate_ownership_transfer(
    actor_user_id: Uuid,
    actor_role: &OrgRole,
    target_user_id: Uuid,
    target_role: &OrgRole,
) -> ApiResult<()> {
    if !actor_role.is_owner() {
        return Err(ApiError(Error::Forbidden(
            "Transferring organization ownership requires an owner membership".into(),
        )));
    }

    if actor_user_id == target_user_id {
        return Err(ApiError(Error::Validation(
            "Ownership transfer target must be a different organization member".into(),
        )));
    }

    if target_role.is_owner() {
        return Err(ApiError(Error::Conflict(
            "The selected member is already an organization owner".into(),
        )));
    }

    Ok(())
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

#[derive(Debug, Deserialize)]
struct UpdateTeamRequest {
    name: Option<String>,
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AddTeamMemberRequest {
    username: String,
}

#[derive(Debug, Deserialize)]
struct ReplaceTeamPackageAccessRequest {
    permissions: Vec<String>,
}

#[derive(Debug, Clone)]
struct TeamRecord {
    id: Uuid,
    org_id: Uuid,
    name: String,
    slug: String,
    description: Option<String>,
}

#[derive(Debug, Clone)]
struct TeamPackageAccessTarget {
    id: Uuid,
    ecosystem: String,
    name: String,
    normalized_name: String,
}

async fn create_team(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path(slug): Path<String>,
    Json(body): Json<CreateTeamRequest>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    ensure_scope(&identity, SCOPE_ORGS_WRITE)?;
    validation::validate_slug(&body.slug).map_err(ApiError::from)?;

    if body.name.trim().is_empty() {
        return Err(ApiError(Error::Validation(
            "Team name must not be empty".into(),
        )));
    }

    let org_id = ensure_org_admin_by_slug(&state.db, &slug, identity.user_id).await?;
    let team_id = Uuid::new_v4();

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query(
        "INSERT INTO teams (id, org_id, name, slug, description, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, NOW(), NOW())",
    )
    .bind(team_id)
    .bind(org_id)
    .bind(body.name.trim())
    .bind(&body.slug)
    .bind(&body.description)
    .execute(&mut *tx)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db) if db.is_unique_violation() => {
            ApiError(Error::AlreadyExists("Team slug already exists in this organization".into()))
        }
        _ => ApiError(Error::Database(e)),
    })?;

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_org_id, metadata, occurred_at) \
         VALUES ($1, 'team_create', $2, $3, $4, $5, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(org_id)
    .bind(serde_json::json!({
        "team_id": team_id,
        "team_slug": body.slug,
        "team_name": body.name.trim(),
        "description": body.description,
    }))
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    tx.commit()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "id": team_id,
            "slug": body.slug,
            "name": body.name.trim(),
        })),
    ))
}

async fn update_team(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path((slug, team_slug)): Path<(String, String)>,
    Json(body): Json<UpdateTeamRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_ORGS_WRITE)?;

    if body.name.is_none() && body.description.is_none() {
        return Err(ApiError(Error::Validation(
            "At least one team field must be provided".into(),
        )));
    }

    if body
        .name
        .as_ref()
        .is_some_and(|name| name.trim().is_empty())
    {
        return Err(ApiError(Error::Validation(
            "Team name must not be empty".into(),
        )));
    }

    let org_id = ensure_org_admin_by_slug(&state.db, &slug, identity.user_id).await?;
    let team = load_team_record(&state.db, org_id, &team_slug).await?;
    let updated_name = body.name.as_deref().map(str::trim);

    sqlx::query(
        "UPDATE teams \
         SET name = COALESCE($1, name), \
             description = COALESCE($2, description), \
             updated_at = NOW() \
         WHERE id = $3",
    )
    .bind(updated_name)
    .bind(&body.description)
    .bind(team.id)
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_org_id, metadata, occurred_at) \
         VALUES ($1, 'team_update', $2, $3, $4, $5, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(org_id)
    .bind(serde_json::json!({
        "team_id": team.id,
        "team_slug": team.slug,
        "previous_name": team.name,
        "previous_description": team.description,
        "name": updated_name.unwrap_or(team.name.as_str()),
        "description": body.description,
    }))
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(Json(serde_json::json!({ "message": "Team updated" })))
}

async fn delete_team(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path((slug, team_slug)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_ORGS_WRITE)?;

    let org_id = ensure_org_admin_by_slug(&state.db, &slug, identity.user_id).await?;
    let team = load_team_record(&state.db, org_id, &team_slug).await?;

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    let member_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM team_memberships WHERE team_id = $1",
    )
    .bind(team.id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    let package_access_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM team_package_access WHERE team_id = $1",
    )
    .bind(team.id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query("DELETE FROM teams WHERE id = $1")
        .bind(team.id)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_org_id, metadata, occurred_at) \
         VALUES ($1, 'team_delete', $2, $3, $4, $5, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(org_id)
    .bind(serde_json::json!({
        "team_id": team.id,
        "team_slug": team.slug,
        "team_name": team.name,
        "removed_member_count": member_count,
        "removed_package_access_count": package_access_count,
    }))
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    tx.commit()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(Json(serde_json::json!({ "message": "Team deleted" })))
}

async fn list_team_members(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path((slug, team_slug)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_ORGS_WRITE)?;

    let org_id = ensure_org_admin_by_slug(&state.db, &slug, identity.user_id).await?;
    let team = load_team_record(&state.db, org_id, &team_slug).await?;

    let rows = sqlx::query(
        "SELECT u.id, u.username, u.display_name, tm.added_at \
         FROM team_memberships tm \
         JOIN users u ON u.id = tm.user_id \
         WHERE tm.team_id = $1 \
         ORDER BY tm.added_at ASC",
    )
    .bind(team.id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    let members = rows
        .iter()
        .map(|row| {
            serde_json::json!({
                "id": row.try_get::<Uuid, _>("id").ok(),
                "username": row.try_get::<String, _>("username").ok(),
                "display_name": row.try_get::<Option<String>, _>("display_name").ok().flatten(),
                "added_at": row.try_get::<chrono::DateTime<chrono::Utc>, _>("added_at").ok(),
            })
        })
        .collect::<Vec<_>>();

    Ok(Json(serde_json::json!({
        "team": {
            "id": team.id,
            "slug": team.slug,
            "name": team.name,
        },
        "members": members,
    })))
}

async fn add_team_member(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path((slug, team_slug)): Path<(String, String)>,
    Json(body): Json<AddTeamMemberRequest>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    ensure_scope(&identity, SCOPE_ORGS_WRITE)?;

    let org_id = ensure_org_admin_by_slug(&state.db, &slug, identity.user_id).await?;
    let team = load_team_record(&state.db, org_id, &team_slug).await?;

    let user_row = sqlx::query(
        "SELECT u.id, EXISTS (\
             SELECT 1 \
             FROM org_memberships om \
             WHERE om.org_id = $1 AND om.user_id = u.id\
         ) AS is_org_member \
         FROM users u \
         WHERE u.username = $2",
    )
    .bind(org_id)
    .bind(&body.username)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| ApiError(Error::NotFound(format!("User '{}' not found", body.username))))?;

    let user_id: Uuid = user_row
        .try_get("id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let is_org_member = user_row
        .try_get::<bool, _>("is_org_member")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    if !is_org_member {
        return Err(ApiError(Error::Conflict(
            "Team members must already belong to the organization".into(),
        )));
    }

    let result = sqlx::query(
        "INSERT INTO team_memberships (id, team_id, user_id, added_at) \
         VALUES ($1, $2, $3, NOW()) \
         ON CONFLICT (team_id, user_id) DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind(team.id)
    .bind(user_id)
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    if result.rows_affected() == 0 {
        return Ok((
            StatusCode::OK,
            Json(serde_json::json!({ "message": "Team member already present" })),
        ));
    }

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_user_id, target_org_id, metadata, occurred_at) \
         VALUES ($1, 'team_member_add', $2, $3, $4, $5, $6, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(user_id)
    .bind(org_id)
    .bind(serde_json::json!({
        "team_id": team.id,
        "team_slug": team.slug,
        "team_name": team.name,
        "username": body.username,
    }))
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "message": "Team member added" })),
    ))
}

async fn remove_team_member(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path((slug, team_slug, username)): Path<(String, String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_ORGS_WRITE)?;

    let org_id = ensure_org_admin_by_slug(&state.db, &slug, identity.user_id).await?;
    let team = load_team_record(&state.db, org_id, &team_slug).await?;

    let user_row = sqlx::query("SELECT id FROM users WHERE username = $1")
        .bind(&username)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?
        .ok_or_else(|| ApiError(Error::NotFound(format!("User '{username}' not found"))))?;

    let user_id: Uuid = user_row
        .try_get("id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    let result = sqlx::query("DELETE FROM team_memberships WHERE team_id = $1 AND user_id = $2")
        .bind(team.id)
        .bind(user_id)
        .execute(&state.db)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    if result.rows_affected() == 0 {
        return Err(ApiError(Error::NotFound("Team membership not found".into())));
    }

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_user_id, target_org_id, metadata, occurred_at) \
         VALUES ($1, 'team_member_remove', $2, $3, $4, $5, $6, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(user_id)
    .bind(org_id)
    .bind(serde_json::json!({
        "team_id": team.id,
        "team_slug": team.slug,
        "team_name": team.name,
        "username": username,
    }))
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(Json(serde_json::json!({ "message": "Team member removed" })))
}

async fn list_team_package_access(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path((slug, team_slug)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_ORGS_WRITE)?;

    let org_id = ensure_org_admin_by_slug(&state.db, &slug, identity.user_id).await?;
    let team = load_team_record(&state.db, org_id, &team_slug).await?;

    let rows = sqlx::query(
        "SELECT p.id, p.name, p.normalized_name, p.ecosystem, \
                ARRAY_AGG(tpa.permission::text ORDER BY tpa.permission::text) AS permissions, \
                MAX(tpa.granted_at) AS granted_at \
         FROM team_package_access tpa \
         JOIN packages p ON p.id = tpa.package_id \
         WHERE tpa.team_id = $1 AND p.owner_org_id = $2 \
         GROUP BY p.id, p.name, p.normalized_name, p.ecosystem \
         ORDER BY p.ecosystem ASC, p.name ASC",
    )
    .bind(team.id)
    .bind(org_id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    let package_access = rows
        .iter()
        .map(|row| {
            serde_json::json!({
                "package_id": row.try_get::<Uuid, _>("id").ok(),
                "name": row.try_get::<String, _>("name").ok(),
                "normalized_name": row.try_get::<String, _>("normalized_name").ok(),
                "ecosystem": row.try_get::<String, _>("ecosystem").ok(),
                "permissions": row.try_get::<Vec<String>, _>("permissions").ok(),
                "granted_at": row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("granted_at").ok().flatten(),
            })
        })
        .collect::<Vec<_>>();

    Ok(Json(serde_json::json!({
        "team": {
            "id": team.id,
            "slug": team.slug,
            "name": team.name,
        },
        "package_access": package_access,
    })))
}

async fn replace_team_package_access(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path((slug, team_slug, ecosystem_str, name)): Path<(String, String, String, String)>,
    Json(body): Json<ReplaceTeamPackageAccessRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_ORGS_WRITE)?;

    let org_id = ensure_org_admin_by_slug(&state.db, &slug, identity.user_id).await?;
    let team = load_team_record(&state.db, org_id, &team_slug).await?;
    let ecosystem = crate::routes::parse_ecosystem(&ecosystem_str)?;
    let package = load_org_owned_package_for_team_access(&state.db, org_id, &ecosystem, &name).await?;
    let permissions = normalize_team_permissions(&body.permissions)?;
    let permission_strings = team_permission_strings(&permissions);

    let previous_permissions = sqlx::query(
        "SELECT permission::text AS permission \
         FROM team_package_access \
         WHERE team_id = $1 AND package_id = $2 \
         ORDER BY permission::text ASC",
    )
    .bind(team.id)
    .bind(package.id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .into_iter()
    .filter_map(|row| row.try_get::<String, _>("permission").ok())
    .collect::<Vec<_>>();

    if previous_permissions == permission_strings {
        return Ok(Json(serde_json::json!({
            "message": "Team package access unchanged",
            "package": {
                "id": package.id,
                "ecosystem": package.ecosystem,
                "name": package.name,
                "normalized_name": package.normalized_name,
            },
            "permissions": permission_strings,
        })));
    }

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query("DELETE FROM team_package_access WHERE team_id = $1 AND package_id = $2")
        .bind(team.id)
        .bind(package.id)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    for permission in &permissions {
        sqlx::query(
            "INSERT INTO team_package_access (id, team_id, package_id, permission, granted_at) \
             VALUES ($1, $2, $3, $4, NOW())",
        )
        .bind(Uuid::new_v4())
        .bind(team.id)
        .bind(package.id)
        .bind(permission)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;
    }

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_org_id, target_package_id, metadata, occurred_at) \
         VALUES ($1, 'team_package_access_update', $2, $3, $4, $5, $6, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(team.org_id)
    .bind(package.id)
    .bind(serde_json::json!({
        "team_id": team.id,
        "team_slug": team.slug,
        "team_name": team.name,
        "ecosystem": package.ecosystem,
        "package_name": package.name,
        "package_normalized_name": package.normalized_name,
        "previous_permissions": previous_permissions,
        "permissions": permission_strings,
    }))
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    tx.commit()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(Json(serde_json::json!({
        "message": "Team package access updated",
        "package": {
            "id": package.id,
            "ecosystem": package.ecosystem,
            "name": package.name,
            "normalized_name": package.normalized_name,
        },
        "permissions": permission_strings,
    })))
}

async fn remove_team_package_access(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path((slug, team_slug, ecosystem_str, name)): Path<(String, String, String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_ORGS_WRITE)?;

    let org_id = ensure_org_admin_by_slug(&state.db, &slug, identity.user_id).await?;
    let team = load_team_record(&state.db, org_id, &team_slug).await?;
    let ecosystem = crate::routes::parse_ecosystem(&ecosystem_str)?;
    let package = load_org_owned_package_for_team_access(&state.db, org_id, &ecosystem, &name).await?;

    let previous_permissions = sqlx::query(
        "SELECT permission::text AS permission \
         FROM team_package_access \
         WHERE team_id = $1 AND package_id = $2 \
         ORDER BY permission::text ASC",
    )
    .bind(team.id)
    .bind(package.id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .into_iter()
    .filter_map(|row| row.try_get::<String, _>("permission").ok())
    .collect::<Vec<_>>();

    if previous_permissions.is_empty() {
        return Err(ApiError(Error::NotFound(
            "Team package access not found".into(),
        )));
    }

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query("DELETE FROM team_package_access WHERE team_id = $1 AND package_id = $2")
        .bind(team.id)
        .bind(package.id)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_org_id, target_package_id, metadata, occurred_at) \
         VALUES ($1, 'team_package_access_update', $2, $3, $4, $5, $6, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(team.org_id)
    .bind(package.id)
    .bind(serde_json::json!({
        "team_id": team.id,
        "team_slug": team.slug,
        "team_name": team.name,
        "ecosystem": package.ecosystem,
        "package_name": package.name,
        "package_normalized_name": package.normalized_name,
        "previous_permissions": previous_permissions,
        "permissions": Vec::<String>::new(),
    }))
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    tx.commit()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(Json(serde_json::json!({ "message": "Team package access removed" })))
}

async fn list_org_packages(
    State(state): State<AppState>,
    identity: OptionalAuthenticatedIdentity,
    Path(slug): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Json<serde_json::Value>> {
    let limit: i64 = q.get("per_page").and_then(|s| s.parse().ok()).unwrap_or(20_i64).min(100);
    let page: i64 = q.get("page").and_then(|s| s.parse().ok()).unwrap_or(1_i64);
    let offset = (page - 1) * limit;
    let org_row = sqlx::query("SELECT id FROM organizations WHERE slug = $1")
        .bind(&slug)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?
        .ok_or_else(|| ApiError(Error::NotFound(format!("Organization '{slug}' not found"))))?;

    let org_id: Uuid = org_row
        .try_get("id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let can_view_non_public = match identity.user_id() {
        Some(actor_user_id) => is_org_member(&state.db, org_id, actor_user_id).await?,
        None => false,
    };

    let rows = sqlx::query(
        "SELECT p.id, p.name, p.ecosystem, p.description, p.download_count, p.created_at \
         FROM packages p \
         JOIN repositories r ON r.id = p.repository_id \
         WHERE p.owner_org_id = $1 \
           AND ($2::bool = true OR (p.visibility = 'public' AND r.visibility = 'public')) \
         ORDER BY p.download_count DESC \
         LIMIT $3 OFFSET $4",
    )
    .bind(org_id)
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
                "download_count": r.try_get::<i64, _>("download_count").ok(),
                "created_at": r.try_get::<chrono::DateTime<chrono::Utc>, _>("created_at").ok(),
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "packages": packages })))
}

async fn load_team_record(db: &sqlx::PgPool, org_id: Uuid, team_slug: &str) -> ApiResult<TeamRecord> {
    let row = sqlx::query(
        "SELECT id, org_id, name, slug, description \
         FROM teams \
         WHERE org_id = $1 AND slug = $2",
    )
    .bind(org_id)
    .bind(team_slug)
    .fetch_optional(db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| ApiError(Error::NotFound(format!("Team '{}' not found", team_slug))))?;

    Ok(TeamRecord {
        id: row
            .try_get("id")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        org_id: row
            .try_get("org_id")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        name: row
            .try_get("name")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        slug: row
            .try_get("slug")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        description: row
            .try_get::<Option<String>, _>("description")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
    })
}

async fn load_org_owned_package_for_team_access(
    db: &sqlx::PgPool,
    org_id: Uuid,
    ecosystem: &publaryn_core::domain::namespace::Ecosystem,
    package_name: &str,
) -> ApiResult<TeamPackageAccessTarget> {
    let normalized_name = normalize_package_name(package_name, ecosystem);
    let row = sqlx::query(
        "SELECT id, ecosystem, name, normalized_name, owner_org_id \
         FROM packages \
         WHERE ecosystem = $1 AND normalized_name = $2",
    )
    .bind(ecosystem.as_str())
    .bind(&normalized_name)
    .fetch_optional(db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| {
        ApiError(Error::NotFound(format!(
            "Package '{}' not found in ecosystem '{}'",
            package_name,
            ecosystem.as_str()
        )))
    })?;

    let owner_org_id = row
        .try_get::<Option<Uuid>, _>("owner_org_id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    if owner_org_id != Some(org_id) {
        return Err(ApiError(Error::Forbidden(
            "Teams can only be granted access to packages owned by the same organization".into(),
        )));
    }

    Ok(TeamPackageAccessTarget {
        id: row
            .try_get("id")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        ecosystem: row
            .try_get("ecosystem")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        name: row
            .try_get("name")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        normalized_name: row
            .try_get("normalized_name")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
    })
}

fn normalize_team_permissions(input: &[String]) -> ApiResult<Vec<TeamPermission>> {
    if input.is_empty() {
        return Err(ApiError(Error::Validation(
            "At least one team permission is required".into(),
        )));
    }

    let mut seen = BTreeSet::new();
    let mut normalized = Vec::new();

    for raw_permission in input {
        let permission = TeamPermission::from_str(raw_permission).map_err(ApiError::from)?;
        if seen.insert(permission.as_str()) {
            normalized.push(permission);
        }
    }

    normalized.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    Ok(normalized)
}

fn team_permission_strings(permissions: &[TeamPermission]) -> Vec<String> {
    permissions
        .iter()
        .map(|permission| permission.as_str().to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use publaryn_core::domain::{organization::OrgRole, team::TeamPermission};

    use super::{normalize_team_permissions, validate_ownership_transfer};

    #[test]
    fn ownership_transfer_requires_current_owner() {
        let error = validate_ownership_transfer(
            Uuid::new_v4(),
            &OrgRole::Admin,
            Uuid::new_v4(),
            &OrgRole::Viewer,
        )
        .expect_err("non-owners must not transfer ownership");

        assert_eq!(
            error.0.to_string(),
            "Forbidden: Transferring organization ownership requires an owner membership"
        );
    }

    #[test]
    fn ownership_transfer_rejects_self_transfer() {
        let actor_id = Uuid::new_v4();
        let error = validate_ownership_transfer(actor_id, &OrgRole::Owner, actor_id, &OrgRole::Owner)
            .expect_err("self-transfer must be rejected");

        assert_eq!(
            error.0.to_string(),
            "Validation error: Ownership transfer target must be a different organization member"
        );
    }

    #[test]
    fn ownership_transfer_rejects_existing_owner_target() {
        let error = validate_ownership_transfer(
            Uuid::new_v4(),
            &OrgRole::Owner,
            Uuid::new_v4(),
            &OrgRole::Owner,
        )
        .expect_err("transferring to another owner should fail");

        assert_eq!(
            error.0.to_string(),
            "Conflict: The selected member is already an organization owner"
        );
    }

    #[test]
    fn ownership_transfer_accepts_non_owner_target() {
        validate_ownership_transfer(
            Uuid::new_v4(),
            &OrgRole::Owner,
            Uuid::new_v4(),
            &OrgRole::Admin,
        )
        .expect("owner should be able to transfer to an existing non-owner member");
    }

    #[test]
    fn normalize_team_permissions_sorts_and_deduplicates() {
        let permissions = normalize_team_permissions(&[
            "publish".to_owned(),
            "admin".to_owned(),
            "publish".to_owned(),
        ])
        .expect("permissions should normalize");

        assert_eq!(permissions, vec![TeamPermission::Admin, TeamPermission::Publish]);
    }

    #[test]
    fn normalize_team_permissions_rejects_empty_inputs() {
        let error = normalize_team_permissions(&[])
            .expect_err("empty permission lists must be rejected");

        assert_eq!(
            error.0.to_string(),
            "Validation error: At least one team permission is required"
        );
    }

    #[test]
    fn normalize_team_permissions_rejects_unknown_values() {
        let error = normalize_team_permissions(&["superpowers".to_owned()])
            .expect_err("unknown permissions must be rejected");

        assert_eq!(
            error.0.to_string(),
            "Validation error: Unknown team permission: superpowers"
        );
    }
}
