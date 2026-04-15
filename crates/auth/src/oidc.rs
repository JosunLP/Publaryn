use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, TimeZone, Utc};
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use once_cell::sync::Lazy;
use publaryn_core::error::{Error, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use thiserror::Error as ThisError;

static OIDC_HTTP_CLIENT: Lazy<Client> = Lazy::new(|| {
    Client::builder()
        .timeout(Duration::from_secs(5))
        .user_agent("publaryn-oidc/0.1")
        .build()
        .expect("OIDC HTTP client should build")
});

pub const TRUSTED_PUBLISHING_TOKEN_TTL_SECONDS: i64 = 900;

/// OIDC discovery document (subset).
#[derive(Debug, Deserialize)]
pub struct OidcDiscovery {
    pub issuer: String,
    pub jwks_uri: String,
    pub token_endpoint: Option<String>,
    pub userinfo_endpoint: Option<String>,
}

/// Claims extracted from an OIDC ID token for trusted publishing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustedPublishingClaims {
    /// Issuer URL (e.g. `https://token.actions.githubusercontent.com`).
    pub iss: String,
    /// Subject — CI job identifier.
    pub sub: String,
    /// JWT ID for replay protection.
    pub jti: Option<String>,
    /// JWT expiration timestamp.
    pub exp: i64,
    /// Repository (e.g. `org/repo`).
    pub repository: Option<String>,
    /// Repository owner (for example a GitHub organization or username).
    pub repository_owner: Option<String>,
    /// Stable repository owner identifier when provided by the issuer.
    pub repository_owner_id: Option<String>,
    /// Workflow ref.
    pub workflow_ref: Option<String>,
    /// Reusable workflow ref when available.
    pub job_workflow_ref: Option<String>,
    /// Source control ref when available.
    pub r#ref: Option<String>,
    /// Environment.
    pub environment: Option<String>,
}

impl TrustedPublishingClaims {
    pub fn expires_at(&self) -> Option<DateTime<Utc>> {
        Utc.timestamp_opt(self.exp, 0).single()
    }
}

#[derive(Debug, ThisError)]
pub enum TrustedPublishingError {
    #[error("malformed JWT")]
    MalformedJwt,

    #[error("unknown trusted publishing issuer")]
    UnknownIssuer,

    #[error("{0}")]
    InvalidToken(String),

    #[error("{0}")]
    Internal(String),
}

#[derive(Debug, Deserialize)]
struct RsaJwkSet {
    keys: Vec<RsaJwk>,
}

#[derive(Debug, Deserialize)]
struct RsaJwk {
    kid: Option<String>,
    kty: String,
    n: Option<String>,
    e: Option<String>,
}

/// Supported OIDC trusted publishing issuers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustedIssuer {
    GitHubActions,
    GitLabCi,
    AzureDevOps,
    Custom(String),
}

impl TrustedIssuer {
    pub fn discovery_url(&self) -> String {
        match self {
            TrustedIssuer::GitHubActions => {
                "https://token.actions.githubusercontent.com/.well-known/openid-configuration"
                    .to_owned()
            }
            TrustedIssuer::GitLabCi => {
                "https://gitlab.com/.well-known/openid-configuration".to_owned()
            }
            TrustedIssuer::AzureDevOps => {
                "https://vstoken.dev.azure.com/.well-known/openid-configuration".to_owned()
            }
            TrustedIssuer::Custom(url) => {
                format!("{url}/.well-known/openid-configuration")
            }
        }
    }

    /// Parse an issuer URL into a known variant.
    pub fn from_issuer_url(url: &str) -> Self {
        match url {
            s if s.contains("token.actions.githubusercontent.com") => {
                TrustedIssuer::GitHubActions
            }
            s if s.contains("gitlab.com") => TrustedIssuer::GitLabCi,
            s if s.contains("vstoken.dev.azure.com") => TrustedIssuer::AzureDevOps,
            other => TrustedIssuer::Custom(other.to_owned()),
        }
    }
}

