use async_trait::async_trait;
use axum::{
    extract::{FromRef, FromRequestParts},
    http::{header::AUTHORIZATION, request::Parts},
};
use chrono::Utc;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use publaryn_core::{error::Error, security};

use crate::{
    error::{ApiError, ApiResult},
    state::AppState,
};

const ORG_ADMIN_ROLES: &[&str] = &["owner", "admin"];
const PACKAGE_WRITE_ROLES: &[&str] = &["owner", "admin", "maintainer", "publisher"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialKind {
    Jwt,
    ApiToken,
}

#[derive(Debug, Clone)]
pub struct AuthenticatedIdentity {
    pub user_id: Uuid,
    pub token_id: Option<Uuid>,
    pub scopes: Vec<String>,
    pub credential_kind: CredentialKind,
}

impl AuthenticatedIdentity {
    pub fn audit_actor_token_id(&self) -> Option<Uuid> {
        match self.credential_kind {
            CredentialKind::Jwt => None,
            CredentialKind::ApiToken => self.token_id,
        }
    }

    pub fn scopes(&self) -> &[String] {
        &self.scopes
    }
}

#[async_trait]
impl<S> FromRequestParts<S> for AuthenticatedIdentity
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let authorization = parts
            .headers
            .get(AUTHORIZATION)
            .ok_or_else(|| ApiError(Error::Unauthorized("Missing Authorization header".into())))?;

        let authorization = authorization
            .to_str()
            .map_err(|_| ApiError(Error::Unauthorized("Invalid Authorization header".into())))?;

        let token = parse_bearer_token(authorization)?;
        let app_state = AppState::from_ref(state);
        authenticate_bearer_token(token, &app_state).await
    }
}

fn parse_bearer_token(header_value: &str) -> ApiResult<&str> {
    let mut parts = header_value.splitn(2, ' ');
    let scheme = parts.next().unwrap_or_default();
    let token = parts.next().unwrap_or_default().trim();

    if !scheme.eq_ignore_ascii_case("bearer") || token.is_empty() {
        return Err(ApiError(Error::Unauthorized(
            "Authorization header must use the Bearer scheme".into(),
        )));
    }

    Ok(token)
}

async fn authenticate_bearer_token(token: &str, state: &AppState) -> ApiResult<AuthenticatedIdentity> {
    if token.starts_with("pub_") {
        authenticate_api_token(token, state).await
    } else {
        authenticate_jwt(token, state)
    }
}

fn authenticate_jwt(token: &str, state: &AppState) -> ApiResult<AuthenticatedIdentity> {
    let claims = publaryn_auth::validate_token(
        token,
        &state.config.auth.jwt_secret,
        &state.config.auth.issuer,
    )
    .map_err(ApiError::from)?;

    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| {
        ApiError(Error::Unauthorized(
            "Token subject is not a valid user identifier".into(),
        ))
    })?;

    let token_id = Uuid::parse_str(&claims.jti).map_err(|_| {
        ApiError(Error::Unauthorized(
            "Token identifier is not a valid UUID".into(),
        ))
    })?;

    Ok(AuthenticatedIdentity {
        user_id,
        token_id: Some(token_id),
        scopes: claims.scopes,
        credential_kind: CredentialKind::Jwt,
    })
}

