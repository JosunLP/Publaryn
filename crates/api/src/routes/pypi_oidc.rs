use axum::{
    body::Bytes,
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use chrono::{Duration, Utc};
use serde::Deserialize;
use sqlx::Row;
use uuid::Uuid;

use publaryn_auth::{
    verify_trusted_publishing_token, TrustedPublishingClaims, TrustedPublishingError,
    TRUSTED_PUBLISHING_TOKEN_TTL_SECONDS,
};
use publaryn_core::{error::Error, security};

use crate::{scopes::SCOPE_PACKAGES_WRITE, state::AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/_/oidc/audience", get(oidc_audience))
        .route("/_/oidc/mint-token", post(mint_token))
}

#[derive(Debug, Deserialize)]
struct MintTokenRequest {
    token: String,
}

#[derive(Debug, Clone)]
struct TrustedPublisherMatch {
    trusted_publisher_id: Uuid,
    package_id: Uuid,
    package_name: String,
    repository_id: Uuid,
    repository_slug: String,
    created_by: Uuid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PublisherSelectionError {
    NoMatch,
    Ambiguous,
}

async fn oidc_audience(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "audience": trusted_publishing_audience(&state.config.server.base_url),
    }))
}

async fn mint_token(
    State(state): State<AppState>,
    body: Bytes,
) -> (StatusCode, Json<serde_json::Value>) {
    let payload = match parse_mint_token_request(&body) {
        Ok(payload) => payload,
        Err(description) => return token_error_response(StatusCode::UNPROCESSABLE_ENTITY, "invalid-payload", description),
    };

    let audience = trusted_publishing_audience(&state.config.server.base_url);
    let claims = match verify_trusted_publishing_token(&payload.token, &audience).await {
        Ok(claims) => claims,
        Err(TrustedPublishingError::MalformedJwt) => {
            return token_error_response(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid-payload",
                "malformed JWT",
            )
        }
        Err(TrustedPublishingError::UnknownIssuer) => {
            return token_error_response(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid-payload",
                "unknown trusted publishing issuer",
            )
        }
        Err(TrustedPublishingError::InvalidToken(description)) => {
            return token_error_response(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid-token",
                description,
            )
        }
        Err(TrustedPublishingError::Internal(description)) => {
            tracing::warn!(error = %description, "Trusted publishing verification failed");
            return token_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal-error",
                "trusted publishing verification failed",
            );
        }
    };

    let jwt_id = match claims.jti.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
        Some(jwt_id) => jwt_id.to_owned(),
        None => {
            return token_error_response(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid-token",
                "valid token, but missing jti claim",
            )
        }
    };

    let oidc_expires_at = match claims.expires_at() {
        Some(expires_at) => expires_at,
        None => {
            return token_error_response(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid-token",
                "valid token, but exp could not be interpreted",
            )
        }
    };

    let publisher_matches = match load_matching_trusted_publishers(&state, &claims).await {
        Ok(publisher_matches) => publisher_matches,
        Err(error) => {
            tracing::warn!(error = %error, "Failed to load trusted publishers for PyPI OIDC exchange");
            return token_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal-error",
                "failed to resolve trusted publisher configuration",
            );
        }
    };

    let publisher = match select_single_trusted_publisher(publisher_matches) {
        Ok(publisher) => publisher,
        Err(PublisherSelectionError::NoMatch) => {
            return token_error_response(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid-publisher",
                "valid token, but no corresponding PyPI trusted publisher is configured",
            )
        }
        Err(PublisherSelectionError::Ambiguous) => {
            return token_error_response(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid-publisher",
                "valid token, but multiple PyPI trusted publishers matched it; narrow the publisher constraints before using trusted publishing",
            )
        }
    };

    let raw_token = format!("pub_{}", security::generate_random_token(32));
    let token_hash = security::hash_token(&raw_token);
    let token_id = Uuid::new_v4();
    let expires_at = Utc::now() + Duration::seconds(TRUSTED_PUBLISHING_TOKEN_TTL_SECONDS);
    let token_name = format!("PyPI trusted publisher: {}", publisher.package_name);
    let scopes = vec![SCOPE_PACKAGES_WRITE.to_owned()];

    let mut tx = match state.db.begin().await {
        Ok(tx) => tx,
        Err(error) => {
            tracing::warn!(error = %error, "Failed to start trusted publishing token mint transaction");
            return token_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal-error",
                "failed to mint a trusted publishing token",
            );
        }
    };

    let replay_insert = match sqlx::query(
        "INSERT INTO oidc_token_replays (issuer, jwt_id, expires_at, created_at) \
         VALUES ($1, $2, $3, NOW()) \
         ON CONFLICT DO NOTHING",
    )
    .bind(&claims.iss)
    .bind(&jwt_id)
    .bind(oidc_expires_at)
    .execute(&mut *tx)
    .await
    {
        Ok(result) => result,
        Err(error) => {
            tracing::warn!(error = %error, "Failed to persist OIDC replay protection record");
            return token_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal-error",
                "failed to mint a trusted publishing token",
            );
        }
    };

    if replay_insert.rows_affected() == 0 {
        tx.rollback().await.ok();
        return token_error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid-reuse-token",
            "invalid token: already used",
        );
    }

    if let Err(error) = sqlx::query(
        "INSERT INTO tokens (id, kind, prefix, token_hash, name, user_id, package_id, repository_id, scopes, expires_at, is_revoked, created_at) \
         VALUES ($1, 'oidc_derived', 'pub_', $2, $3, $4, $5, $6, $7, $8, false, NOW())",
    )
    .bind(token_id)
    .bind(&token_hash)
    .bind(&token_name)
    .bind(publisher.created_by)
    .bind(publisher.package_id)
    .bind(publisher.repository_id)
    .bind(&scopes)
    .bind(expires_at)
    .execute(&mut *tx)
    .await
    {
        tracing::warn!(error = %error, "Failed to insert trusted publishing token");
        return token_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal-error",
            "failed to mint a trusted publishing token",
        );
    }

    if let Err(error) = sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_user_id, target_package_id, metadata, occurred_at) \
         VALUES ($1, 'token_create', $2, NULL, $2, $3, $4, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(publisher.created_by)
    .bind(publisher.package_id)
    .bind(serde_json::json!({
        "source": "pypi_trusted_publishing",
        "token_id": token_id,
        "kind": "oidc_derived",
        "name": token_name,
        "issuer": claims.iss,
        "subject": claims.sub,
        "repository": claims.repository,
        "workflow_ref": claims.workflow_ref,
        "environment": claims.environment,
        "repository_slug": publisher.repository_slug,
        "trusted_publisher_id": publisher.trusted_publisher_id,
        "expires_at": expires_at,
        "scopes": scopes,
    }))
    .execute(&mut *tx)
    .await
    {
        tracing::warn!(error = %error, "Failed to audit trusted publishing token mint");
        return token_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal-error",
            "failed to mint a trusted publishing token",
        );
    }

    if let Err(error) = tx.commit().await {
        tracing::warn!(error = %error, "Failed to commit trusted publishing token mint transaction");
        return token_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal-error",
            "failed to mint a trusted publishing token",
        );
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "token": raw_token,
            "expires": TRUSTED_PUBLISHING_TOKEN_TTL_SECONDS,
        })),
    )
}