/// Verify that the issuer is in the allow-list.
pub fn assert_trusted_issuer(issuer: &str, allowed: &[TrustedIssuer]) -> Result<()> {
    let parsed = TrustedIssuer::from_issuer_url(issuer);
    if allowed.contains(&parsed) {
        Ok(())
    } else {
        Err(Error::Unauthorized(format!(
            "OIDC issuer '{issuer}' is not trusted"
        )))
    }
}

pub async fn verify_trusted_publishing_token(
    token: &str,
    audience: &str,
) -> std::result::Result<TrustedPublishingClaims, TrustedPublishingError> {
    let unverified_claims = decode_unverified_claims(token)?;
    let issuer = string_claim(&unverified_claims, "iss").ok_or(TrustedPublishingError::MalformedJwt)?;

    assert_trusted_issuer(
        issuer,
        &[
            TrustedIssuer::GitHubActions,
            TrustedIssuer::GitLabCi,
            TrustedIssuer::AzureDevOps,
        ],
    )
    .map_err(|_| TrustedPublishingError::UnknownIssuer)?;

    let discovery = fetch_discovery_document(issuer).await?;
    if discovery.issuer != issuer {
        return Err(TrustedPublishingError::InvalidToken(
            "the issuer discovery document does not match the presented issuer".into(),
        ));
    }

    let header = decode_header(token).map_err(|_| TrustedPublishingError::MalformedJwt)?;
    let algorithm = match header.alg {
        Algorithm::RS256 | Algorithm::RS384 | Algorithm::RS512 => header.alg,
        _ => {
            return Err(TrustedPublishingError::InvalidToken(
                "unsupported OIDC signing algorithm".into(),
            ))
        }
    };

    let decoding_key = fetch_decoding_key(&discovery.jwks_uri, header.kid.as_deref()).await?;

    let mut validation = Validation::new(algorithm);
    validation.set_issuer(&[issuer]);
    validation.set_audience(&[audience]);
    validation.validate_nbf = true;
    validation.leeway = 30;

    let decoded = decode::<Value>(token, &decoding_key, &validation).map_err(|_| {
        TrustedPublishingError::InvalidToken("malformed or invalid token".into())
    })?;

    serde_json::from_value::<TrustedPublishingClaims>(decoded.claims).map_err(|_| {
        TrustedPublishingError::InvalidToken(
            "the trusted publishing token is missing required claims".into(),
        )
    })
}

async fn fetch_discovery_document(
    issuer: &str,
) -> std::result::Result<OidcDiscovery, TrustedPublishingError> {
    let discovery_url = TrustedIssuer::from_issuer_url(issuer).discovery_url();
    let response = OIDC_HTTP_CLIENT
        .get(&discovery_url)
        .send()
        .await
        .map_err(|error| {
            TrustedPublishingError::Internal(format!(
                "failed to fetch the OIDC discovery document: {error}",
            ))
        })?;

    if !response.status().is_success() {
        return Err(TrustedPublishingError::Internal(format!(
            "the OIDC discovery endpoint returned unexpected status {}",
            response.status(),
        )));
    }

    response.json::<OidcDiscovery>().await.map_err(|error| {
        TrustedPublishingError::Internal(format!(
            "failed to decode the OIDC discovery document: {error}",
        ))
    })
}

async fn fetch_decoding_key(
    jwks_uri: &str,
    key_id: Option<&str>,
) -> std::result::Result<DecodingKey, TrustedPublishingError> {
    let response = OIDC_HTTP_CLIENT
        .get(jwks_uri)
        .send()
        .await
        .map_err(|error| {
            TrustedPublishingError::Internal(format!(
                "failed to fetch the OIDC signing keys: {error}",
            ))
        })?;

    if !response.status().is_success() {
        return Err(TrustedPublishingError::Internal(format!(
            "the OIDC signing key endpoint returned unexpected status {}",
            response.status(),
        )));
    }

    let jwks = response.json::<RsaJwkSet>().await.map_err(|error| {
        TrustedPublishingError::Internal(format!(
            "failed to decode the OIDC signing keys: {error}",
        ))
    })?;

    let key = select_rsa_key(&jwks, key_id)?;
    let modulus = key.n.as_deref().ok_or_else(|| {
        TrustedPublishingError::InvalidToken(
            "the OIDC signing key is missing its RSA modulus".into(),
        )
    })?;
    let exponent = key.e.as_deref().ok_or_else(|| {
        TrustedPublishingError::InvalidToken(
            "the OIDC signing key is missing its RSA exponent".into(),
        )
    })?;

    DecodingKey::from_rsa_components(modulus, exponent).map_err(|error| {
        TrustedPublishingError::Internal(format!(
            "failed to construct the OIDC decoding key: {error}",
        ))
    })
}

