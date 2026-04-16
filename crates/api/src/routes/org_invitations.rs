use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{delete, get, post},
    Json, Router,
};
use chrono::{Duration, Utc};
use serde::Deserialize;
use sqlx::{postgres::PgRow, Row};
use std::str::FromStr;
use uuid::Uuid;

use publaryn_core::{
    domain::{OrgRole, OrganizationInvitation, OrganizationInvitationStatus},
    error::Error,
};

use crate::{
    error::{ApiError, ApiResult},
    request_auth::{ensure_org_admin_by_slug, AuthenticatedIdentity},
    scopes::{ensure_scope, SCOPE_ORGS_JOIN, SCOPE_ORGS_WRITE},
    state::AppState,
};

const DEFAULT_INVITATION_TTL_DAYS: i64 = 7;
const MAX_INVITATION_TTL_DAYS: u32 = 30;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/orgs/:slug/invitations", get(list_org_invitations))
        .route("/v1/orgs/:slug/invitations", post(create_org_invitation))
        .route(
            "/v1/orgs/:slug/invitations/:id",
            delete(revoke_org_invitation),
        )
        .route("/v1/org-invitations", get(list_my_org_invitations))
        .route(
            "/v1/org-invitations/:id/accept",
            post(accept_org_invitation),
        )
        .route(
            "/v1/org-invitations/:id/decline",
            post(decline_org_invitation),
        )
}

#[derive(Debug, Deserialize)]
struct CreateOrgInvitationRequest {
    username_or_email: String,
    role: Option<String>,
    expires_in_days: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct InvitationListQuery {
    include_inactive: Option<bool>,
}

async fn create_org_invitation(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path(slug): Path<String>,
    Json(body): Json<CreateOrgInvitationRequest>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    ensure_scope(&identity, SCOPE_ORGS_WRITE)?;

    let identifier = body.username_or_email.trim();
    if identifier.is_empty() {
        return Err(ApiError(Error::Validation(
            "Invitation target must not be empty".into(),
        )));
    }

    let expires_in_days = body
        .expires_in_days
        .unwrap_or(DEFAULT_INVITATION_TTL_DAYS as u32);
    if expires_in_days == 0 || expires_in_days > MAX_INVITATION_TTL_DAYS {
        return Err(ApiError(Error::Validation(format!(
            "Invitation expiry must be between 1 and {MAX_INVITATION_TTL_DAYS} days"
        ))));
    }

    let role = body
        .role
        .as_deref()
        .map(OrgRole::from_str)
        .transpose()
        .map_err(ApiError::from)?
        .unwrap_or(OrgRole::Viewer);

    if role.is_owner() {
        return Err(ApiError(Error::Validation(
            "Owner invitations are not supported; use a dedicated ownership transfer flow".into(),
        )));
    }

    let org_id = ensure_org_admin_by_slug(&state.db, &slug, identity.user_id).await?;

    let user_row = sqlx::query(
        "SELECT id, username, email \
         FROM users \
         WHERE (username = $1 OR email = $1) AND is_active = true",
    )
    .bind(identifier)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| {
        ApiError(Error::NotFound(
            "No active user matches the provided username or email".into(),
        ))
    })?;

    let invited_user_id: Uuid = user_row
        .try_get("id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let invited_username: String = user_row
        .try_get("username")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let invited_email: String = user_row
        .try_get("email")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    let existing_membership = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (\
             SELECT 1 \
             FROM org_memberships \
             WHERE org_id = $1 AND user_id = $2\
         )",
    )
    .bind(org_id)
    .bind(invited_user_id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    if existing_membership {
        return Err(ApiError(Error::Conflict(
            "The selected user is already a member of this organization".into(),
        )));
    }

    sqlx::query(
        "UPDATE org_invitations \
         SET revoked_by = $3, revoked_at = NOW() \
         WHERE org_id = $1 \
           AND invited_user_id = $2 \
           AND accepted_at IS NULL \
           AND declined_at IS NULL \
           AND revoked_at IS NULL \
           AND expires_at <= NOW()",
    )
    .bind(org_id)
    .bind(invited_user_id)
    .bind(identity.user_id)
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    let expires_at = Utc::now() + Duration::days(expires_in_days as i64);
    let invitation =
        OrganizationInvitation::new(org_id, invited_user_id, role, identity.user_id, expires_at)
            .map_err(ApiError::from)?;

    sqlx::query(
        "INSERT INTO org_invitations (id, org_id, invited_user_id, role, invited_by, expires_at, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(invitation.id)
    .bind(invitation.org_id)
    .bind(invitation.invited_user_id)
    .bind(invitation.role.as_str())
    .bind(invitation.invited_by)
    .bind(invitation.expires_at)
    .bind(invitation.created_at)
    .execute(&state.db)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db) if db.is_unique_violation() => ApiError(Error::AlreadyExists(
            "An active invitation already exists for this user".into(),
        )),
        _ => ApiError(Error::Database(e)),
    })?;

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_user_id, target_org_id, metadata, occurred_at) \
         VALUES ($1, 'org_invitation_create', $2, $3, $4, $5, $6, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(invited_user_id)
    .bind(org_id)
    .bind(serde_json::json!({
        "org_slug": slug,
        "invited_username": invited_username,
        "invited_email": invited_email,
        "role": invitation.role.as_str(),
        "expires_at": invitation.expires_at,
    }))
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "id": invitation.id,
            "org_slug": slug,
            "invited_user": {
                "id": invited_user_id,
                "username": invited_username,
                "email": invited_email,
            },
            "role": invitation.role.as_str(),
            "status": invitation.status_at(Utc::now()).as_str(),
            "expires_at": invitation.expires_at,
            "created_at": invitation.created_at,
            "next_step": "The invited user can review pending invitations via GET /v1/org-invitations and accept or decline them after authenticating.",
        })),
    ))
}

