use publaryn_auth::{hash_password, verify_password};
use publaryn_auth::oidc::{TrustedIssuer, TrustedPublishingClaims, assert_trusted_issuer};
use publaryn_auth::session::Session;
use publaryn_auth::token::{create_token, validate_token};
use uuid::Uuid;

// ══════════════════════════════════════════════════════════════════════════════
// Password hashing
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_hash_and_verify_password() {
    let password = "super_secure_password_123";
    let hash = hash_password(password).expect("hashing failed");
    assert!(verify_password(password, &hash).expect("verify failed"));
    assert!(!verify_password("wrong_password", &hash).expect("verify failed"));
}

#[test]
fn test_hash_produces_unique_salts() {
    let h1 = hash_password("same_password").unwrap();
    let h2 = hash_password("same_password").unwrap();
    assert_ne!(h1, h2, "Each hash should use a unique salt");
}

#[test]
fn test_verify_empty_password_does_not_match() {
    let hash = hash_password("real_password").unwrap();
    assert!(!verify_password("", &hash).unwrap());
}

#[test]
fn test_verify_against_malformed_hash_fails() {
    let result = verify_password("any", "not-a-valid-hash");
    assert!(result.is_err());
}

// ══════════════════════════════════════════════════════════════════════════════
// JWT tokens
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_create_and_validate_token() {
    let user_id = Uuid::new_v4();
    let token_id = Uuid::new_v4();
    let secret = "test_secret_at_least_32_chars_long_!";
    let issuer = "https://publaryn.example.com";

    let jwt = create_token(
        user_id,
        token_id,
        vec!["read:packages".to_owned()],
        secret,
        3600,
        issuer,
    )
    .expect("token creation failed");

    let claims = validate_token(&jwt, secret, issuer).expect("validation failed");
    assert_eq!(claims.sub, user_id.to_string());
    assert_eq!(claims.jti, token_id.to_string());
    assert!(claims.scopes.contains(&"read:packages".to_owned()));
}

#[test]
fn test_validate_token_wrong_secret_fails() {
    let user_id = Uuid::new_v4();
    let token_id = Uuid::new_v4();
    let issuer = "https://publaryn.example.com";

    let jwt = create_token(
        user_id,
        token_id,
        vec![],
        "correct_secret_at_least_32_chars_!",
        3600,
        issuer,
    )
    .expect("token creation failed");

    let result = validate_token(&jwt, "wrong_secret_at_least_32_chars_!!!", issuer);
    assert!(result.is_err());
}

#[test]
fn test_validate_expired_token_fails() {
    let user_id = Uuid::new_v4();
    let token_id = Uuid::new_v4();
    let secret = "test_secret_at_least_32_chars_long_!";
    let issuer = "https://publaryn.example.com";

    let jwt = create_token(user_id, token_id, vec![], secret, -3600, issuer)
        .expect("token creation failed");

    let result = validate_token(&jwt, secret, issuer);
    assert!(result.is_err());
}

#[test]
fn test_validate_token_wrong_issuer_fails() {
    let user_id = Uuid::new_v4();
    let token_id = Uuid::new_v4();
    let secret = "test_secret_at_least_32_chars_long_!";

    let jwt = create_token(
        user_id,
        token_id,
        vec![],
        secret,
        3600,
        "https://issuer-a.example.com",
    )
    .expect("token creation failed");

    let result = validate_token(&jwt, secret, "https://issuer-b.example.com");
    assert!(result.is_err());
}

#[test]
fn test_token_claims_contain_correct_scopes() {
    let secret = "test_secret_at_least_32_chars_long_!";
    let issuer = "https://publaryn.example.com";
    let scopes = vec!["packages:write".into(), "tokens:read".into()];

    let jwt = create_token(Uuid::new_v4(), Uuid::new_v4(), scopes.clone(), secret, 3600, issuer)
        .unwrap();

    let claims = validate_token(&jwt, secret, issuer).unwrap();
    assert_eq!(claims.scopes, scopes);
}