fn select_rsa_key<'a>(
    jwks: &'a RsaJwkSet,
    key_id: Option<&str>,
) -> std::result::Result<&'a RsaJwk, TrustedPublishingError> {
    let rsa_keys = jwks
        .keys
        .iter()
        .filter(|key| key.kty.eq_ignore_ascii_case("RSA"))
        .collect::<Vec<_>>();

    if rsa_keys.is_empty() {
        return Err(TrustedPublishingError::InvalidToken(
            "the issuer did not publish a usable RSA signing key".into(),
        ));
    }

    if let Some(key_id) = key_id {
        return rsa_keys
            .into_iter()
            .find(|key| key.kid.as_deref() == Some(key_id))
            .ok_or_else(|| {
                TrustedPublishingError::InvalidToken(
                    "the trusted publishing token referenced an unknown signing key".into(),
                )
            });
    }

    if rsa_keys.len() == 1 {
        return Ok(rsa_keys[0]);
    }

    Err(TrustedPublishingError::InvalidToken(
        "the trusted publishing token did not identify which signing key was used".into(),
    ))
}

fn decode_unverified_claims(token: &str) -> std::result::Result<Value, TrustedPublishingError> {
    let mut segments = token.split('.');
    let _header = segments.next().ok_or(TrustedPublishingError::MalformedJwt)?;
    let payload = segments.next().ok_or(TrustedPublishingError::MalformedJwt)?;
    let _signature = segments.next().ok_or(TrustedPublishingError::MalformedJwt)?;
    if segments.next().is_some() {
        return Err(TrustedPublishingError::MalformedJwt);
    }

    let payload = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| TrustedPublishingError::MalformedJwt)?;

    serde_json::from_slice(&payload).map_err(|_| TrustedPublishingError::MalformedJwt)
}

fn string_claim<'a>(claims: &'a Value, claim_name: &str) -> Option<&'a str> {
    claims.get(claim_name).and_then(Value::as_str)
}

#[cfg(test)]
mod tests {
    use super::{decode_unverified_claims, TrustedIssuer};
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use serde_json::json;

    #[test]
    fn decode_unverified_claims_extracts_json_payload() {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","typ":"JWT"}"#);
        let payload = URL_SAFE_NO_PAD.encode(
            json!({
                "iss": "https://token.actions.githubusercontent.com",
                "sub": "repo:octo/demo:environment:pypi",
                "exp": 4_102_444_800_i64,
            })
            .to_string(),
        );
        let token = format!("{header}.{payload}.signature");

        let claims = decode_unverified_claims(&token).expect("claims should decode");

        assert_eq!(claims.get("iss").and_then(|value| value.as_str()), Some("https://token.actions.githubusercontent.com"));
    }

    #[test]
    fn decode_unverified_claims_rejects_malformed_tokens() {
        let error = decode_unverified_claims("not-a-jwt").expect_err("malformed token must fail");

        assert_eq!(error.to_string(), "malformed JWT");
    }

    #[test]
    fn trusted_issuer_detection_handles_known_providers() {
        assert_eq!(
            TrustedIssuer::from_issuer_url("https://token.actions.githubusercontent.com"),
            TrustedIssuer::GitHubActions
        );
        assert_eq!(
            TrustedIssuer::from_issuer_url("https://gitlab.com"),
            TrustedIssuer::GitLabCi
        );
        assert_eq!(
            TrustedIssuer::from_issuer_url("https://vstoken.dev.azure.com/example"),
            TrustedIssuer::AzureDevOps
        );
    }
}