async fn list_org_invitations(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path(slug): Path<String>,
    Query(query): Query<InvitationListQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_ORGS_WRITE)?;

    let org_id = ensure_org_admin_by_slug(&state.db, &slug, identity.user_id).await?;
    let include_inactive = query.include_inactive.unwrap_or(false);

    let sql = if include_inactive {
        "SELECT oi.id, oi.org_id, oi.invited_user_id, oi.role, oi.invited_by, oi.accepted_by, oi.accepted_at, \
                oi.declined_by, oi.declined_at, oi.revoked_by, oi.revoked_at, oi.expires_at, oi.created_at, \
                invited_user.username AS invited_username, invited_user.email AS invited_email, \
                inviter.username AS invited_by_username \
         FROM org_invitations oi \
         JOIN users invited_user ON invited_user.id = oi.invited_user_id \
         JOIN users inviter ON inviter.id = oi.invited_by \
         WHERE oi.org_id = $1 \
         ORDER BY oi.created_at DESC"
    } else {
        "SELECT oi.id, oi.org_id, oi.invited_user_id, oi.role, oi.invited_by, oi.accepted_by, oi.accepted_at, \
                oi.declined_by, oi.declined_at, oi.revoked_by, oi.revoked_at, oi.expires_at, oi.created_at, \
                invited_user.username AS invited_username, invited_user.email AS invited_email, \
                inviter.username AS invited_by_username \
         FROM org_invitations oi \
         JOIN users invited_user ON invited_user.id = oi.invited_user_id \
         JOIN users inviter ON inviter.id = oi.invited_by \
         WHERE oi.org_id = $1 \
           AND oi.accepted_at IS NULL \
           AND oi.declined_at IS NULL \
           AND oi.revoked_at IS NULL \
           AND oi.expires_at > NOW() \
         ORDER BY oi.created_at DESC"
    };

    let rows = sqlx::query(sql)
        .bind(org_id)
        .fetch_all(&state.db)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    let now = Utc::now();
    let invitations = rows
        .iter()
        .map(|row| org_invitation_admin_payload(row, now))
        .collect::<ApiResult<Vec<_>>>()?;

    Ok(Json(serde_json::json!({ "invitations": invitations })))
}

async fn revoke_org_invitation(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path((slug, id)): Path<(String, Uuid)>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_ORGS_WRITE)?;

    let org_id = ensure_org_admin_by_slug(&state.db, &slug, identity.user_id).await?;

    let row = sqlx::query(
        "UPDATE org_invitations \
         SET revoked_by = $3, revoked_at = NOW() \
         WHERE id = $1 \
           AND org_id = $2 \
           AND accepted_at IS NULL \
           AND declined_at IS NULL \
           AND revoked_at IS NULL \
         RETURNING id, org_id, invited_user_id, role, invited_by, accepted_by, accepted_at, \
                   declined_by, declined_at, revoked_by, revoked_at, expires_at, created_at",
    )
    .bind(id)
    .bind(org_id)
    .bind(identity.user_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| ApiError(Error::NotFound("Invitation not found".into())))?;

    let invitation = build_invitation_from_row(&row)?;

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_user_id, target_org_id, metadata, occurred_at) \
         VALUES ($1, 'org_invitation_revoke', $2, $3, $4, $5, $6, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(invitation.invited_user_id)
    .bind(invitation.org_id)
    .bind(serde_json::json!({
        "org_slug": slug,
        "role": invitation.role.as_str(),
        "invitation_id": invitation.id,
    }))
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(Json(serde_json::json!({
        "message": "Invitation revoked",
        "id": invitation.id,
        "status": invitation.status_at(Utc::now()).as_str(),
    })))
}