fn parse_mint_token_request(body: &[u8]) -> Result<MintTokenRequest, String> {
    let payload = serde_json::from_slice::<MintTokenRequest>(body).map_err(|_| {
        "The request body must be a JSON object with a string 'token' field".to_owned()
    })?;

    if payload.token.trim().is_empty() {
        return Err("The trusted publishing token payload must not be empty".into());
    }

    Ok(payload)
}

async fn load_matching_trusted_publishers(
    state: &AppState,
    claims: &TrustedPublishingClaims,
) -> Result<Vec<TrustedPublisherMatch>, Error> {
    let rows = sqlx::query(
        "SELECT tp.id, tp.package_id, tp.created_by, p.name AS package_name, r.id AS repository_id, \
                r.slug AS repository_slug \
         FROM trusted_publishers tp \
         JOIN packages p ON p.id = tp.package_id \
         JOIN repositories r ON r.id = p.repository_id \
         WHERE p.ecosystem = 'pypi' \
           AND p.is_archived = false \
           AND tp.issuer = $1 \
           AND tp.subject = $2 \
           AND (tp.repository IS NULL OR tp.repository = $3) \
           AND (tp.workflow_ref IS NULL OR tp.workflow_ref = $4) \
           AND (tp.environment IS NULL OR tp.environment = $5) \
         ORDER BY tp.created_at ASC",
    )
    .bind(&claims.iss)
    .bind(&claims.sub)
    .bind(&claims.repository)
    .bind(&claims.workflow_ref)
    .bind(&claims.environment)
    .fetch_all(&state.db)
    .await
    .map_err(Error::Database)?;

    Ok(rows
        .into_iter()
        .map(|row| TrustedPublisherMatch {
            trusted_publisher_id: row.try_get("id").unwrap_or_else(|_| Uuid::nil()),
            package_id: row.try_get("package_id").unwrap_or_else(|_| Uuid::nil()),
            package_name: row.try_get("package_name").unwrap_or_default(),
            repository_id: row.try_get("repository_id").unwrap_or_else(|_| Uuid::nil()),
            repository_slug: row.try_get("repository_slug").unwrap_or_default(),
            created_by: row.try_get("created_by").unwrap_or_else(|_| Uuid::nil()),
        })
        .collect())
}

