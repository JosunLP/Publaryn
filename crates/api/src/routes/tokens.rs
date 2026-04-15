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
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/tokens", post(create_token))
        .route("/v1/tokens", get(list_tokens))
        .route("/v1/tokens/:id", delete(revoke_token))
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
    Json(body): Json<CreateTokenRequest>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    // TODO: extract user_id from JWT middleware
    let user_id = Uuid::nil();

    let raw_token = format!("pub_{}", security::generate_random_token(32));
    let token_hash = security::hash_token(&raw_token);
    let token_id = Uuid::new_v4();
    let kind = body.kind.as_deref().unwrap_or("personal");
    let expires_at = body.expires_in_days.map(|d| Utc::now() + Duration::days(d as i64));

    let scopes_str: Vec<&str> = body.scopes.iter().map(|s| s.as_str()).collect();

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
    .bind(&body.scopes)
    .bind(expires_at)
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "id": token_id,
            "token": raw_token,
            "name": body.name,
            "scopes": body.scopes,
            "expires_at": expires_at,
            "warning": "Store this token securely. It will not be shown again.",
        })),
    ))
}

async fn list_tokens(State(state): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    let user_id = Uuid::nil(); // TODO: extract from JWT

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
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    let user_id = Uuid::nil(); // TODO: extract from JWT

    sqlx::query(
        "UPDATE tokens SET is_revoked = true WHERE id = $1 AND user_id = $2",
    )
    .bind(id)
    .bind(user_id)
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(Json(serde_json::json!({ "message": "Token revoked" })))
}
