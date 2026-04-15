use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
};
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;

use publaryn_api::{config::Config, router::build_router, state::AppState};

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Build an Axum app backed by the given DB pool.
fn app(pool: PgPool) -> axum::Router {
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let config = Config::test_config(&database_url);
    let state = AppState::new_with_pool(pool, config);
    build_router(state).expect("router should build")
}

/// Parse a response body as JSON.
async fn body_json(resp: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("read body");
    serde_json::from_slice(&bytes).expect("parse JSON")
}

/// Register a user via POST /v1/auth/register and return the JSON response.
async fn register_user(
    app: &axum::Router,
    username: &str,
    email: &str,
    password: &str,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/auth/register")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "username": username,
                "email": email,
                "password": password,
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Login a user via POST /v1/auth/login and return the JWT.
async fn login_user(app: &axum::Router, username: &str, password: &str) -> String {
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "username_or_email": username,
                "password": password,
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "login should succeed");
    let body = body_json(resp).await;
    body["token"].as_str().expect("token field").to_owned()
}

// ══════════════════════════════════════════════════════════════════════════════
// Health
// ══════════════════════════════════════════════════════════════════════════════

#[sqlx::test(migrations = "../../migrations")]
async fn test_health_returns_ok(pool: PgPool) {
    let app = app(pool);
    let req = Request::get("/health").body(Body::empty()).unwrap();
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["status"], "ok");
    assert_eq!(body["service"], "publaryn");
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_readiness_returns_ok_when_db_available(pool: PgPool) {
    let app = app(pool);
    let req = Request::get("/readiness").body(Body::empty()).unwrap();
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["status"], "ready");
}

// ══════════════════════════════════════════════════════════════════════════════
// Auth: Register
// ══════════════════════════════════════════════════════════════════════════════

#[sqlx::test(migrations = "../../migrations")]
async fn test_register_success(pool: PgPool) {
    let app = app(pool);
    let (status, body) = register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["username"], "alice");
    assert_eq!(body["email"], "alice@test.dev");
    assert!(body["id"].as_str().is_some());
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_register_duplicate_username_fails(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let (status, _) = register_user(&app, "alice", "alice2@test.dev", "super_secret_pw!").await;

    assert_eq!(status, StatusCode::CONFLICT);
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_register_short_password_rejected(pool: PgPool) {
    let app = app(pool);
    let (status, body) = register_user(&app, "bob", "bob@test.dev", "short").await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["error"].as_str().unwrap().contains("12 characters"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_register_invalid_username_rejected(pool: PgPool) {
    let app = app(pool);
    let (status, _) = register_user(&app, "a", "a@test.dev", "super_secret_pw!").await;

    assert!(
        status == StatusCode::UNPROCESSABLE_ENTITY || status == StatusCode::BAD_REQUEST,
        "Expected 422 or 400, got {status}"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_register_invalid_email_rejected(pool: PgPool) {
    let app = app(pool);
    let (status, _) = register_user(&app, "charlie", "not-an-email", "super_secret_pw!").await;

    assert!(
        status == StatusCode::UNPROCESSABLE_ENTITY || status == StatusCode::BAD_REQUEST,
        "Expected 422 or 400, got {status}"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Auth: Login
// ══════════════════════════════════════════════════════════════════════════════

#[sqlx::test(migrations = "../../migrations")]
async fn test_login_success(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;

    let token = login_user(&app, "alice", "super_secret_pw!").await;
    assert!(!token.is_empty());
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_login_wrong_password_fails(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;

    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "username_or_email": "alice",
                "password": "wrong_password!!",
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_login_nonexistent_user_fails(pool: PgPool) {
    let app = app(pool);

    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "username_or_email": "nobody",
                "password": "doesnt_matter!!",
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ══════════════════════════════════════════════════════════════════════════════
// Tokens: CRUD
// ══════════════════════════════════════════════════════════════════════════════

#[sqlx::test(migrations = "../../migrations")]
async fn test_create_token(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/tokens")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::from(
            json!({
                "name": "ci-token",
                "scopes": ["tokens:read"],
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = body_json(resp).await;
    assert!(body["token"].as_str().unwrap().starts_with("pub_"));
    assert_eq!(body["name"], "ci-token");
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_list_tokens(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let jwt = login_user(&app, "alice", "super_secret_pw!").await;

    // Create a token first
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/tokens")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::from(
            json!({ "name": "my-tok", "scopes": ["tokens:read"] }).to_string(),
        ))
        .unwrap();
    app.clone().oneshot(req).await.unwrap();

    // List tokens
    let req = Request::builder()
        .method(Method::GET)
        .uri("/v1/tokens")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let tokens = body.as_array().expect("response should be an array");
    assert!(!tokens.is_empty());
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_revoke_token(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let jwt = login_user(&app, "alice", "super_secret_pw!").await;

    // Create a token
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/tokens")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::from(
            json!({ "name": "del-me", "scopes": ["tokens:read"] }).to_string(),
        ))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let body = body_json(resp).await;
    let token_id = body["id"].as_str().unwrap();

    // Revoke
    let req = Request::builder()
        .method(Method::DELETE)
        .uri(format!("/v1/tokens/{token_id}"))
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert!(
        resp.status() == StatusCode::OK || resp.status() == StatusCode::NO_CONTENT,
        "Expected 200 or 204, got {}",
        resp.status()
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Unauthenticated access
// ══════════════════════════════════════════════════════════════════════════════

#[sqlx::test(migrations = "../../migrations")]
async fn test_tokens_endpoint_requires_auth(pool: PgPool) {
    let app = app(pool);

    let req = Request::get("/v1/tokens").body(Body::empty()).unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_create_org_requires_auth(pool: PgPool) {
    let app = app(pool);

    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/orgs")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({ "name": "evil-corp", "slug": "evil-corp" }).to_string(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ══════════════════════════════════════════════════════════════════════════════
// Users: public profile
// ══════════════════════════════════════════════════════════════════════════════

#[sqlx::test(migrations = "../../migrations")]
async fn test_get_user_profile(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;

    let req = Request::get("/v1/users/alice").body(Body::empty()).unwrap();
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["username"], "alice");
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_get_nonexistent_user_returns_404(pool: PgPool) {
    let app = app(pool);

    let req = Request::get("/v1/users/nobody").body(Body::empty()).unwrap();
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ══════════════════════════════════════════════════════════════════════════════
// Organizations
// ══════════════════════════════════════════════════════════════════════════════

#[sqlx::test(migrations = "../../migrations")]
async fn test_create_and_get_org(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let jwt = login_user(&app, "alice", "super_secret_pw!").await;

    // Create org
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/orgs")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::from(
            json!({ "name": "Acme Corp", "slug": "acme-corp" }).to_string(),
        ))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Get org
    let req = Request::get("/v1/orgs/acme-corp")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["slug"], "acme-corp");
}

// ══════════════════════════════════════════════════════════════════════════════
// Non-existent routes return 404
// ══════════════════════════════════════════════════════════════════════════════

#[sqlx::test(migrations = "../../migrations")]
async fn test_unknown_route_returns_404(pool: PgPool) {
    let app = app(pool);

    let req = Request::get("/v1/totally-nonexistent")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
