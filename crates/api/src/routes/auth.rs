use axum::{routing::post, Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::Row;

use publaryn_auth::{hash_password, verify_password};
use publaryn_core::{
    domain::user::User,
    error::Error,
};

use crate::{
    error::{ApiError, ApiResult},
    scopes::default_session_scopes,
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/auth/register", post(register))
        .route("/v1/auth/login", post(login))
        .route("/v1/auth/logout", post(logout))
}

// ── Register ─────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct RegisterRequest {
    username: String,
    email: String,
    password: String,
}

#[derive(Debug, Serialize)]
struct RegisterResponse {
    id: uuid::Uuid,
    username: String,
    email: String,
}

async fn register(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(body): Json<RegisterRequest>,
) -> ApiResult<Json<RegisterResponse>> {
    publaryn_core::validation::validate_username(&body.username).map_err(ApiError::from)?;
    publaryn_core::validation::validate_email(&body.email).map_err(ApiError::from)?;

    if body.password.len() < 12 {
        return Err(ApiError(Error::Validation(
            "Password must be at least 12 characters".into(),
        )));
    }

    let password_hash = hash_password(&body.password)
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    let user = User::new(
        body.username.clone(),
        body.email.clone(),
        Some(password_hash),
    );

    sqlx::query(
        "INSERT INTO users (id, username, email, password_hash, is_admin, is_active, \
         email_verified, mfa_enabled, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, false, true, false, false, $5, $5)",
    )
    .bind(user.id)
    .bind(&user.username)
    .bind(&user.email)
    .bind(&user.password_hash)
    .bind(user.created_at)
    .execute(&state.db)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db) if db.is_unique_violation() => ApiError(Error::AlreadyExists(
            "Username or email already registered".into(),
        )),
        _ => ApiError(Error::Database(e)),
    })?;

    Ok(Json(RegisterResponse {
        id: user.id,
        username: user.username,
        email: user.email,
    }))
}

// ── Login ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct LoginRequest {
    username_or_email: String,
    password: String,
}

#[derive(Debug, Serialize)]
struct LoginResponse {
    token: String,
    expires_in: i64,
}

async fn login(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(body): Json<LoginRequest>,
) -> ApiResult<Json<LoginResponse>> {
    let row = sqlx::query(
        "SELECT id, password_hash, is_active, is_admin FROM users \
         WHERE username = $1 OR email = $1",
    )
    .bind(&body.username_or_email)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| ApiError(Error::Unauthorized("Invalid credentials".into())))?;

    let is_active: bool = row.try_get("is_active").unwrap_or(false);
    if !is_active {
        return Err(ApiError(Error::Unauthorized("Account is disabled".into())));
    }

    let hash: Option<String> = row.try_get("password_hash").ok().flatten();
    let hash = hash.ok_or_else(|| {
        ApiError(Error::Unauthorized(
            "Password login not available for this account".into(),
        ))
    })?;

    let valid = verify_password(&body.password, &hash)
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    if !valid {
        return Err(ApiError(Error::Unauthorized("Invalid credentials".into())));
    }

    let user_id: uuid::Uuid = row
        .try_get("id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let is_admin: bool = row.try_get("is_admin").unwrap_or(false);
    let token_id = uuid::Uuid::new_v4();
    let ttl = state.config.auth.jwt_ttl_seconds;
    let session_scopes = default_session_scopes(is_admin);

    let jwt = publaryn_auth::create_token(
        user_id,
        token_id,
        session_scopes,
        &state.config.auth.jwt_secret,
        ttl,
        &state.config.auth.issuer,
    )
    .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    Ok(Json(LoginResponse {
        token: jwt,
        expires_in: ttl,
    }))
}

// ── Logout ────────────────────────────────────────────────────────────────────

async fn logout() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "message": "Logged out" }))
}
