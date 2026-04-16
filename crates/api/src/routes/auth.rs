use axum::{routing::post, Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::Row;

use publaryn_auth::{hash_password, verify_password, verify_totp, verify_recovery_code};
use publaryn_core::{
    domain::user::User,
    error::Error,
};

use crate::{
    error::{ApiError, ApiResult},
    request_auth::AuthenticatedIdentity,
    scopes::default_session_scopes,
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/auth/register", post(register))
        .route("/v1/auth/login", post(login))
        .route("/v1/auth/logout", post(logout))
        .route("/v1/auth/mfa/setup", post(mfa_setup))
        .route("/v1/auth/mfa/verify-setup", post(mfa_verify_setup))
        .route("/v1/auth/mfa/disable", post(mfa_disable))
        .route("/v1/auth/mfa/challenge", post(mfa_challenge))
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

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum LoginResponseBody {
    Token(LoginResponse),
    MfaRequired(MfaRequiredResponse),
}

#[derive(Debug, Serialize)]
struct MfaRequiredResponse {
    mfa_required: bool,
    mfa_token: String,
    expires_in: i64,
}

/// MFA pending token TTL — short-lived, only valid for the challenge step.
const MFA_PENDING_TTL_SECONDS: i64 = 300; // 5 minutes

async fn login(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(body): Json<LoginRequest>,
) -> ApiResult<Json<LoginResponseBody>> {
    let row = sqlx::query(
        "SELECT id, password_hash, is_active, is_admin, mfa_enabled FROM users \
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
    let mfa_enabled: bool = row.try_get("mfa_enabled").unwrap_or(false);

    // If MFA is enabled, return a short-lived pending token instead of a full
    // session JWT. The client must complete the flow via /mfa/challenge.
    if mfa_enabled {
        let token_id = uuid::Uuid::new_v4();
        let mfa_scopes = vec!["mfa:pending".to_string()];
        let mfa_jwt = publaryn_auth::create_token(
            user_id,
            token_id,
            mfa_scopes,
            &state.config.auth.jwt_secret,
            MFA_PENDING_TTL_SECONDS,
            &state.config.auth.issuer,
        )
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

        return Ok(Json(LoginResponseBody::MfaRequired(MfaRequiredResponse {
            mfa_required: true,
            mfa_token: mfa_jwt,
            expires_in: MFA_PENDING_TTL_SECONDS,
        })));
    }

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

    Ok(Json(LoginResponseBody::Token(LoginResponse {
        token: jwt,
        expires_in: ttl,
    })))
}

// ── Logout ────────────────────────────────────────────────────────────────────

async fn logout() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "message": "Logged out" }))
}

// ── MFA Setup ─────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct MfaSetupResponse {
    secret: String,
    provisioning_uri: String,
    recovery_codes: Vec<String>,
}

async fn mfa_setup(
    axum::extract::State(state): axum::extract::State<AppState>,
    identity: AuthenticatedIdentity,
) -> ApiResult<Json<MfaSetupResponse>> {
    // Fetch the user's current MFA state and username.
    let row = sqlx::query("SELECT username, mfa_enabled FROM users WHERE id = $1")
        .bind(identity.user_id)
        .fetch_one(&state.db)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    let mfa_enabled: bool = row.try_get("mfa_enabled").unwrap_or(false);
    if mfa_enabled {
        return Err(ApiError(Error::Validation(
            "MFA is already enabled on this account".into(),
        )));
    }

    let username: String = row
        .try_get("username")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    let setup = publaryn_auth::setup_totp(&username, "Publaryn")
        .map_err(|e| ApiError(e))?;

    // Store the pending secret so verify-setup can confirm it.
    sqlx::query(
        "UPDATE users SET mfa_totp_pending_secret = $1, \
         mfa_recovery_code_hashes = $2, updated_at = NOW() \
         WHERE id = $3",
    )
    .bind(&setup.secret_base32)
    .bind(&setup.recovery_code_hashes)
    .bind(identity.user_id)
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(Json(MfaSetupResponse {
        secret: setup.secret_base32,
        provisioning_uri: setup.provisioning_uri,
        recovery_codes: setup.recovery_codes,
    }))
}

// ── MFA Verify Setup ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct MfaVerifySetupRequest {
    code: String,
}