async fn authenticate_api_token(token: &str, state: &AppState) -> ApiResult<AuthenticatedIdentity> {
    let token_hash = security::hash_token(token);
    let row = sqlx::query(
        "SELECT id, user_id, scopes, expires_at \
         FROM tokens \
         WHERE token_hash = $1 AND is_revoked = false",
    )
    .bind(&token_hash)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| ApiError(Error::Unauthorized("Invalid API token".into())))?;

    let expires_at = row
        .try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("expires_at")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    if expires_at.is_some_and(|value| value <= Utc::now()) {
        return Err(ApiError(Error::Unauthorized("API token has expired".into())));
    }

    let token_id: Uuid = row
        .try_get("id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let user_id = row
        .try_get::<Option<Uuid>, _>("user_id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?
        .ok_or_else(|| {
            ApiError(Error::Unauthorized(
                "API token is not associated with a user account".into(),
            ))
        })?;
    let scopes = row
        .try_get::<Vec<String>, _>("scopes")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    sqlx::query("UPDATE tokens SET last_used_at = NOW() WHERE id = $1")
        .bind(token_id)
        .execute(&state.db)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(AuthenticatedIdentity {
        user_id,
        token_id: Some(token_id),
        scopes,
        credential_kind: CredentialKind::ApiToken,
    })
}

async fn fetch_org_id_by_slug(db: &PgPool, slug: &str) -> ApiResult<Uuid> {
    let row = sqlx::query("SELECT id FROM organizations WHERE slug = $1")
        .bind(slug)
        .fetch_optional(db)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?
        .ok_or_else(|| ApiError(Error::NotFound(format!("Organization '{slug}' not found"))))?;

    row.try_get("id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))
}

async fn actor_has_org_roles(
    db: &PgPool,
    org_id: Uuid,
    actor_user_id: Uuid,
    allowed_roles: &[&str],
) -> ApiResult<bool> {
    let allowed_roles: Vec<String> = allowed_roles.iter().map(|role| (*role).to_owned()).collect();

    sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (\
             SELECT 1 \
             FROM org_memberships \
             WHERE org_id = $1 AND user_id = $2 AND role::text = ANY($3)\
         )",
    )
    .bind(org_id)
    .bind(actor_user_id)
    .bind(&allowed_roles)
    .fetch_one(db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))
}

pub async fn ensure_org_admin_by_id(db: &PgPool, org_id: Uuid, actor_user_id: Uuid) -> ApiResult<()> {
    if actor_has_org_roles(db, org_id, actor_user_id, ORG_ADMIN_ROLES).await? {
        return Ok(());
    }

    Err(ApiError(Error::Forbidden(
        "Organization administration requires owner or admin membership".into(),
    )))
}

pub async fn ensure_org_admin_by_slug(db: &PgPool, slug: &str, actor_user_id: Uuid) -> ApiResult<Uuid> {
    let org_id = fetch_org_id_by_slug(db, slug).await?;
    ensure_org_admin_by_id(db, org_id, actor_user_id).await?;
    Ok(org_id)
}

pub async fn is_platform_admin(db: &PgPool, actor_user_id: Uuid) -> ApiResult<bool> {
    sqlx::query_scalar::<_, bool>(
        "SELECT COALESCE((\
             SELECT is_admin \
             FROM users \
             WHERE id = $1 AND is_active = true\
         ), false)",
    )
    .bind(actor_user_id)
    .fetch_one(db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))
}

pub async fn ensure_platform_admin(db: &PgPool, actor_user_id: Uuid) -> ApiResult<()> {
    if is_platform_admin(db, actor_user_id).await? {
        return Ok(());
    }

    Err(ApiError(Error::Forbidden(
        "This operation requires platform administrator privileges".into(),
    )))
}

