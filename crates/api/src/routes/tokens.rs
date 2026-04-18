use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post},
    Json, Router,
};
use chrono::{Duration, Utc};
use serde::Deserialize;
use sqlx::Row;
use uuid::Uuid;

use publaryn_core::{error::Error, security};

use crate::{
    error::{ApiError, ApiResult},
    request_auth::{is_platform_admin, AuthenticatedIdentity},
    scopes::{
        ensure_scope, ensure_scope_grant_allowed, normalize_requested_scopes, SCOPE_TOKENS_READ,
        SCOPE_TOKENS_WRITE,
    },
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/tokens", post(create_token))
        .route("/v1/tokens", get(list_tokens))
        .route("/v1/tokens/{id}", delete(revoke_token))
}

#[derive(Debug, Deserialize)]
struct CreateTokenRequest {
    name: String,
    scopes: Vec<String>,
    expires_in_days: Option<u32>,
    kind: Option<String>,
}

async fn create_token(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Json(body): Json<CreateTokenRequest>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    ensure_scope(&identity, SCOPE_TOKENS_WRITE)?;

    let user_id = identity.user_id;
    let token_name = body.name.clone();
    let scopes = normalize_requested_scopes(&body.scopes).map_err(ApiError::from)?;
    let actor_is_platform_admin = is_platform_admin(&state.db, user_id).await?;
    ensure_scope_grant_allowed(&scopes, actor_is_platform_admin)?;

    let raw_token = format!("pub_{}", security::generate_random_token(32));
    let token_hash = security::hash_token(&raw_token);
    let token_id = Uuid::new_v4();
    let kind = body.kind.as_deref().unwrap_or("personal");
    let expires_at = body
        .expires_in_days
        .map(|d| Utc::now() + Duration::days(d as i64));

    sqlx::query(
        "INSERT INTO tokens (id, kind, prefix, token_hash, name, user_id, scopes, \
         expires_at, is_revoked, created_at) \
         VALUES ($1, $2, 'pub_', $3, $4, $5, $6, $7, false, NOW())",
    )
    .bind(token_id)
    .bind(kind)
    .bind(&token_hash)
    .bind(&body.name)
    .bind(user_id)
    .bind(&scopes)
    .bind(expires_at)
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_user_id, metadata, occurred_at) \
         VALUES ($1, 'token_create', $2, $3, $2, $4, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(user_id)
    .bind(identity.audit_actor_token_id())
    .bind(serde_json::json!({
        "token_id": token_id,
        "kind": kind,
        "name": token_name,
        "scopes": scopes,
        "expires_at": expires_at,
    }))
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "id": token_id,
            "token": raw_token,
            "name": body.name,
            "scopes": scopes,
            "expires_at": expires_at,
            "warning": "Store this token securely. It will not be shown again.",
        })),
    ))
}

async fn list_tokens(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_TOKENS_READ)?;

    let user_id = identity.user_id;

    let rows = sqlx::query(
        "SELECT id, name, kind, scopes, prefix, expires_at, last_used_at, is_revoked, created_at \
         FROM tokens \
         WHERE user_id = $1 AND is_revoked = false \
         ORDER BY created_at DESC",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    let tokens: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.try_get::<Uuid, _>("id").ok(),
                "name": r.try_get::<String, _>("name").ok(),
                "kind": r.try_get::<String, _>("kind").ok(),
                "scopes": r.try_get::<Vec<String>, _>("scopes").ok(),
                "prefix": r.try_get::<String, _>("prefix").ok(),
                "expires_at": r.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("expires_at").ok().flatten(),
                "last_used_at": r.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("last_used_at").ok().flatten(),
                "created_at": r.try_get::<chrono::DateTime<chrono::Utc>, _>("created_at").ok(),
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "tokens": tokens })))
}

async fn revoke_token(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_TOKENS_WRITE)?;

    let user_id = identity.user_id;

    let result = sqlx::query("UPDATE tokens SET is_revoked = true WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(user_id)
        .execute(&state.db)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    if result.rows_affected() == 0 {
        return Err(ApiError(Error::NotFound("Token not found".into())));
    }

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_user_id, metadata, occurred_at) \
         VALUES ($1, 'token_revoke', $2, $3, $2, $4, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(user_id)
    .bind(identity.audit_actor_token_id())
    .bind(serde_json::json!({ "token_id": id }))
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(Json(serde_json::json!({ "message": "Token revoked" })))
}
