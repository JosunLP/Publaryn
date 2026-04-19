use axum::{
    http::{
        header::{AUTHORIZATION, WWW_AUTHENTICATE},
        HeaderMap, HeaderValue, StatusCode,
    },
    response::{IntoResponse, Response},
    Json,
};
use chrono::Utc;
use serde::Serialize;
use sqlx::Row;
use uuid::Uuid;

use crate::routes::OciAppState;

#[derive(Debug, Clone)]
pub struct OciIdentity {
    pub user_id: Uuid,
    pub token_id: Option<Uuid>,
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum AuthFailure {
    Missing,
    Invalid(String),
}

#[derive(Debug, Serialize)]
struct OciErrorDocument {
    errors: Vec<OciErrorEntry>,
}

#[derive(Debug, Serialize)]
struct OciErrorEntry {
    code: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<serde_json::Value>,
}

pub fn with_registry_headers(mut response: Response) -> Response {
    response.headers_mut().insert(
        HeaderName::from_static("docker-distribution-api-version"),
        HeaderValue::from_static("registry/2.0"),
    );
    response
}

use axum::http::HeaderName;

pub fn oci_error_response(
    status: StatusCode,
    code: &str,
    message: &str,
    detail: Option<serde_json::Value>,
) -> Response {
    with_registry_headers(
        (
            status,
            Json(OciErrorDocument {
                errors: vec![OciErrorEntry {
                    code: code.to_owned(),
                    message: message.to_owned(),
                    detail,
                }],
            }),
        )
            .into_response(),
    )
}

pub fn challenge_scope_for_catalog() -> String {
    "registry:catalog:*".into()
}

pub fn challenge_scope_for_repository(name: &str, push: bool) -> String {
    if push {
        format!("repository:{name}:pull,push")
    } else {
        format!("repository:{name}:pull")
    }
}

pub fn challenge_response<S: OciAppState>(state: &S, scope: &str, message: &str) -> Response {
    let realm = state.base_url().trim_end_matches('/');
    let header_value = format!(
        "Bearer realm=\"{realm}\",service=\"publaryn\",scope=\"{scope}\""
    );

    let mut response = oci_error_response(StatusCode::UNAUTHORIZED, "UNAUTHORIZED", message, None);
    let header = HeaderValue::from_str(&header_value).unwrap_or_else(|_| {
        HeaderValue::from_static("Bearer realm=\"publaryn\",service=\"publaryn\"")
    });
    response.headers_mut().insert(WWW_AUTHENTICATE, header);
    response
}

pub async fn authenticate_optional<S: OciAppState>(
    state: &S,
    headers: &HeaderMap,
) -> Result<Option<OciIdentity>, AuthFailure> {
    let Some(token) = extract_bearer_token(headers)? else {
        return Ok(None);
    };

    authenticate_token(state, &token).await.map(Some)
}

pub async fn authenticate_required<S: OciAppState>(
    state: &S,
    headers: &HeaderMap,
) -> Result<OciIdentity, AuthFailure> {
    let Some(token) = extract_bearer_token(headers)? else {
        return Err(AuthFailure::Missing);
    };

    authenticate_token(state, &token).await
}

pub fn has_scope(identity: &OciIdentity, scope: &str) -> bool {
    identity.scopes.iter().any(|candidate| candidate == scope)
}

fn extract_bearer_token(headers: &HeaderMap) -> Result<Option<String>, AuthFailure> {
    let Some(authorization) = headers.get(AUTHORIZATION) else {
        return Ok(None);
    };

    let authorization = authorization
        .to_str()
        .map_err(|_| AuthFailure::Invalid("Authorization header is not valid UTF-8".into()))?;
    let token = authorization
        .strip_prefix("Bearer ")
        .or_else(|| authorization.strip_prefix("bearer "))
        .ok_or_else(|| AuthFailure::Invalid("Authorization header must use the Bearer scheme".into()))?
        .trim();

    if token.is_empty() {
        return Err(AuthFailure::Invalid(
            "Authorization header must include a non-empty bearer token".into(),
        ));
    }

    Ok(Some(token.to_owned()))
}

async fn authenticate_token<S: OciAppState>(
    state: &S,
    token: &str,
) -> Result<OciIdentity, AuthFailure> {
    if token.starts_with("pub_") {
        let token_hash = publaryn_core::security::hash_token(token);
        let row = sqlx::query(
            "SELECT id, user_id, scopes, expires_at, kind \
             FROM tokens \
             WHERE token_hash = $1 AND is_revoked = false",
        )
        .bind(&token_hash)
        .fetch_optional(state.db())
        .await
        .map_err(|_| AuthFailure::Invalid("Failed to validate API token".into()))?
        .ok_or_else(|| AuthFailure::Invalid("Invalid or revoked API token".into()))?;

        let token_kind: String = row.try_get("kind").unwrap_or_default();
        if token_kind == "oidc_derived" {
            return Err(AuthFailure::Invalid(
                "OIDC-derived tokens are not valid for OCI operations".into(),
            ));
        }

        let expires_at = row
            .try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("expires_at")
            .unwrap_or(None);
        if expires_at.is_some_and(|expires_at| expires_at <= Utc::now()) {
            return Err(AuthFailure::Invalid("API token has expired".into()));
        }

        let user_id = row
            .try_get::<Option<Uuid>, _>("user_id")
            .unwrap_or(None)
            .ok_or_else(|| AuthFailure::Invalid("API token is not associated with a user".into()))?;
        let token_id: Option<Uuid> = row.try_get("id").ok();
        let scopes: Vec<String> = row.try_get("scopes").unwrap_or_default();

        if let Some(token_id) = token_id {
            let _ = sqlx::query("UPDATE tokens SET last_used_at = NOW() WHERE id = $1")
                .bind(token_id)
                .execute(state.db())
                .await;
        }

        return Ok(OciIdentity {
            user_id,
            token_id,
            scopes,
        });
    }

    let claims = publaryn_auth::validate_token(token, state.jwt_secret(), state.jwt_issuer())
        .map_err(|_| AuthFailure::Invalid("Invalid or expired bearer token".into()))?;
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AuthFailure::Invalid("Bearer token subject is not a valid UUID".into()))?;
    let token_id = Uuid::parse_str(&claims.jti).ok();

    Ok(OciIdentity {
        user_id,
        token_id,
        scopes: claims.scopes,
    })
}