fn select_single_trusted_publisher(
    mut matches: Vec<TrustedPublisherMatch>,
) -> Result<TrustedPublisherMatch, PublisherSelectionError> {
    match matches.len() {
        0 => Err(PublisherSelectionError::NoMatch),
        1 => Ok(matches.remove(0)),
        _ => Err(PublisherSelectionError::Ambiguous),
    }
}

fn trusted_publishing_audience(base_url: &str) -> String {
    let authority = base_url
        .split("://")
        .nth(1)
        .unwrap_or(base_url)
        .split('/')
        .next()
        .unwrap_or_default()
        .trim();

    if authority.is_empty() {
        "publaryn".to_owned()
    } else {
        authority.to_owned()
    }
}

fn token_error_response(
    status: StatusCode,
    code: &str,
    description: impl Into<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(serde_json::json!({
            "message": "Token request failed",
            "errors": [
                {
                    "code": code,
                    "description": description.into(),
                }
            ],
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        select_single_trusted_publisher, trusted_publishing_audience, PublisherSelectionError,
        TrustedPublisherMatch,
    };
    use uuid::Uuid;

    fn publisher_match(name: &str) -> TrustedPublisherMatch {
        TrustedPublisherMatch {
            trusted_publisher_id: Uuid::new_v4(),
            package_id: Uuid::new_v4(),
            package_name: name.to_owned(),
            repository_id: Uuid::new_v4(),
            repository_slug: format!("{name}-repo"),
            created_by: Uuid::new_v4(),
        }
    }

    #[test]
    fn trusted_publishing_audience_uses_base_url_authority() {
        assert_eq!(
            trusted_publishing_audience("https://packages.example.test"),
            "packages.example.test"
        );
        assert_eq!(
            trusted_publishing_audience("http://localhost:3000/publaryn"),
            "localhost:3000"
        );
    }

    #[test]
    fn trusted_publishing_audience_falls_back_for_invalid_base_url() {
        assert_eq!(trusted_publishing_audience(""), "publaryn");
    }

    #[test]
    fn select_single_trusted_publisher_rejects_ambiguous_matches() {
        let error = select_single_trusted_publisher(vec![publisher_match("one"), publisher_match("two")])
            .expect_err("multiple matches must be rejected");

        assert_eq!(error, PublisherSelectionError::Ambiguous);
    }

    #[test]
    fn select_single_trusted_publisher_accepts_one_match() {
        let selected = select_single_trusted_publisher(vec![publisher_match("demo")])
            .expect("single match should be accepted");

        assert_eq!(selected.package_name, "demo");
    }
}
