use publaryn_auth::{hash_password, verify_password};
use publaryn_auth::token::{create_token, validate_token};
use uuid::Uuid;

#[test]
fn test_hash_and_verify_password() {
    let password = "super_secure_password_123";
    let hash = hash_password(password).expect("hashing failed");
    assert!(verify_password(password, &hash).expect("verify failed"));
    assert!(!verify_password("wrong_password", &hash).expect("verify failed"));
}

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