#[test]
fn test_token_iat_is_before_exp() {
    let secret = "test_secret_at_least_32_chars_long_!";
    let issuer = "https://publaryn.example.com";
    let jwt = create_token(Uuid::new_v4(), Uuid::new_v4(), vec![], secret, 7200, issuer).unwrap();
    let claims = validate_token(&jwt, secret, issuer).unwrap();
    assert!(claims.iat < claims.exp);
    assert!((claims.exp - claims.iat - 7200).abs() <= 2); // allow 2s clock skew
}

// ══════════════════════════════════════════════════════════════════════════════
// Session
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_session_new_is_active_and_not_expired() {
    let session = Session::new(Uuid::new_v4(), 3600, None, None);
    assert!(session.is_active);
    assert!(!session.is_expired());
}

#[test]
fn test_session_with_metadata() {
    let session = Session::new(
        Uuid::new_v4(),
        3600,
        Some("127.0.0.1".into()),
        Some("Mozilla/5.0".into()),
    );
    assert_eq!(session.ip_address.as_deref(), Some("127.0.0.1"));
    assert_eq!(session.user_agent.as_deref(), Some("Mozilla/5.0"));
}

#[test]
fn test_session_zero_ttl_is_immediately_expired() {
    let session = Session::new(Uuid::new_v4(), 0, None, None);
    // With 0 TTL, expires_at == created_at, so is_expired() should be true (>= comparison)
    assert!(session.is_expired());
}

// ══════════════════════════════════════════════════════════════════════════════
// Trusted publishing: TrustedIssuer
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_trusted_issuer_from_github_url() {
    let issuer = TrustedIssuer::from_issuer_url("https://token.actions.githubusercontent.com");
    assert_eq!(issuer, TrustedIssuer::GitHubActions);
}

#[test]
fn test_trusted_issuer_from_gitlab_url() {
    let issuer = TrustedIssuer::from_issuer_url("https://gitlab.com");
    assert_eq!(issuer, TrustedIssuer::GitLabCi);
}

#[test]
fn test_trusted_issuer_from_azure_url() {
    let issuer = TrustedIssuer::from_issuer_url("https://vstoken.dev.azure.com");
    assert_eq!(issuer, TrustedIssuer::AzureDevOps);
}

#[test]
fn test_trusted_issuer_custom_url() {
    let issuer = TrustedIssuer::from_issuer_url("https://my-oidc.example.com");
    assert!(matches!(issuer, TrustedIssuer::Custom(_)));
}

#[test]
fn test_trusted_issuer_discovery_urls() {
    assert!(TrustedIssuer::GitHubActions
        .discovery_url()
        .contains("token.actions.githubusercontent.com"));
    assert!(TrustedIssuer::GitLabCi
        .discovery_url()
        .contains("gitlab.com"));
    assert!(TrustedIssuer::AzureDevOps
        .discovery_url()
        .contains("vstoken.dev.azure.com"));
    assert_eq!(
        TrustedIssuer::Custom("https://custom.example.com".into()).discovery_url(),
        "https://custom.example.com/.well-known/openid-configuration"
    );
}

#[test]
fn test_assert_trusted_issuer_allowed() {
    let allowed = vec![TrustedIssuer::GitHubActions];
    assert!(assert_trusted_issuer(
        "https://token.actions.githubusercontent.com",
        &allowed
    )
    .is_ok());
}

#[test]
fn test_assert_trusted_issuer_rejected() {
    let allowed = vec![TrustedIssuer::GitHubActions];
    assert!(assert_trusted_issuer("https://evil.example.com", &allowed).is_err());
}

// ══════════════════════════════════════════════════════════════════════════════
// TrustedPublishingClaims
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_trusted_publishing_claims_expires_at() {
    let claims = TrustedPublishingClaims {
        iss: "https://token.actions.githubusercontent.com".into(),
        sub: "repo:org/repo:ref:refs/heads/main".into(),
        jti: Some("abc-123".into()),
        exp: 1_700_000_000,
        repository: Some("org/repo".into()),
        repository_owner: Some("org".into()),
        repository_owner_id: Some("12345".into()),
        workflow_ref: None,
        job_workflow_ref: None,
        r#ref: None,
        environment: None,
    };

    let dt = claims.expires_at().expect("should parse timestamp");
    assert_eq!(dt.timestamp(), 1_700_000_000);
}