async fn mfa_verify_setup(
    axum::extract::State(state): axum::extract::State<AppState>,
    identity: AuthenticatedIdentity,
    Json(body): Json<MfaVerifySetupRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let row = sqlx::query(
        "SELECT mfa_enabled, mfa_totp_pending_secret FROM users WHERE id = $1",
    )
    .bind(identity.user_id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    let mfa_enabled: bool = row.try_get("mfa_enabled").unwrap_or(false);
    if mfa_enabled {
        return Err(ApiError(Error::Validation(
            "MFA is already enabled".into(),
        )));
    }

    let pending_secret: Option<String> = row.try_get("mfa_totp_pending_secret").ok().flatten();
    let pending_secret = pending_secret.ok_or_else(|| {
        ApiError(Error::Validation(
            "No pending MFA setup — call /mfa/setup first".into(),
        ))
    })?;

    let valid = verify_totp(&pending_secret, &body.code)
        .map_err(|e| ApiError(e))?;

    if !valid {
        return Err(ApiError(Error::Unauthorized(
            "Invalid TOTP code".into(),
        )));
    }

    // Activate MFA: move pending secret to active, clear pending.
    sqlx::query(
        "UPDATE users SET mfa_enabled = true, mfa_totp_secret = $1, \
         mfa_totp_pending_secret = NULL, updated_at = NOW() \
         WHERE id = $2",
    )
    .bind(&pending_secret)
    .bind(identity.user_id)
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    // Audit event
    let _ = sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_user_id, metadata, occurred_at) \
         VALUES ($1, 'mfa_enable'::audit_action, $2, $3, $2, $4, NOW())",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(serde_json::json!({ "source": "auth.mfa_verify_setup" }))
    .execute(&state.db)
    .await;

    Ok(Json(serde_json::json!({ "mfa_enabled": true })))
}

// ── MFA Disable ───────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct MfaDisableRequest {
    code: String,
}

async fn mfa_disable(
    axum::extract::State(state): axum::extract::State<AppState>,
    identity: AuthenticatedIdentity,
    Json(body): Json<MfaDisableRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let row = sqlx::query(
        "SELECT mfa_enabled, mfa_totp_secret FROM users WHERE id = $1",
    )
    .bind(identity.user_id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    let mfa_enabled: bool = row.try_get("mfa_enabled").unwrap_or(false);
    if !mfa_enabled {
        return Err(ApiError(Error::Validation(
            "MFA is not enabled on this account".into(),
        )));
    }

    let secret: String = row
        .try_get::<Option<String>, _>("mfa_totp_secret")
        .ok()
        .flatten()
        .ok_or_else(|| ApiError(Error::Internal("MFA secret missing".into())))?;

    let valid = verify_totp(&secret, &body.code).map_err(|e| ApiError(e))?;
    if !valid {
        return Err(ApiError(Error::Unauthorized("Invalid TOTP code".into())));
    }

    sqlx::query(
        "UPDATE users SET mfa_enabled = false, mfa_totp_secret = NULL, \
         mfa_totp_pending_secret = NULL, mfa_recovery_code_hashes = '{}', \
         updated_at = NOW() \
         WHERE id = $1",
    )
    .bind(identity.user_id)
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    // Audit event
    let _ = sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_user_id, metadata, occurred_at) \
         VALUES ($1, 'mfa_disable'::audit_action, $2, $3, $2, $4, NOW())",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(serde_json::json!({ "source": "auth.mfa_disable" }))
    .execute(&state.db)
    .await;

    Ok(Json(serde_json::json!({ "mfa_enabled": false })))
}

// ── MFA Challenge ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct MfaChallengeRequest {
    mfa_token: String,
    code: String,
}

async fn mfa_challenge(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(body): Json<MfaChallengeRequest>,
) -> ApiResult<Json<LoginResponse>> {
    // Validate the short-lived MFA pending token.
    let claims = publaryn_auth::validate_token(
        &body.mfa_token,
        &state.config.auth.jwt_secret,
        &state.config.auth.issuer,
    )
    .map_err(|_| ApiError(Error::Unauthorized("Invalid or expired MFA token".into())))?;

    // Ensure this is actually an MFA pending token.
    if !claims.scopes.iter().any(|s| s == "mfa:pending") {
        return Err(ApiError(Error::Unauthorized(
            "Token is not an MFA challenge token".into(),
        )));
    }

    let user_id: uuid::Uuid = claims
        .sub
        .parse()
        .map_err(|_| ApiError(Error::Internal("Invalid user ID in token".into())))?;

    let row = sqlx::query(
        "SELECT is_admin, mfa_totp_secret, mfa_recovery_code_hashes FROM users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    let secret: String = row
        .try_get::<Option<String>, _>("mfa_totp_secret")
        .ok()
        .flatten()
        .ok_or_else(|| ApiError(Error::Internal("MFA secret missing".into())))?;

    let recovery_hashes: Vec<String> = row
        .try_get("mfa_recovery_code_hashes")
        .unwrap_or_default();

    // Try TOTP first, then recovery code.
    let totp_valid = verify_totp(&secret, &body.code).map_err(|e| ApiError(e))?;

    if !totp_valid {
        // Attempt recovery code verification.
        if let Some(idx) = verify_recovery_code(&body.code, &recovery_hashes) {
            // Mark this recovery code as used by removing it from the array.
            let mut remaining = recovery_hashes;
            remaining.remove(idx);
            let _ = sqlx::query(
                "UPDATE users SET mfa_recovery_code_hashes = $1, updated_at = NOW() WHERE id = $2",
            )
            .bind(&remaining)
            .bind(user_id)
            .execute(&state.db)
            .await;
        } else {
            return Err(ApiError(Error::Unauthorized("Invalid TOTP or recovery code".into())));
        }
    }

    // Issue a full session token.
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