async fn list_my_org_invitations(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Query(query): Query<InvitationListQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_ORGS_JOIN)?;

    let include_inactive = query.include_inactive.unwrap_or(false);
    let sql = if include_inactive {
        "SELECT oi.id, oi.org_id, oi.invited_user_id, oi.role, oi.invited_by, oi.accepted_by, oi.accepted_at, \
                oi.declined_by, oi.declined_at, oi.revoked_by, oi.revoked_at, oi.expires_at, oi.created_at, \
                org.slug AS org_slug, org.name AS org_name, inviter.username AS invited_by_username \
         FROM org_invitations oi \
         JOIN organizations org ON org.id = oi.org_id \
         JOIN users inviter ON inviter.id = oi.invited_by \
         WHERE oi.invited_user_id = $1 \
         ORDER BY oi.created_at DESC"
    } else {
        "SELECT oi.id, oi.org_id, oi.invited_user_id, oi.role, oi.invited_by, oi.accepted_by, oi.accepted_at, \
                oi.declined_by, oi.declined_at, oi.revoked_by, oi.revoked_at, oi.expires_at, oi.created_at, \
                org.slug AS org_slug, org.name AS org_name, inviter.username AS invited_by_username \
         FROM org_invitations oi \
         JOIN organizations org ON org.id = oi.org_id \
         JOIN users inviter ON inviter.id = oi.invited_by \
         WHERE oi.invited_user_id = $1 \
           AND oi.accepted_at IS NULL \
           AND oi.declined_at IS NULL \
           AND oi.revoked_at IS NULL \
           AND oi.expires_at > NOW() \
         ORDER BY oi.created_at DESC"
    };

    let rows = sqlx::query(sql)
        .bind(identity.user_id)
        .fetch_all(&state.db)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    let now = Utc::now();
    let invitations = rows
        .iter()
        .map(|row| org_invitation_self_payload(row, now))
        .collect::<ApiResult<Vec<_>>>()?;

    Ok(Json(serde_json::json!({ "invitations": invitations })))
}

