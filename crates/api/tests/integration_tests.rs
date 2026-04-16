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

/// Create an organization via POST /v1/orgs and return the response.
async fn create_org(
    app: &axum::Router,
    jwt: &str,
    name: &str,
    slug: &str,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/orgs")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::from(
            json!({
                "name": name,
                "slug": slug,
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Create a team for an organization and return the response.
async fn create_team(
    app: &axum::Router,
    jwt: &str,
    org_slug: &str,
    name: &str,
    team_slug: &str,
    description: Option<&str>,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::POST)
        .uri(format!("/v1/orgs/{org_slug}/teams"))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::from(
            json!({
                "name": name,
                "slug": team_slug,
                "description": description,
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Add a user as an organization member and return the response.
async fn add_org_member(
    app: &axum::Router,
    jwt: &str,
    org_slug: &str,
    username: &str,
    role: &str,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::POST)
        .uri(format!("/v1/orgs/{org_slug}/members"))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::from(
            json!({
                "username": username,
                "role": role,
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Remove a user from an organization and return the response.
async fn remove_org_member(
    app: &axum::Router,
    jwt: &str,
    org_slug: &str,
    username: &str,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::DELETE)
        .uri(format!("/v1/orgs/{org_slug}/members/{username}"))
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// List organization-scoped audit entries and return the response.
async fn list_org_audit(
    app: &axum::Router,
    jwt: &str,
    org_slug: &str,
    query: Option<&str>,
) -> (StatusCode, Value) {
    let uri = match query {
        Some(query) if !query.trim().is_empty() => {
            format!("/v1/orgs/{org_slug}/audit?{query}")
        }
        _ => format!("/v1/orgs/{org_slug}/audit"),
    };

    let req = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Create a repository and return the response.
async fn create_repository(
    app: &axum::Router,
    jwt: &str,
    name: &str,
    slug: &str,
    owner_org_id: Option<&str>,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/repositories")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::from(
            json!({
                "name": name,
                "slug": slug,
                "kind": "public",
                "visibility": "public",
                "owner_org_id": owner_org_id,
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Create a package and return the response.
async fn create_package(
    app: &axum::Router,
    jwt: &str,
    ecosystem: &str,
    name: &str,
    repository_slug: &str,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/packages")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::from(
            json!({
                "ecosystem": ecosystem,
                "name": name,
                "repository_slug": repository_slug,
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
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

#[sqlx::test(migrations = "../../migrations")]
async fn test_add_org_member_updates_existing_member_role(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, _) = create_org(&app, &jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = add_org_member(&app, &jwt, "acme-corp", "bob", "viewer").await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected add member response: {body}"
    );
    assert_eq!(body["message"], "Member added");

    let (status, body) = add_org_member(&app, &jwt, "acme-corp", "bob", "maintainer").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected role update response: {body}"
    );
    assert_eq!(body["message"], "Member role updated");

    let req = Request::builder()
        .method(Method::GET)
        .uri("/v1/orgs/acme-corp/members")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let members = body["members"]
        .as_array()
        .expect("members response should be an array");

    let bob_memberships = members
        .iter()
        .filter(|member| member["username"] == "bob")
        .collect::<Vec<_>>();

    assert_eq!(
        bob_memberships.len(),
        1,
        "existing memberships should be updated in place; members response: {body}"
    );
    assert_eq!(
        bob_memberships[0]["role"],
        "maintainer",
        "members response: {body}"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_org_admin_can_read_org_audit_logs(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, _) = create_org(&app, &jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = add_org_member(&app, &jwt, "acme-corp", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = list_org_audit(&app, &jwt, "acme-corp", Some("per_page=10")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["page"], 1);
    assert_eq!(body["per_page"], 10);

    let logs = body["logs"]
        .as_array()
        .expect("logs response should be an array");
    let member_add_log = logs
        .iter()
        .find(|log| log["action"] == "org_member_add")
        .expect("org_member_add should be present in org audit log");

    assert_eq!(member_add_log["actor_username"], "alice");
    assert_eq!(member_add_log["target_username"], "bob");
    assert_eq!(member_add_log["metadata"]["username"], "bob");
    assert_eq!(member_add_log["metadata"]["role"], "viewer");
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_org_audit_requires_org_admin_membership(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let owner_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, _) = create_org(&app, &owner_jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = add_org_member(&app, &owner_jwt, "acme-corp", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = list_org_audit(&app, &bob_jwt, "acme-corp", None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(body["error"]
        .as_str()
        .expect("error should be present")
        .contains("owner or admin"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_org_audit_is_isolated_to_requested_org(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    register_user(&app, "charlie", "charlie@test.dev", "super_secret_pw!").await;
    let jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, acme_body) = create_org(&app, &jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let acme_org_id = acme_body["id"].as_str().expect("acme org id");

    let (status, beta_body) = create_org(&app, &jwt, "Beta Corp", "beta-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let beta_org_id = beta_body["id"].as_str().expect("beta org id");

    let (status, _) = add_org_member(&app, &jwt, "acme-corp", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = add_org_member(&app, &jwt, "beta-corp", "charlie", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = list_org_audit(&app, &jwt, "acme-corp", Some("per_page=20")).await;
    assert_eq!(status, StatusCode::OK);

    let logs = body["logs"]
        .as_array()
        .expect("logs response should be an array");
    assert!(logs.iter().all(|log| log["target_org_id"] == acme_org_id));
    assert!(logs.iter().all(|log| log["target_org_id"] != beta_org_id));
    assert!(logs.iter().any(|log| log["metadata"]["username"] == "bob"));
    assert!(logs.iter().all(|log| log["metadata"]["username"] != "charlie"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_org_audit_supports_action_filtering(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, _) = create_org(&app, &jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = add_org_member(&app, &jwt, "acme-corp", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = add_org_member(&app, &jwt, "acme-corp", "bob", "maintainer").await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = list_org_audit(
        &app,
        &jwt,
        "acme-corp",
        Some("action=org_role_change&per_page=10"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let logs = body["logs"]
        .as_array()
        .expect("logs response should be an array");
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0]["action"], "org_role_change");
    assert_eq!(logs[0]["metadata"]["username"], "bob");
    assert_eq!(logs[0]["metadata"]["role"], "maintainer");
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_org_audit_includes_member_removal(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, _) = create_org(&app, &jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = add_org_member(&app, &jwt, "acme-corp", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = remove_org_member(&app, &jwt, "acme-corp", "bob").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["message"], "Member removed");

    let (status, body) = list_org_audit(
        &app,
        &jwt,
        "acme-corp",
        Some("action=org_member_remove&per_page=10"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let logs = body["logs"]
        .as_array()
        .expect("logs response should be an array");
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0]["action"], "org_member_remove");
    assert_eq!(logs[0]["target_username"], "bob");
    assert_eq!(logs[0]["metadata"]["username"], "bob");
    assert_eq!(logs[0]["metadata"]["role"], "viewer");
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_org_audit_includes_ownership_transfer(pool: PgPool) {
    let app = app(pool);
    let (status, _) = register_user(&app, "owner_user", "owner@example.com", "Str0ngP@ssword!").await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = register_user(&app, "target_user", "target@example.com", "Str0ngP@ssword!").await;
    assert_eq!(status, StatusCode::OK);

    let owner_jwt = login_user(&app, "owner_user", "Str0ngP@ssword!").await;

    let (status, _) = create_org(&app, &owner_jwt, "Transfer Org", "transfer-org").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = add_org_member(&app, &owner_jwt, "transfer-org", "target_user", "maintainer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = transfer_ownership(&app, &owner_jwt, "transfer-org", "target_user").await;
    assert_eq!(status, StatusCode::OK, "unexpected ownership transfer response: {body}");

    let (status, body) = list_org_audit(
        &app,
        &owner_jwt,
        "transfer-org",
        Some("action=org_ownership_transfer&per_page=10"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let logs = body["logs"]
        .as_array()
        .expect("logs response should be an array");
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0]["action"], "org_ownership_transfer");
    assert_eq!(logs[0]["target_username"], "target_user");
    assert_eq!(logs[0]["metadata"]["new_owner_username"], "target_user");
    assert_eq!(logs[0]["metadata"]["former_owner_new_role"], "admin");
}

// ══════════════════════════════════════════════════════════════════════════════
// Teams
// ══════════════════════════════════════════════════════════════════════════════

#[sqlx::test(migrations = "../../migrations")]
async fn test_team_crud_roundtrip(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, _) = create_org(&app, &jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = create_team(
        &app,
        &jwt,
        "acme-corp",
        "Release Engineering",
        "release-engineering",
        Some("Owns package publication workflows"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["slug"], "release-engineering");
    assert_eq!(body["name"], "Release Engineering");

    let req = Request::builder()
        .method(Method::GET)
        .uri("/v1/orgs/acme-corp/teams")
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let teams = body["teams"].as_array().expect("teams response should be an array");
    assert_eq!(teams.len(), 1);
    assert_eq!(teams[0]["slug"], "release-engineering");
    assert_eq!(teams[0]["description"], "Owns package publication workflows");

    let req = Request::builder()
        .method(Method::PATCH)
        .uri("/v1/orgs/acme-corp/teams/release-engineering")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::from(
            json!({
                "name": "Release Operations",
                "description": "Coordinates releases and publication",
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["message"], "Team updated");

    let req = Request::builder()
        .method(Method::GET)
        .uri("/v1/orgs/acme-corp/teams")
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let teams = body["teams"].as_array().expect("teams response should be an array");
    assert_eq!(teams[0]["name"], "Release Operations");
    assert_eq!(teams[0]["description"], "Coordinates releases and publication");

    let req = Request::builder()
        .method(Method::DELETE)
        .uri("/v1/orgs/acme-corp/teams/release-engineering")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["message"], "Team deleted");

    let req = Request::builder()
        .method(Method::GET)
        .uri("/v1/orgs/acme-corp/teams")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let teams = body["teams"].as_array().expect("teams response should be an array");
    assert!(teams.is_empty(), "team should be removed after deletion");
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_add_and_remove_team_member(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, _) = create_org(&app, &jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = add_org_member(&app, &jwt, "acme-corp", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_team(
        &app,
        &jwt,
        "acme-corp",
        "Release Engineering",
        "release-engineering",
        Some("Owns package publication workflows"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/orgs/acme-corp/teams/release-engineering/members")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::from(json!({ "username": "bob" }).to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = body_json(resp).await;
    assert_eq!(body["message"], "Team member added");

    let req = Request::builder()
        .method(Method::GET)
        .uri("/v1/orgs/acme-corp/teams/release-engineering/members")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let members = body["members"].as_array().expect("members response should be an array");
    assert_eq!(body["team"]["slug"], "release-engineering");
    assert_eq!(members.len(), 1);
    assert_eq!(members[0]["username"], "bob");

    let req = Request::builder()
        .method(Method::DELETE)
        .uri("/v1/orgs/acme-corp/teams/release-engineering/members/bob")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["message"], "Team member removed");

    let req = Request::builder()
        .method(Method::GET)
        .uri("/v1/orgs/acme-corp/teams/release-engineering/members")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let members = body["members"].as_array().expect("members response should be an array");
    assert!(members.is_empty(), "team member should be removed");
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_add_team_member_requires_org_membership(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "charlie", "charlie@test.dev", "super_secret_pw!").await;
    let jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, _) = create_org(&app, &jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_team(
        &app,
        &jwt,
        "acme-corp",
        "Release Engineering",
        "release-engineering",
        Some("Owns package publication workflows"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/orgs/acme-corp/teams/release-engineering/members")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::from(json!({ "username": "charlie" }).to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body = body_json(resp).await;
    assert!(body["error"]
        .as_str()
        .expect("error should be present")
        .contains("must already belong to the organization"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_team_package_access_roundtrip(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id should be returned");

    let (status, _) = create_repository(
        &app,
        &jwt,
        "Acme Packages",
        "acme-packages",
        Some(org_id),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_package(&app, &jwt, "npm", "acme-widget", "acme-packages").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_team(
        &app,
        &jwt,
        "acme-corp",
        "Release Engineering",
        "release-engineering",
        Some("Owns delegated package publication workflows"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let req = Request::builder()
        .method(Method::PUT)
        .uri("/v1/orgs/acme-corp/teams/release-engineering/package-access/npm/acme-widget")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::from(
            json!({
                "permissions": ["publish", "write_metadata"],
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["message"], "Team package access updated");
    assert_eq!(body["permissions"], json!(["publish", "write_metadata"]));

    let req = Request::builder()
        .method(Method::GET)
        .uri("/v1/orgs/acme-corp/teams/release-engineering/package-access")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let package_access = body["package_access"]
        .as_array()
        .expect("package access response should be an array");
    assert_eq!(body["team"]["slug"], "release-engineering");
    assert_eq!(package_access.len(), 1);
    assert_eq!(package_access[0]["ecosystem"], "npm");
    assert_eq!(package_access[0]["name"], "acme-widget");
    assert_eq!(package_access[0]["permissions"], json!(["publish", "write_metadata"]));

    let req = Request::builder()
        .method(Method::PUT)
        .uri("/v1/orgs/acme-corp/teams/release-engineering/package-access/npm/acme-widget")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::from(json!({ "permissions": ["admin"] }).to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["permissions"], json!(["admin"]));

    let req = Request::builder()
        .method(Method::DELETE)
        .uri("/v1/orgs/acme-corp/teams/release-engineering/package-access/npm/acme-widget")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["message"], "Team package access removed");

    let req = Request::builder()
        .method(Method::GET)
        .uri("/v1/orgs/acme-corp/teams/release-engineering/package-access")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let package_access = body["package_access"]
        .as_array()
        .expect("package access response should be an array");
    assert!(package_access.is_empty(), "package access should be removed");
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_team_package_access_rejects_empty_permissions(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id should be returned");

    let (status, _) = create_repository(
        &app,
        &jwt,
        "Acme Packages",
        "acme-packages",
        Some(org_id),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_package(&app, &jwt, "npm", "acme-widget", "acme-packages").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_team(
        &app,
        &jwt,
        "acme-corp",
        "Release Engineering",
        "release-engineering",
        Some("Owns delegated package publication workflows"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let req = Request::builder()
        .method(Method::PUT)
        .uri("/v1/orgs/acme-corp/teams/release-engineering/package-access/npm/acme-widget")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::from(json!({ "permissions": [] }).to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert!(
        resp.status() == StatusCode::UNPROCESSABLE_ENTITY
            || resp.status() == StatusCode::BAD_REQUEST,
        "Expected 422 or 400, got {}",
        resp.status()
    );
    let body = body_json(resp).await;
    assert!(body["error"]
        .as_str()
        .expect("error should be present")
        .contains("At least one team permission is required"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_team_package_access_rejects_unknown_permissions(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id should be returned");

    let (status, _) = create_repository(
        &app,
        &jwt,
        "Acme Packages",
        "acme-packages",
        Some(org_id),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_package(&app, &jwt, "npm", "acme-widget", "acme-packages").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_team(
        &app,
        &jwt,
        "acme-corp",
        "Release Engineering",
        "release-engineering",
        Some("Owns delegated package publication workflows"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let req = Request::builder()
        .method(Method::PUT)
        .uri("/v1/orgs/acme-corp/teams/release-engineering/package-access/npm/acme-widget")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::from(
            json!({ "permissions": ["superpowers"] }).to_string(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert!(
        resp.status() == StatusCode::UNPROCESSABLE_ENTITY
            || resp.status() == StatusCode::BAD_REQUEST,
        "Expected 422 or 400, got {}",
        resp.status()
    );
    let body = body_json(resp).await;
    assert!(body["error"]
        .as_str()
        .expect("error should be present")
        .contains("Unknown team permission: superpowers"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_team_package_access_rejects_packages_outside_org(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id should be returned");

    let (status, _) = create_repository(
        &app,
        &jwt,
        "Acme Packages",
        "acme-packages",
        Some(org_id),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_repository(
        &app,
        &jwt,
        "Personal Packages",
        "personal-packages",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_package(
        &app,
        &jwt,
        "npm",
        "personal-widget",
        "personal-packages",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_team(
        &app,
        &jwt,
        "acme-corp",
        "Release Engineering",
        "release-engineering",
        Some("Owns delegated package publication workflows"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let req = Request::builder()
        .method(Method::PUT)
        .uri("/v1/orgs/acme-corp/teams/release-engineering/package-access/npm/personal-widget")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::from(json!({ "permissions": ["publish"] }).to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body = body_json(resp).await;
    assert!(body["error"]
        .as_str()
        .expect("error should be present")
        .contains("same organization"));
}

// ══════════════════════════════════════════════════════════════════════════════
// Organization ownership transfer
// ══════════════════════════════════════════════════════════════════════════════

/// Transfer organization ownership and return the response.
async fn transfer_ownership(
    app: &axum::Router,
    jwt: &str,
    org_slug: &str,
    target_username: &str,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::POST)
        .uri(format!("/v1/orgs/{org_slug}/ownership-transfer"))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::from(
            json!({ "username": target_username }).to_string(),
        ))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_ownership_transfer_success(pool: PgPool) {
    let app = app(pool);

    // Register owner and target user
    let (status, _) = register_user(&app, "owner_user", "owner@example.com", "Str0ngP@ssword!").await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = register_user(&app, "target_user", "target@example.com", "Str0ngP@ssword!").await;
    assert_eq!(status, StatusCode::OK);

    let owner_jwt = login_user(&app, "owner_user", "Str0ngP@ssword!").await;

    // Create org (owner_user becomes owner)
    let (status, _) = create_org(&app, &owner_jwt, "Transfer Org", "transfer-org").await;
    assert_eq!(status, StatusCode::CREATED);

    // Add target_user as a maintainer
    let (status, _) = add_org_member(&app, &owner_jwt, "transfer-org", "target_user", "maintainer").await;
    assert_eq!(status, StatusCode::CREATED);

    // Transfer ownership to target_user
    let (status, body) = transfer_ownership(&app, &owner_jwt, "transfer-org", "target_user").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["message"], "Organization ownership transferred");
    assert_eq!(body["new_owner"]["username"], "target_user");
    assert_eq!(body["new_owner"]["role"], "owner");
    assert_eq!(body["previous_owner"]["new_role"], "admin");

    // Verify the new owner can perform owner actions (e.g. transfer again)
    let target_jwt = login_user(&app, "target_user", "Str0ngP@ssword!").await;
    let (status, body) = transfer_ownership(&app, &target_jwt, "transfer-org", "owner_user").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["new_owner"]["username"], "owner_user");
    assert_eq!(body["new_owner"]["role"], "owner");
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_ownership_transfer_to_self_fails(pool: PgPool) {
    let app = app(pool);

    let (status, _) = register_user(&app, "selfowner", "selfowner@example.com", "Str0ngP@ssword!").await;
    assert_eq!(status, StatusCode::OK);
    let jwt = login_user(&app, "selfowner", "Str0ngP@ssword!").await;

    let (status, _) = create_org(&app, &jwt, "Self Org", "self-org").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = transfer_ownership(&app, &jwt, "self-org", "selfowner").await;
    assert!(
        status == StatusCode::UNPROCESSABLE_ENTITY || status == StatusCode::BAD_REQUEST,
        "self-transfer should be rejected, got {status}"
    );
    assert!(body["error"]
        .as_str()
        .expect("error should be present")
        .contains("different"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_ownership_transfer_by_non_owner_fails(pool: PgPool) {
    let app = app(pool);

    let (status, _) = register_user(&app, "real_owner", "real_owner@example.com", "Str0ngP@ssword!").await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = register_user(&app, "admin_user", "admin_user@example.com", "Str0ngP@ssword!").await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = register_user(&app, "bystander", "bystander@example.com", "Str0ngP@ssword!").await;
    assert_eq!(status, StatusCode::OK);

    let owner_jwt = login_user(&app, "real_owner", "Str0ngP@ssword!").await;
    let admin_jwt = login_user(&app, "admin_user", "Str0ngP@ssword!").await;

    let (status, _) = create_org(&app, &owner_jwt, "Guarded Org", "guarded-org").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = add_org_member(&app, &owner_jwt, "guarded-org", "admin_user", "admin").await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = add_org_member(&app, &owner_jwt, "guarded-org", "bystander", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    // Admin attempts to transfer ownership — should be forbidden
    let (status, body) = transfer_ownership(&app, &admin_jwt, "guarded-org", "bystander").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(body["error"]
        .as_str()
        .expect("error should be present")
        .contains("owner"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_ownership_transfer_to_already_owner_fails(pool: PgPool) {
    let app = app(pool);

    // This test verifies that transferring to a user who is already an owner is rejected.
    // Since the API promotes the creator to owner and does not allow adding another owner
    // via add_org_member, we first transfer, then attempt to transfer back to confirm
    // the original owner (now admin) can receive ownership, and then try transferring
    // to someone who is already owner.

    let (status, _) = register_user(&app, "alice_ot", "alice_ot@example.com", "Str0ngP@ssword!").await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = register_user(&app, "bob_ot", "bob_ot@example.com", "Str0ngP@ssword!").await;
    assert_eq!(status, StatusCode::OK);

    let alice_jwt = login_user(&app, "alice_ot", "Str0ngP@ssword!").await;

    let (status, _) = create_org(&app, &alice_jwt, "Dupe Org", "dupe-org").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = add_org_member(&app, &alice_jwt, "dupe-org", "bob_ot", "maintainer").await;
    assert_eq!(status, StatusCode::CREATED);

    // Transfer to bob (succeeds, bob becomes owner)
    let (status, _) = transfer_ownership(&app, &alice_jwt, "dupe-org", "bob_ot").await;
    assert_eq!(status, StatusCode::OK);

    // Now bob is owner. Attempt to transfer to bob again (already owner) should fail.
    let bob_jwt = login_user(&app, "bob_ot", "Str0ngP@ssword!").await;
    let (status, body) = transfer_ownership(&app, &bob_jwt, "dupe-org", "bob_ot").await;

    // Could be 409 (Conflict) or 422 depending on which validation fires first (self or already-owner)
    assert!(
        status == StatusCode::CONFLICT
            || status == StatusCode::UNPROCESSABLE_ENTITY
            || status == StatusCode::BAD_REQUEST,
        "transferring to self/already-owner should be rejected, got {status}"
    );
    let error_msg = body["error"].as_str().expect("error should be present");
    assert!(
        error_msg.contains("already") || error_msg.contains("different"),
        "error should mention 'already' or 'different', got: {error_msg}"
    );
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
