use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use publaryn_core::error::{Error, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// JWT claims embedded in access tokens.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenClaims {
    /// Subject (user ID).
    pub sub: String,
    /// Expiry (Unix timestamp).
    pub exp: i64,
    /// Issued at (Unix timestamp).
    pub iat: i64,
    /// Issuer.
    pub iss: String,
    /// Token ID (jti) — matches the `Token.id` in the database.
    pub jti: String,
    /// Granted scopes.
    pub scopes: Vec<String>,
}

/// Create a signed JWT for the given user and scopes.
pub fn create_token(
    user_id: Uuid,
    token_id: Uuid,
    scopes: Vec<String>,
    secret: &str,
    ttl_seconds: i64,
    issuer: &str,
) -> Result<String> {
    let now = Utc::now();
    let claims = TokenClaims {
        sub: user_id.to_string(),
        exp: (now + Duration::seconds(ttl_seconds)).timestamp(),
        iat: now.timestamp(),
        iss: issuer.to_owned(),
        jti: token_id.to_string(),
        scopes,
    };
    encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| Error::Internal(format!("JWT encoding failed: {e}")))
}

/// Validate and decode a JWT, returning its claims.
pub fn validate_token(token: &str, secret: &str, issuer: &str) -> Result<TokenClaims> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_issuer(&[issuer]);
    decode::<TokenClaims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .map(|d| d.claims)
    .map_err(|e| Error::Unauthorized(format!("Invalid token: {e}")))
}