async fn accept_org_invitation(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_ORGS_JOIN)?;

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    let row = sqlx::query(
        "SELECT oi.id, oi.org_id, oi.invited_user_id, oi.role, oi.invited_by, oi.accepted_by, oi.accepted_at, \
                oi.declined_by, oi.declined_at, oi.revoked_by, oi.revoked_at, oi.expires_at, oi.created_at, \
                org.slug AS org_slug, org.name AS org_name \
         FROM org_invitations oi \
         JOIN organizations org ON org.id = oi.org_id \
         WHERE oi.id = $1 AND oi.invited_user_id = $2 \
         FOR UPDATE",
    )
    .bind(id)
    .bind(identity.user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| ApiError(Error::NotFound("Invitation not found".into())))?;

    let invitation = build_invitation_from_row(&row)?;
    let org_slug: String = row
        .try_get("org_slug")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let org_name: String = row
        .try_get("org_name")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let status = invitation.status_at(Utc::now());

    if !invitation.is_actionable_at(Utc::now()) {
        return Err(ApiError(Error::Conflict(format!(
            "Invitation is no longer actionable (status: {})",
            status.as_str()
        ))));
    }

    let membership_insert = sqlx::query(
        "INSERT INTO org_memberships (id, org_id, user_id, role, invited_by, joined_at) \
         VALUES ($1, $2, $3, $4, $5, NOW()) \
         ON CONFLICT (org_id, user_id) DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind(invitation.org_id)
    .bind(identity.user_id)
    .bind(invitation.role.as_str())
    .bind(invitation.invited_by)
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    if membership_insert.rows_affected() == 0 {
        return Err(ApiError(Error::Conflict(
            "You are already a member of this organization".into(),
        )));
    }

    sqlx::query(
        "UPDATE org_invitations \
         SET accepted_by = $2, accepted_at = NOW() \
         WHERE id = $1",
    )
    .bind(invitation.id)
    .bind(identity.user_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_user_id, target_org_id, metadata, occurred_at) \
         VALUES ($1, 'org_invitation_accept', $2, $3, $2, $4, $5, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(invitation.org_id)
    .bind(serde_json::json!({
        "org_slug": org_slug,
        "org_name": org_name,
        "role": invitation.role.as_str(),
        "invitation_id": invitation.id,
    }))
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    tx.commit()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(Json(serde_json::json!({
        "message": "Invitation accepted",
        "org": {
            "id": invitation.org_id,
            "slug": org_slug,
            "name": org_name,
        },
        "role": invitation.role.as_str(),
    })))
}

async fn decline_org_invitation(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_ORGS_JOIN)?;

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    let row = sqlx::query(
        "SELECT id, org_id, invited_user_id, role, invited_by, accepted_by, accepted_at, \
                declined_by, declined_at, revoked_by, revoked_at, expires_at, created_at \
         FROM org_invitations \
         WHERE id = $1 AND invited_user_id = $2 \
         FOR UPDATE",
    )
    .bind(id)
    .bind(identity.user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| ApiError(Error::NotFound("Invitation not found".into())))?;

    let invitation = build_invitation_from_row(&row)?;
    let status = invitation.status_at(Utc::now());

    if !invitation.is_actionable_at(Utc::now()) {
        return Err(ApiError(Error::Conflict(format!(
            "Invitation is no longer actionable (status: {})",
            status.as_str()
        ))));
    }

    sqlx::query(
        "UPDATE org_invitations \
         SET declined_by = $2, declined_at = NOW() \
         WHERE id = $1",
    )
    .bind(invitation.id)
    .bind(identity.user_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_user_id, target_org_id, metadata, occurred_at) \
         VALUES ($1, 'org_invitation_decline', $2, $3, $2, $4, $5, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(invitation.org_id)
    .bind(serde_json::json!({
        "role": invitation.role.as_str(),
        "invitation_id": invitation.id,
    }))
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    tx.commit()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(Json(serde_json::json!({
        "message": "Invitation declined",
        "id": invitation.id,
    })))
}

fn build_invitation_from_row(row: &PgRow) -> ApiResult<OrganizationInvitation> {
    Ok(OrganizationInvitation {
        id: row
            .try_get("id")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        org_id: row
            .try_get("org_id")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        invited_user_id: row
            .try_get("invited_user_id")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        role: row
            .try_get("role")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        invited_by: row
            .try_get("invited_by")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        accepted_by: row
            .try_get("accepted_by")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        accepted_at: row
            .try_get("accepted_at")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        declined_by: row
            .try_get("declined_by")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        declined_at: row
            .try_get("declined_at")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        revoked_by: row
            .try_get("revoked_by")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        revoked_at: row
            .try_get("revoked_at")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        expires_at: row
            .try_get("expires_at")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        created_at: row
            .try_get("created_at")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
    })
}

fn org_invitation_admin_payload(
    row: &PgRow,
    now: chrono::DateTime<chrono::Utc>,
) -> ApiResult<serde_json::Value> {
    let invitation = build_invitation_from_row(row)?;
    let invited_username: String = row
        .try_get("invited_username")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let invited_email: String = row
        .try_get("invited_email")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let invited_by_username: String = row
        .try_get("invited_by_username")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    Ok(serde_json::json!({
        "id": invitation.id,
        "invited_user": {
            "id": invitation.invited_user_id,
            "username": invited_username,
            "email": invited_email,
        },
        "role": invitation.role.as_str(),
        "status": invitation.status_at(now).as_str(),
        "invited_by": {
            "id": invitation.invited_by,
            "username": invited_by_username,
        },
        "accepted_by": invitation.accepted_by,
        "accepted_at": invitation.accepted_at,
        "declined_by": invitation.declined_by,
        "declined_at": invitation.declined_at,
        "revoked_by": invitation.revoked_by,
        "revoked_at": invitation.revoked_at,
        "expires_at": invitation.expires_at,
        "created_at": invitation.created_at,
    }))
}

fn org_invitation_self_payload(
    row: &PgRow,
    now: chrono::DateTime<chrono::Utc>,
) -> ApiResult<serde_json::Value> {
    let invitation = build_invitation_from_row(row)?;
    let org_slug: String = row
        .try_get("org_slug")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let org_name: String = row
        .try_get("org_name")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let invited_by_username: String = row
        .try_get("invited_by_username")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let status = invitation.status_at(now);

    Ok(serde_json::json!({
        "id": invitation.id,
        "org": {
            "id": invitation.org_id,
            "slug": org_slug,
            "name": org_name,
        },
        "role": invitation.role.as_str(),
        "status": status.as_str(),
        "invited_by": {
            "id": invitation.invited_by,
            "username": invited_by_username,
        },
        "expires_at": invitation.expires_at,
        "created_at": invitation.created_at,
        "actionable": matches!(status, OrganizationInvitationStatus::Pending),
    }))
}
