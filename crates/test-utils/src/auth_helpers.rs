use publaryn_auth::token::create_token;
use uuid::Uuid;

/// Create a JWT token for a test user, suitable for use as `Authorization: Bearer <jwt>`.
pub fn create_test_jwt(
    user_id: Uuid,
    scopes: &[&str],
    secret: &str,
    issuer: &str,
) -> String {
    let token_id = Uuid::new_v4();
    let scope_strings: Vec<String> = scopes.iter().map(|s| (*s).to_owned()).collect();
    create_token(user_id, token_id, scope_strings, secret, 3600, issuer)
        .expect("JWT creation should succeed in tests")
}

/// Convenience: format a Bearer authorization header value.
pub fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}