pub async fn ensure_repository_write_access(
    db: &PgPool,
    slug: &str,
    actor_user_id: Uuid,
) -> ApiResult<Uuid> {
    let row = sqlx::query(
        "SELECT id, owner_user_id, owner_org_id \
         FROM repositories \
         WHERE slug = $1",
    )
    .bind(slug)
    .fetch_optional(db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| ApiError(Error::NotFound(format!("Repository '{slug}' not found"))))?;

    let repository_id: Uuid = row
        .try_get("id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let owner_user_id = row
        .try_get::<Option<Uuid>, _>("owner_user_id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let owner_org_id = row
        .try_get::<Option<Uuid>, _>("owner_org_id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    if owner_user_id == Some(actor_user_id) {
        return Ok(repository_id);
    }

    if let Some(owner_org_id) = owner_org_id {
        if actor_has_org_roles(db, owner_org_id, actor_user_id, ORG_ADMIN_ROLES).await? {
            return Ok(repository_id);
        }
    }

    Err(ApiError(Error::Forbidden(
        "You do not have permission to modify this repository".into(),
    )))
}

pub async fn ensure_package_write_access(
    db: &PgPool,
    ecosystem: &str,
    normalized_name: &str,
    actor_user_id: Uuid,
) -> ApiResult<Uuid> {
    let row = sqlx::query(
        "SELECT id, owner_user_id, owner_org_id \
         FROM packages \
         WHERE ecosystem = $1 AND normalized_name = $2",
    )
    .bind(ecosystem)
    .bind(normalized_name)
    .fetch_optional(db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| {
        ApiError(Error::NotFound(format!(
            "Package '{normalized_name}' not found in ecosystem '{ecosystem}'"
        )))
    })?;

    let package_id: Uuid = row
        .try_get("id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let owner_user_id = row
        .try_get::<Option<Uuid>, _>("owner_user_id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let owner_org_id = row
        .try_get::<Option<Uuid>, _>("owner_org_id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    if owner_user_id == Some(actor_user_id) {
        return Ok(package_id);
    }

    if let Some(owner_org_id) = owner_org_id {
        if actor_has_org_roles(db, owner_org_id, actor_user_id, PACKAGE_WRITE_ROLES).await? {
            return Ok(package_id);
        }
    }

    Err(ApiError(Error::Forbidden(
        "You do not have permission to modify this package".into(),
    )))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{extract::FromRequestParts, http::{header::AUTHORIZATION, Request}};
    use sqlx::postgres::PgPoolOptions;
    use uuid::Uuid;

    use publaryn_search::index::MeilisearchIndex;

    use crate::{
        config::{
            AuthConfig, Config, DatabaseConfig, RedisConfig, SearchConfig, ServerConfig,
            StorageConfig,
        },
        state::AppState,
    };

    use super::{AuthenticatedIdentity, CredentialKind};

    fn test_state() -> AppState {
        AppState {
            db: PgPoolOptions::new()
                .connect_lazy("postgres://publaryn:publaryn_dev@localhost/publaryn")
                .expect("lazy postgres pool"),
            config: Arc::new(Config {
                server: ServerConfig {
                    bind_address: "127.0.0.1:3000".into(),
                    base_url: "http://localhost:3000".into(),
                },
                database: DatabaseConfig {
                    url: "postgres://publaryn:publaryn_dev@localhost/publaryn".into(),
                    max_connections: 5,
                },
                auth: AuthConfig {
                    jwt_secret: "test_secret_at_least_32_chars_long_!".into(),
                    jwt_ttl_seconds: 3600,
                    session_ttl_seconds: 86400,
                    issuer: "https://publaryn.example.com".into(),
                },
                storage: StorageConfig {
                    endpoint: "http://localhost:9000".into(),
                    bucket: "publaryn-artifacts".into(),
                    access_key: "minioadmin".into(),
                    secret_key: "minioadmin123".into(),
                    region: "us-east-1".into(),
                },
                search: SearchConfig {
                    url: "http://localhost:7700".into(),
                    api_key: None,
                },
                redis: RedisConfig {
                    url: "redis://localhost:6379".into(),
                },
            }),
            search: Arc::new(MeilisearchIndex::new("http://localhost:7700", None)),
        }
    }

    #[tokio::test]
    async fn extracts_identity_from_jwt_bearer_token() {
        let state = test_state();
        let user_id = Uuid::new_v4();
        let token_id = Uuid::new_v4();
        let jwt = publaryn_auth::create_token(
            user_id,
            token_id,
            vec!["write:packages".into()],
            &state.config.auth.jwt_secret,
            state.config.auth.jwt_ttl_seconds,
            &state.config.auth.issuer,
        )
        .expect("jwt should be created");

        let (mut parts, _) = Request::builder()
            .header(AUTHORIZATION, format!("Bearer {jwt}"))
            .body(())
            .expect("request should build")
            .into_parts();

        let identity = AuthenticatedIdentity::from_request_parts(&mut parts, &state)
            .await
            .expect("identity should be extracted");

        assert_eq!(identity.user_id, user_id);
        assert_eq!(identity.token_id, Some(token_id));
        assert_eq!(identity.credential_kind, CredentialKind::Jwt);
        assert_eq!(identity.scopes(), ["write:packages"]);
        assert!(identity.scopes().iter().any(|scope| scope == "write:packages"));
        assert_eq!(identity.audit_actor_token_id(), None);
    }

    #[tokio::test]
    async fn rejects_requests_without_authorization_header() {
        let state = test_state();
        let (mut parts, _) = Request::builder()
            .body(())
            .expect("request should build")
            .into_parts();

        let error = AuthenticatedIdentity::from_request_parts(&mut parts, &state)
            .await
            .expect_err("missing header must fail");

        assert_eq!(error.0.to_string(), "Unauthorized: Missing Authorization header");
    }
}
