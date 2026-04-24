use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
};
use base64::engine::{general_purpose::STANDARD as BASE64, Engine};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use std::{net::ToSocketAddrs, sync::Arc};
use tokio::time::Duration;
use tower::ServiceExt;
use url::Url;
use uuid::Uuid;

use publaryn_api::{
    config::Config, job_handlers::CleanupOciBlobsHandler, router::build_router, state::AppState,
    storage::ArtifactStoreReaderAdapter,
};
use publaryn_workers::{
    handler::JobHandler,
    scanners::{PolicyScanner, ScanArtifactHandler, SecretsScanner},
};

// ── Helpers ──────────────────────────────────────────────────────────────────

const TEST_RESPONSE_BODY_LIMIT: usize = 8 * 1024 * 1024;

/// Build an Axum app backed by the given DB pool.
fn app(pool: PgPool) -> axum::Router {
    app_with_state(pool).1
}

/// Build Axum app state and the corresponding router backed by the given DB pool.
fn app_with_state(pool: PgPool) -> (AppState, axum::Router) {
    // When constructing state with `new_with_pool`, the provided pool is used for
    // database access and `config.database.url` is not used to establish a
    // connection in this test helper. Keep the fallback as an explicit
    // placeholder to avoid accidental coupling to a real database.
    let database_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "unused://database-url".into());
    let config = Config::test_config(&database_url);
    let state = AppState::new_with_pool(pool, config);
    let app = build_router(state.clone()).expect("router should build");
    (state, app)
}

/// Parse a response body as JSON.
async fn body_json(resp: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(resp.into_body(), TEST_RESPONSE_BODY_LIMIT)
        .await
        .expect("read body");
    serde_json::from_slice(&bytes).expect("parse JSON")
}

/// Parse a response body as text.
async fn body_text(resp: axum::response::Response) -> String {
    let bytes = axum::body::to_bytes(resp.into_body(), TEST_RESPONSE_BODY_LIMIT)
        .await
        .expect("read body");
    String::from_utf8(bytes.to_vec()).expect("parse text body")
}

/// Parse a response body as raw bytes.
async fn body_bytes(resp: axum::response::Response) -> Vec<u8> {
    axum::body::to_bytes(resp.into_body(), TEST_RESPONSE_BODY_LIMIT)
        .await
        .expect("read body")
        .to_vec()
}

fn enc_path_segment(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

/// Extract the first `detail` field from Cargo's standard error response body.
fn cargo_error_detail(body: &Value) -> &str {
    body["errors"]
        .as_array()
        .and_then(|errors| errors.first())
        .and_then(|error| error["detail"].as_str())
        .expect("cargo error response should contain one detail message")
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

async fn promote_user_to_platform_admin(pool: &PgPool, username: &str) {
    sqlx::query(
        "UPDATE users \
         SET is_admin = TRUE, updated_at = NOW() \
         WHERE username = $1",
    )
    .bind(username)
    .execute(pool)
    .await
    .expect("user should be promotable to platform admin in tests");
}

/// Create a personal access token and return the response.
async fn create_personal_access_token(
    app: &axum::Router,
    jwt: &str,
    name: &str,
    scopes: &[&str],
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/tokens")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::from(
            json!({
                "name": name,
                "scopes": scopes,
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Build a Cargo publish wire-format payload from JSON metadata and crate bytes.
fn build_cargo_publish_payload(metadata: Value, crate_bytes: &[u8]) -> Vec<u8> {
    let metadata_bytes =
        serde_json::to_vec(&metadata).expect("cargo publish metadata should serialize");
    let mut payload = Vec::with_capacity(metadata_bytes.len() + crate_bytes.len() + 8);
    payload.extend_from_slice(&(metadata_bytes.len() as u32).to_le_bytes());
    payload.extend_from_slice(&metadata_bytes);
    payload.extend_from_slice(&(crate_bytes.len() as u32).to_le_bytes());
    payload.extend_from_slice(crate_bytes);
    payload
}

/// Publish a Cargo crate via the native adapter and return the JSON response.
async fn publish_cargo_crate(
    app: &axum::Router,
    token: &str,
    payload: Vec<u8>,
) -> (StatusCode, Value) {
    let content_length = payload.len();
    let req = Request::builder()
        .method(Method::PUT)
        .uri("/cargo/api/v1/crates/new")
        .header(header::AUTHORIZATION, token)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_LENGTH, content_length.to_string())
        .body(Body::from(payload))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

fn cargo_sparse_index_path(crate_name: &str) -> String {
    let normalized = crate_name.to_ascii_lowercase().replace('-', "_");
    match normalized.len() {
        1 => format!("/cargo/index/1/{normalized}"),
        2 => format!("/cargo/index/2/{normalized}"),
        3 => format!("/cargo/index/3/{}/{normalized}", &normalized[..1]),
        _ => format!(
            "/cargo/index/{}/{}/{normalized}",
            &normalized[..2],
            &normalized[2..4]
        ),
    }
}

async fn get_cargo_sparse_index(
    app: &axum::Router,
    auth: Option<&str>,
    crate_name: &str,
) -> axum::response::Response {
    let mut req = Request::builder()
        .method(Method::GET)
        .uri(cargo_sparse_index_path(crate_name));
    if let Some(token) = auth {
        req = req.header(header::AUTHORIZATION, token);
    }
    app.clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn download_cargo_crate(
    app: &axum::Router,
    auth: Option<&str>,
    crate_name: &str,
    version: &str,
) -> axum::response::Response {
    let mut req = Request::builder().method(Method::GET).uri(format!(
        "/cargo/api/v1/crates/{}/{}/download",
        enc_path_segment(crate_name),
        enc_path_segment(version)
    ));
    if let Some(token) = auth {
        req = req.header(header::AUTHORIZATION, token);
    }
    app.clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

/// Yank a Cargo crate version through the native adapter.
async fn yank_cargo_crate_version(
    app: &axum::Router,
    token: &str,
    name: &str,
    version: &str,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::DELETE)
        .uri(format!(
            "/cargo/api/v1/crates/{}/{}/yank",
            enc_path_segment(name),
            enc_path_segment(version)
        ))
        .header(header::AUTHORIZATION, token)
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Restore a yanked Cargo crate version through the native adapter.
async fn unyank_cargo_crate_version(
    app: &axum::Router,
    token: &str,
    name: &str,
    version: &str,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::PUT)
        .uri(format!(
            "/cargo/api/v1/crates/{}/{}/unyank",
            enc_path_segment(name),
            enc_path_segment(version)
        ))
        .header(header::AUTHORIZATION, token)
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// List Cargo crate owners through the native compatibility endpoint.
async fn list_cargo_crate_owners(
    app: &axum::Router,
    token: &str,
    name: &str,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::GET)
        .uri(format!(
            "/cargo/api/v1/crates/{}/owners",
            enc_path_segment(name)
        ))
        .header(header::AUTHORIZATION, token)
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Request Cargo crate owner additions through the native compatibility endpoint.
async fn add_cargo_crate_owners(
    app: &axum::Router,
    token: &str,
    name: &str,
    users: &[&str],
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::PUT)
        .uri(format!(
            "/cargo/api/v1/crates/{}/owners",
            enc_path_segment(name)
        ))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, token)
        .body(Body::from(json!({ "users": users }).to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Request Cargo crate owner removals through the native compatibility endpoint.
async fn remove_cargo_crate_owners(
    app: &axum::Router,
    token: &str,
    name: &str,
    users: &[&str],
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::DELETE)
        .uri(format!(
            "/cargo/api/v1/crates/{}/owners",
            enc_path_segment(name)
        ))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, token)
        .body(Body::from(json!({ "users": users }).to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Create an organization via POST /v1/orgs and return the response.
async fn create_org(app: &axum::Router, jwt: &str, name: &str, slug: &str) -> (StatusCode, Value) {
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

/// Update an organization profile and return the response.
async fn update_org_profile(
    app: &axum::Router,
    jwt: &str,
    org_slug: &str,
    payload: Value,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::PATCH)
        .uri(format!("/v1/orgs/{org_slug}"))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::from(payload.to_string()))
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

/// Update a team for an organization and return the response.
async fn update_team_for_org(
    app: &axum::Router,
    jwt: &str,
    org_slug: &str,
    team_slug: &str,
    payload: Value,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::PATCH)
        .uri(format!("/v1/orgs/{org_slug}/teams/{team_slug}"))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::from(payload.to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Delete a team for an organization and return the response.
async fn delete_team_for_org(
    app: &axum::Router,
    jwt: &str,
    org_slug: &str,
    team_slug: &str,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::DELETE)
        .uri(format!("/v1/orgs/{org_slug}/teams/{team_slug}"))
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::empty())
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

/// Send an organization invitation and return the response.
async fn send_org_invitation(
    app: &axum::Router,
    jwt: &str,
    org_slug: &str,
    username_or_email: &str,
    role: &str,
    expires_in_days: u32,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::POST)
        .uri(format!("/v1/orgs/{org_slug}/invitations"))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::from(
            json!({
                "username_or_email": username_or_email,
                "role": role,
                "expires_in_days": expires_in_days,
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Revoke an organization invitation and return the response.
async fn revoke_org_invitation_for_org(
    app: &axum::Router,
    jwt: &str,
    org_slug: &str,
    invitation_id: &str,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::DELETE)
        .uri(format!("/v1/orgs/{org_slug}/invitations/{invitation_id}"))
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Accept an invitation as the current user and return the response.
async fn accept_org_invitation_for_current_user(
    app: &axum::Router,
    jwt: &str,
    invitation_id: &str,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::POST)
        .uri(format!("/v1/org-invitations/{invitation_id}/accept"))
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Decline an invitation as the current user and return the response.
async fn decline_org_invitation_for_current_user(
    app: &axum::Router,
    jwt: &str,
    invitation_id: &str,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::POST)
        .uri(format!("/v1/org-invitations/{invitation_id}/decline"))
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// List organization invitations and return the response.
async fn list_org_invitations_for_org(
    app: &axum::Router,
    jwt: &str,
    org_slug: &str,
    include_inactive: bool,
) -> (StatusCode, Value) {
    let uri = if include_inactive {
        format!("/v1/orgs/{org_slug}/invitations?include_inactive=true")
    } else {
        format!("/v1/orgs/{org_slug}/invitations")
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

/// Export organization-scoped audit entries as CSV and return the raw response.
async fn export_org_audit_csv(
    app: &axum::Router,
    jwt: &str,
    org_slug: &str,
    query: Option<&str>,
) -> axum::response::Response {
    let uri = match query {
        Some(query) if !query.trim().is_empty() => {
            format!("/v1/orgs/{org_slug}/audit/export?{query}")
        }
        _ => format!("/v1/orgs/{org_slug}/audit/export"),
    };

    let req = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::empty())
        .unwrap();

    app.clone().oneshot(req).await.unwrap()
}

/// List platform background jobs and return the response.
async fn list_platform_admin_jobs(
    app: &axum::Router,
    jwt: &str,
    query: Option<&str>,
) -> (StatusCode, Value) {
    let uri = match query {
        Some(query) if !query.trim().is_empty() => format!("/v1/admin/jobs?{query}"),
        _ => "/v1/admin/jobs".to_owned(),
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

/// Read public platform stats and return the response.
async fn get_platform_stats(app: &axum::Router) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::GET)
        .uri("/v1/stats")
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Create a namespace claim and return the response.
async fn create_namespace_claim(
    app: &axum::Router,
    jwt: &str,
    ecosystem: &str,
    namespace: &str,
    owner_org_id: Option<&str>,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/namespaces")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::from(
            json!({
                "ecosystem": ecosystem,
                "namespace": namespace,
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

/// List namespace claims and return the response.
async fn list_namespace_claims(app: &axum::Router, query: Option<&str>) -> (StatusCode, Value) {
    let uri = match query {
        Some(query) if !query.trim().is_empty() => format!("/v1/namespaces?{query}"),
        _ => "/v1/namespaces".to_owned(),
    };

    let req = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// List namespace claims with authentication and return the response.
async fn list_namespace_claims_authenticated(
    app: &axum::Router,
    jwt: &str,
    query: Option<&str>,
) -> (StatusCode, Value) {
    let uri = match query {
        Some(query) if !query.trim().is_empty() => format!("/v1/namespaces?{query}"),
        _ => "/v1/namespaces".to_owned(),
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

/// Delete a namespace claim and return the response.
async fn delete_namespace_claim(
    app: &axum::Router,
    jwt: &str,
    claim_id: &str,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::DELETE)
        .uri(format!("/v1/namespaces/{claim_id}"))
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Transfer a namespace claim and return the response.
async fn transfer_namespace_claim(
    app: &axum::Router,
    jwt: &str,
    claim_id: &str,
    target_org_slug: &str,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::POST)
        .uri(format!("/v1/namespaces/{claim_id}/ownership-transfer"))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::from(
            json!({
                "target_org_slug": target_org_slug,
            })
            .to_string(),
        ))
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
    create_repository_with_options(
        app,
        jwt,
        name,
        slug,
        owner_org_id,
        Some("public"),
        Some("public"),
    )
    .await
}

/// Create a repository with explicit kind/visibility settings and return the response.
async fn create_repository_with_options(
    app: &axum::Router,
    jwt: &str,
    name: &str,
    slug: &str,
    owner_org_id: Option<&str>,
    kind: Option<&str>,
    visibility: Option<&str>,
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
                "kind": kind,
                "visibility": visibility,
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

/// Get repository detail and return the response.
async fn get_repository_detail(
    app: &axum::Router,
    jwt: Option<&str>,
    slug: &str,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(Method::GET)
        .uri(format!("/v1/repositories/{slug}"));

    if let Some(jwt) = jwt {
        request = request.header(header::AUTHORIZATION, format!("Bearer {jwt}"));
    }

    let req = request.body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Update repository detail and return the response.
async fn update_repository_detail(
    app: &axum::Router,
    jwt: &str,
    slug: &str,
    payload: Value,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::PATCH)
        .uri(format!("/v1/repositories/{slug}"))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::from(payload.to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// List repositories owned by an organization and return the response.
async fn list_org_repositories(
    app: &axum::Router,
    jwt: Option<&str>,
    org_slug: &str,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(Method::GET)
        .uri(format!("/v1/orgs/{org_slug}/repositories"));

    if let Some(jwt) = jwt {
        request = request.header(header::AUTHORIZATION, format!("Bearer {jwt}"));
    }

    let req = request.body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// List aggregated repository package coverage for an organization and return the response.
async fn list_org_repository_package_coverage(
    app: &axum::Router,
    jwt: Option<&str>,
    org_slug: &str,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(Method::GET)
        .uri(format!("/v1/orgs/{org_slug}/repository-package-coverage"));

    if let Some(jwt) = jwt {
        request = request.header(header::AUTHORIZATION, format!("Bearer {jwt}"));
    }

    let req = request.body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Load aggregated bootstrap data for an organization workspace and return the response.
async fn get_org_workspace_bootstrap(
    app: &axum::Router,
    jwt: Option<&str>,
    org_slug: &str,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(Method::GET)
        .uri(format!("/v1/orgs/{org_slug}/workspace"));

    if let Some(jwt) = jwt {
        request = request.header(header::AUTHORIZATION, format!("Bearer {jwt}"));
    }

    let req = request.body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// List security findings aggregated for an organization and return the response.
async fn list_org_security_findings(
    app: &axum::Router,
    jwt: Option<&str>,
    org_slug: &str,
) -> (StatusCode, Value) {
    list_org_security_findings_with_query(app, jwt, org_slug, None).await
}

/// List security findings aggregated for an organization with query parameters and return the response.
async fn list_org_security_findings_with_query(
    app: &axum::Router,
    jwt: Option<&str>,
    org_slug: &str,
    query: Option<&str>,
) -> (StatusCode, Value) {
    let mut request = Request::builder().method(Method::GET).uri(match query {
        Some(query) if !query.trim().is_empty() => {
            format!("/v1/orgs/{org_slug}/security-findings?{query}")
        }
        _ => format!("/v1/orgs/{org_slug}/security-findings"),
    });

    if let Some(jwt) = jwt {
        request = request.header(header::AUTHORIZATION, format!("Bearer {jwt}"));
    }

    let req = request.body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Export security findings aggregated for an organization as CSV and return the raw response.
async fn export_org_security_findings_csv(
    app: &axum::Router,
    jwt: Option<&str>,
    org_slug: &str,
    query: Option<&str>,
) -> axum::response::Response {
    let mut request = Request::builder().method(Method::GET).uri(match query {
        Some(query) if !query.trim().is_empty() => {
            format!("/v1/orgs/{org_slug}/security-findings/export?{query}")
        }
        _ => format!("/v1/orgs/{org_slug}/security-findings/export"),
    });

    if let Some(jwt) = jwt {
        request = request.header(header::AUTHORIZATION, format!("Bearer {jwt}"));
    }

    let req = request.body(Body::empty()).unwrap();
    app.clone().oneshot(req).await.unwrap()
}

/// List packages inside a repository and return the response.
async fn list_repository_packages(
    app: &axum::Router,
    jwt: Option<&str>,
    repository_slug: &str,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(Method::GET)
        .uri(format!("/v1/repositories/{repository_slug}/packages"));

    if let Some(jwt) = jwt {
        request = request.header(header::AUTHORIZATION, format!("Bearer {jwt}"));
    }

    let req = request.body(Body::empty()).unwrap();
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
    create_package_with_options(app, jwt, ecosystem, name, repository_slug, None).await
}

/// Create a package with explicit visibility and return the response.
async fn create_package_with_options(
    app: &axum::Router,
    jwt: &str,
    ecosystem: &str,
    name: &str,
    repository_slug: &str,
    visibility: Option<&str>,
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
                "visibility": visibility,
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Get package detail and return the response.
async fn get_package_detail(
    app: &axum::Router,
    jwt: Option<&str>,
    ecosystem: &str,
    name: &str,
) -> (StatusCode, Value) {
    let mut request = Request::builder().method(Method::GET).uri(format!(
        "/v1/packages/{}/{}",
        enc_path_segment(ecosystem),
        enc_path_segment(name)
    ));

    if let Some(jwt) = jwt {
        request = request.header(header::AUTHORIZATION, format!("Bearer {jwt}"));
    }

    let req = request.body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// List trusted publishers for a package and return the response.
async fn list_trusted_publishers_for_package(
    app: &axum::Router,
    jwt: Option<&str>,
    ecosystem: &str,
    name: &str,
) -> (StatusCode, Value) {
    let mut request = Request::builder().method(Method::GET).uri(format!(
        "/v1/packages/{ecosystem}/{name}/trusted-publishers"
    ));

    if let Some(jwt) = jwt {
        request = request.header(header::AUTHORIZATION, format!("Bearer {jwt}"));
    }

    let req = request.body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Create a trusted publisher for a package and return the response.
async fn create_trusted_publisher_for_package(
    app: &axum::Router,
    jwt: &str,
    ecosystem: &str,
    name: &str,
    payload: Value,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::POST)
        .uri(format!(
            "/v1/packages/{ecosystem}/{name}/trusted-publishers"
        ))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::from(payload.to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Delete a trusted publisher for a package and return the response.
async fn delete_trusted_publisher_for_package(
    app: &axum::Router,
    jwt: &str,
    ecosystem: &str,
    name: &str,
    publisher_id: &str,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::DELETE)
        .uri(format!(
            "/v1/packages/{ecosystem}/{name}/trusted-publishers/{publisher_id}"
        ))
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// List packages owned by an organization and return the response.
async fn list_org_packages(
    app: &axum::Router,
    jwt: Option<&str>,
    org_slug: &str,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(Method::GET)
        .uri(format!("/v1/orgs/{org_slug}/packages"));

    if let Some(jwt) = jwt {
        request = request.header(header::AUTHORIZATION, format!("Bearer {jwt}"));
    }

    let req = request.body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Transfer package ownership and return the response.
async fn transfer_package_ownership(
    app: &axum::Router,
    jwt: &str,
    ecosystem: &str,
    name: &str,
    target_org_slug: &str,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::POST)
        .uri(format!(
            "/v1/packages/{ecosystem}/{name}/ownership-transfer"
        ))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::from(
            json!({ "target_org_slug": target_org_slug }).to_string(),
        ))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Transfer repository ownership and return the response.
async fn transfer_repository_ownership(
    app: &axum::Router,
    jwt: &str,
    repository_slug: &str,
    target_org_slug: &str,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::POST)
        .uri(format!(
            "/v1/repositories/{repository_slug}/ownership-transfer"
        ))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::from(
            json!({ "target_org_slug": target_org_slug }).to_string(),
        ))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Archive a package via `DELETE /v1/packages/{ecosystem}/{name}` and return the response.
async fn delete_package_for_ecosystem(
    app: &axum::Router,
    jwt: &str,
    ecosystem: &str,
    name: &str,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::DELETE)
        .uri(format!("/v1/packages/{ecosystem}/{name}"))
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Add an existing organization member to a team and return the response.
async fn add_team_member_to_team(
    app: &axum::Router,
    jwt: &str,
    org_slug: &str,
    team_slug: &str,
    username: &str,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::POST)
        .uri(format!("/v1/orgs/{org_slug}/teams/{team_slug}/members"))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::from(json!({ "username": username }).to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Remove a team member and return the response.
async fn remove_team_member_from_team(
    app: &axum::Router,
    jwt: &str,
    org_slug: &str,
    team_slug: &str,
    username: &str,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::DELETE)
        .uri(format!(
            "/v1/orgs/{org_slug}/teams/{team_slug}/members/{username}"
        ))
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Replace delegated team package access and return the response.
async fn grant_team_package_access(
    app: &axum::Router,
    jwt: &str,
    org_slug: &str,
    team_slug: &str,
    ecosystem: &str,
    name: &str,
    permissions: &[&str],
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::PUT)
        .uri(format!(
            "/v1/orgs/{org_slug}/teams/{team_slug}/package-access/{ecosystem}/{name}"
        ))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::from(
            json!({
                "permissions": permissions,
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Remove delegated team package access and return the response.
async fn remove_team_package_access(
    app: &axum::Router,
    jwt: &str,
    org_slug: &str,
    team_slug: &str,
    ecosystem: &str,
    name: &str,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::DELETE)
        .uri(format!(
            "/v1/orgs/{org_slug}/teams/{team_slug}/package-access/{ecosystem}/{name}"
        ))
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// List delegated team repository access and return the response.
async fn list_team_repository_access(
    app: &axum::Router,
    jwt: &str,
    org_slug: &str,
    team_slug: &str,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::GET)
        .uri(format!(
            "/v1/orgs/{org_slug}/teams/{team_slug}/repository-access"
        ))
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Replace delegated team repository access and return the response.
async fn grant_team_repository_access(
    app: &axum::Router,
    jwt: &str,
    org_slug: &str,
    team_slug: &str,
    repository_slug: &str,
    permissions: &[&str],
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::PUT)
        .uri(format!(
            "/v1/orgs/{org_slug}/teams/{team_slug}/repository-access/{repository_slug}"
        ))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::from(
            json!({
                "permissions": permissions,
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Remove delegated team repository access and return the response.
async fn remove_team_repository_access(
    app: &axum::Router,
    jwt: &str,
    org_slug: &str,
    team_slug: &str,
    repository_slug: &str,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::DELETE)
        .uri(format!(
            "/v1/orgs/{org_slug}/teams/{team_slug}/repository-access/{repository_slug}"
        ))
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// List delegated team namespace access and return the response.
async fn list_team_namespace_access(
    app: &axum::Router,
    jwt: &str,
    org_slug: &str,
    team_slug: &str,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::GET)
        .uri(format!(
            "/v1/orgs/{org_slug}/teams/{team_slug}/namespace-access"
        ))
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Replace delegated team namespace access and return the response.
async fn grant_team_namespace_access(
    app: &axum::Router,
    jwt: &str,
    org_slug: &str,
    team_slug: &str,
    claim_id: &str,
    permissions: &[&str],
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::PUT)
        .uri(format!(
            "/v1/orgs/{org_slug}/teams/{team_slug}/namespace-access/{claim_id}"
        ))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::from(
            json!({
                "permissions": permissions,
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Remove delegated team namespace access and return the response.
async fn remove_team_namespace_access(
    app: &axum::Router,
    jwt: &str,
    org_slug: &str,
    team_slug: &str,
    claim_id: &str,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::DELETE)
        .uri(format!(
            "/v1/orgs/{org_slug}/teams/{team_slug}/namespace-access/{claim_id}"
        ))
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Update package metadata and return the response.
async fn update_package_metadata(
    app: &axum::Router,
    jwt: &str,
    ecosystem: &str,
    name: &str,
    payload: Value,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::PATCH)
        .uri(format!("/v1/packages/{ecosystem}/{name}"))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::from(payload.to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Search publicly discoverable packages and return the response.
async fn search_public_packages(
    app: &axum::Router,
    query: &str,
    ecosystem: Option<&str>,
) -> (StatusCode, Value) {
    search_packages_with_options(
        app,
        None,
        query,
        SearchPackagesRequestOptions {
            ecosystem,
            ..SearchPackagesRequestOptions::default()
        },
    )
    .await
}

#[derive(Debug, Clone, Copy, Default)]
struct SearchPackagesRequestOptions<'a> {
    ecosystem: Option<&'a str>,
    org: Option<&'a str>,
    repository: Option<&'a str>,
    page: Option<u32>,
    per_page: Option<u32>,
}

/// Search packages through the management API and return the response.
async fn search_packages_with_options(
    app: &axum::Router,
    auth_token: Option<&str>,
    query: &str,
    options: SearchPackagesRequestOptions<'_>,
) -> (StatusCode, Value) {
    let mut params = vec![format!("q={query}")];
    if let Some(ecosystem) = options.ecosystem {
        params.push(format!("ecosystem={ecosystem}"));
    }
    if let Some(org) = options.org {
        params.push(format!("org={org}"));
    }
    if let Some(repository) = options.repository {
        params.push(format!("repository={repository}"));
    }
    if let Some(page) = options.page {
        params.push(format!("page={page}"));
    }
    if let Some(per_page) = options.per_page {
        params.push(format!("per_page={per_page}"));
    }

    let uri = format!("/v1/search?{}", params.join("&"));

    let req = Request::builder().method(Method::GET).uri(uri);
    let req = if let Some(token) = auth_token {
        req.header(header::AUTHORIZATION, format!("Bearer {token}"))
    } else {
        req
    };
    let req = req.body(Body::empty()).unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Search npm packages through the native adapter and return the response.
async fn search_npm_packages(
    app: &axum::Router,
    auth_token: Option<&str>,
    query: &str,
    size: u32,
    from: u32,
) -> (StatusCode, Value) {
    let req = Request::builder().method(Method::GET).uri(format!(
        "/npm/-/v1/search?text={query}&size={size}&from={from}"
    ));
    let req = if let Some(token) = auth_token {
        req.header(header::AUTHORIZATION, format!("Bearer {token}"))
    } else {
        req
    };
    let req = req.body(Body::empty()).unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Search Cargo crates through the native adapter and return the response.
async fn search_cargo_crates(
    app: &axum::Router,
    auth_token: Option<&str>,
    query: &str,
    per_page: u32,
) -> (StatusCode, Value) {
    let req = Request::builder().method(Method::GET).uri(format!(
        "/cargo/api/v1/crates?q={query}&per_page={per_page}"
    ));
    let req = if let Some(token) = auth_token {
        req.header(header::AUTHORIZATION, token)
    } else {
        req
    };
    let req = req.body(Body::empty()).unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

fn is_search_backend_available() -> bool {
    let search_url = std::env::var("SEARCH__URL")
        .ok()
        .or_else(|| load_dotenv_value("SEARCH__URL"))
        .unwrap_or_else(|| "http://localhost:7700".to_owned());

    let Ok(parsed_url) = Url::parse(&search_url) else {
        return false;
    };

    let Some(host) = parsed_url.host_str() else {
        return false;
    };

    let Some(port) = parsed_url.port_or_known_default() else {
        return false;
    };

    let Ok(addresses) = format!("{host}:{port}").to_socket_addrs() else {
        return false;
    };

    addresses.into_iter().any(|address| {
        std::net::TcpStream::connect_timeout(&address, std::time::Duration::from_millis(250))
            .is_ok()
    })
}

fn load_dotenv_value(key: &str) -> Option<String> {
    let contents = std::fs::read_to_string(".env").ok()?;

    contents.lines().find_map(|line| {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            return None;
        }

        let (name, value) = trimmed.split_once('=')?;
        if name == key {
            Some(value.trim().to_owned())
        } else {
            None
        }
    })
}

/// Create a release for a package and return the response.
async fn create_release_for_package(
    app: &axum::Router,
    jwt: &str,
    ecosystem: &str,
    name: &str,
    version: &str,
) -> (StatusCode, Value) {
    create_release_for_package_with_payload(
        app,
        jwt,
        ecosystem,
        name,
        json!({
            "version": version,
        }),
    )
    .await
}

/// Create a release for a package with an explicit payload and return the response.
async fn create_release_for_package_with_payload(
    app: &axum::Router,
    jwt: &str,
    ecosystem: &str,
    name: &str,
    payload: Value,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::POST)
        .uri(format!(
            "/v1/packages/{}/{}/releases",
            enc_path_segment(ecosystem),
            enc_path_segment(name)
        ))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::from(payload.to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Get release detail and return the response.
async fn get_release_detail(
    app: &axum::Router,
    jwt: Option<&str>,
    ecosystem: &str,
    name: &str,
    version: &str,
) -> (StatusCode, Value) {
    let mut request = Request::builder().method(Method::GET).uri(format!(
        "/v1/packages/{}/{}/releases/{}",
        enc_path_segment(ecosystem),
        enc_path_segment(name),
        enc_path_segment(version)
    ));

    if let Some(jwt) = jwt {
        request = request.header(header::AUTHORIZATION, format!("Bearer {jwt}"));
    }

    let req = request.body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// List release artifacts and return the response.
async fn list_release_artifacts(
    app: &axum::Router,
    jwt: Option<&str>,
    ecosystem: &str,
    name: &str,
    version: &str,
) -> (StatusCode, Value) {
    let mut request = Request::builder().method(Method::GET).uri(format!(
        "/v1/packages/{}/{}/releases/{}/artifacts",
        enc_path_segment(ecosystem),
        enc_path_segment(name),
        enc_path_segment(version)
    ));

    if let Some(jwt) = jwt {
        request = request.header(header::AUTHORIZATION, format!("Bearer {jwt}"));
    }

    let req = request.body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Upload an artifact for a release and return the response.
#[allow(clippy::too_many_arguments)]
async fn upload_release_artifact(
    app: &axum::Router,
    jwt: &str,
    ecosystem: &str,
    name: &str,
    version: &str,
    filename: &str,
    kind: &str,
    content_type: &str,
    bytes: &[u8],
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::PUT)
        .uri(format!(
            "/v1/packages/{}/{}/releases/{}/artifacts/{}?kind={}",
            enc_path_segment(ecosystem),
            enc_path_segment(name),
            enc_path_segment(version),
            enc_path_segment(filename),
            enc_path_segment(kind)
        ))
        .header(header::CONTENT_TYPE, content_type)
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::from(bytes.to_vec()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Publish a release and return the response.
async fn publish_release_for_package(
    app: &axum::Router,
    jwt: &str,
    ecosystem: &str,
    name: &str,
    version: &str,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::POST)
        .uri(format!(
            "/v1/packages/{}/{}/releases/{}/publish",
            enc_path_segment(ecosystem),
            enc_path_segment(name),
            enc_path_segment(version)
        ))
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

async fn fetch_background_jobs(
    pool: &PgPool,
    kind: &str,
) -> Vec<(String, serde_json::Value, String)> {
    sqlx::query(
        "SELECT kind::text AS kind, payload, status::text AS status \
         FROM background_jobs \
         WHERE kind::text = $1 \
         ORDER BY created_at ASC",
    )
    .bind(kind)
    .fetch_all(pool)
    .await
    .expect("background jobs should be queryable")
    .into_iter()
    .map(|row| {
        (
            row.try_get::<String, _>("kind")
                .expect("job kind should be present"),
            row.try_get::<serde_json::Value, _>("payload")
                .expect("job payload should be present"),
            row.try_get::<String, _>("status")
                .expect("job status should be present"),
        )
    })
    .collect()
}

async fn fetch_oci_cleanup_jobs(pool: &PgPool) -> Vec<(serde_json::Value, String, DateTime<Utc>)> {
    sqlx::query(
        "SELECT payload, status::text AS status, scheduled_at \
         FROM background_jobs \
         WHERE kind::text = 'cleanup_oci_blobs' \
         ORDER BY created_at ASC",
    )
    .fetch_all(pool)
    .await
    .expect("oci cleanup jobs should be queryable")
    .into_iter()
    .map(|row| {
        (
            row.try_get::<serde_json::Value, _>("payload")
                .expect("cleanup payload should be present"),
            row.try_get::<String, _>("status")
                .expect("cleanup status should be present"),
            row.try_get::<DateTime<Utc>, _>("scheduled_at")
                .expect("cleanup scheduled_at should be present"),
        )
    })
    .collect()
}

async fn count_oci_blob_inventory(pool: &PgPool, digest: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM oci_blob_inventory WHERE digest = $1")
        .bind(digest)
        .fetch_one(pool)
        .await
        .expect("oci blob inventory count should be queryable")
}

/// Yank a release and return the response.
async fn yank_release_for_package(
    app: &axum::Router,
    jwt: &str,
    ecosystem: &str,
    name: &str,
    version: &str,
    reason: Option<&str>,
) -> (StatusCode, Value) {
    let payload = match reason {
        Some(reason) => json!({ "reason": reason }),
        None => json!({}),
    };
    let req = Request::builder()
        .method(Method::PUT)
        .uri(format!(
            "/v1/packages/{ecosystem}/{name}/releases/{version}/yank"
        ))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::from(payload.to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Unyank (restore) a release and return the response.
async fn unyank_release_for_package(
    app: &axum::Router,
    jwt: &str,
    ecosystem: &str,
    name: &str,
    version: &str,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::PUT)
        .uri(format!(
            "/v1/packages/{ecosystem}/{name}/releases/{version}/unyank"
        ))
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Deprecate a release and return the response.
async fn deprecate_release_for_package(
    app: &axum::Router,
    jwt: &str,
    ecosystem: &str,
    name: &str,
    version: &str,
    message: Option<&str>,
) -> (StatusCode, Value) {
    let payload = match message {
        Some(msg) => json!({ "message": msg }),
        None => json!({}),
    };
    let req = Request::builder()
        .method(Method::PUT)
        .uri(format!(
            "/v1/packages/{ecosystem}/{name}/releases/{version}/deprecate"
        ))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::from(payload.to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Remove deprecation from a release and return the response.
async fn undeprecate_release_for_package(
    app: &axum::Router,
    jwt: &str,
    ecosystem: &str,
    name: &str,
    version: &str,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::PUT)
        .uri(format!(
            "/v1/packages/{ecosystem}/{name}/releases/{version}/undeprecate"
        ))
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// List package channel tags and return the response.
async fn list_package_tags(
    app: &axum::Router,
    jwt: Option<&str>,
    ecosystem: &str,
    name: &str,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(Method::GET)
        .uri(format!("/v1/packages/{ecosystem}/{name}/tags"));

    if let Some(jwt) = jwt {
        request = request.header(header::AUTHORIZATION, format!("Bearer {jwt}"));
    }

    let req = request.body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Upsert a package channel tag and return the response.
async fn upsert_package_tag(
    app: &axum::Router,
    jwt: &str,
    ecosystem: &str,
    name: &str,
    tag: &str,
    version: &str,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::PUT)
        .uri(format!("/v1/packages/{ecosystem}/{name}/tags/{tag}"))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::from(json!({ "version": version }).to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Delete a package channel tag and return the response.
async fn delete_package_tag(
    app: &axum::Router,
    jwt: &str,
    ecosystem: &str,
    name: &str,
    tag: &str,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::DELETE)
        .uri(format!("/v1/packages/{ecosystem}/{name}/tags/{tag}"))
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Get a native npm packument and return the response.
async fn get_npm_packument(
    app: &axum::Router,
    jwt: Option<&str>,
    package_name: &str,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(Method::GET)
        .uri(format!("/npm/{package_name}"));

    if let Some(jwt) = jwt {
        request = request.header(header::AUTHORIZATION, format!("Bearer {jwt}"));
    }

    let req = request.body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// List native npm dist-tags and return the response.
async fn list_npm_dist_tags(
    app: &axum::Router,
    jwt: Option<&str>,
    package_name: &str,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(Method::GET)
        .uri(format!("/npm/-/package/{package_name}/dist-tags"));

    if let Some(jwt) = jwt {
        request = request.header(header::AUTHORIZATION, format!("Bearer {jwt}"));
    }

    let req = request.body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Set a native npm dist-tag and return the response.
async fn set_npm_dist_tag(
    app: &axum::Router,
    jwt: &str,
    package_name: &str,
    tag: &str,
    version: &str,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::PUT)
        .uri(format!("/npm/-/package/{package_name}/dist-tags/{tag}"))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::from(json!(version).to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Publish a native npm package version and return the response.
async fn publish_npm_package(
    app: &axum::Router,
    jwt: &str,
    package_name: &str,
    version: &str,
    tarball_bytes: &[u8],
) -> (StatusCode, Value) {
    let filename = format!("{package_name}-{version}.tgz");
    let req = Request::builder()
        .method(Method::PUT)
        .uri(format!("/npm/{package_name}"))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::from(
            json!({
                "name": package_name,
                "versions": {
                    version: {
                        "name": package_name,
                        "version": version,
                        "description": format!("Published {package_name} {version}"),
                    }
                },
                "dist-tags": {
                    "latest": version,
                },
                "_attachments": {
                    filename: {
                        "content_type": "application/gzip",
                        "data": BASE64.encode(tarball_bytes),
                        "length": tarball_bytes.len(),
                    }
                },
                "readme": format!("# {package_name}\n\nPublished in integration tests."),
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Download a native npm tarball and return the response body.
async fn download_npm_tarball(
    app: &axum::Router,
    jwt: Option<&str>,
    package_name: &str,
    filename: &str,
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let mut request = Request::builder()
        .method(Method::GET)
        .uri(format!("/npm/{package_name}/-/{filename}"));

    if let Some(jwt) = jwt {
        request = request.header(header::AUTHORIZATION, format!("Bearer {jwt}"));
    }

    let req = request.body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let headers = resp.headers().clone();
    let body = body_bytes(resp).await;
    (status, headers, body)
}

/// Resolve a release id for a package version directly from the database.
async fn get_release_id(
    pool: &PgPool,
    ecosystem: &str,
    package_name: &str,
    version: &str,
) -> uuid::Uuid {
    sqlx::query_scalar(
        "SELECT r.id \
         FROM releases r \
         JOIN packages p ON p.id = r.package_id \
         WHERE p.ecosystem = $1 AND p.name = $2 AND r.version = $3",
    )
    .bind(ecosystem)
    .bind(package_name)
    .bind(version)
    .fetch_one(pool)
    .await
    .expect("release id should be queryable")
}

async fn promote_release_to_published(
    pool: &PgPool,
    ecosystem: &str,
    normalized_name: &str,
    version: &str,
) {
    sqlx::query(
        "UPDATE releases SET status = 'published', updated_at = NOW() \
         WHERE package_id = (SELECT id FROM packages WHERE ecosystem = $1 AND normalized_name = $2) \
           AND version = $3",
    )
    .bind(ecosystem)
    .bind(normalized_name)
    .bind(version)
    .execute(pool)
    .await
    .expect("release should be promoted to published for lifecycle mutations");
}

/// Build a minimal valid PyPI legacy upload multipart body (with metadata_version 2.4
/// and sha256 digest validation) and return `(content_type, body)`.
fn build_pypi_legacy_upload_multipart(
    package_name: &str,
    version: &str,
    artifact_bytes: &[u8],
) -> (String, Vec<u8>) {
    build_pypi_legacy_upload_multipart_with_fields(package_name, version, artifact_bytes, &[])
}

/// Build a PyPI legacy upload multipart body with additional metadata fields and
/// return `(content_type, body)`.
fn build_pypi_legacy_upload_multipart_with_fields(
    package_name: &str,
    version: &str,
    artifact_bytes: &[u8],
    metadata_fields: &[(&str, &str)],
) -> (String, Vec<u8>) {
    use sha2::{Digest, Sha256};

    let boundary = "----publaryn-test-boundary-2f4e";
    let filename = format!("{package_name}-{version}.tar.gz");
    let mut digest = Sha256::new();
    digest.update(artifact_bytes);
    let sha256_hex = hex::encode(digest.finalize());

    let mut body: Vec<u8> = Vec::new();
    let mut text_fields: Vec<(String, String)> = vec![
        (":action".to_owned(), "file_upload".to_owned()),
        ("protocol_version".to_owned(), "1".to_owned()),
        ("metadata_version".to_owned(), "2.4".to_owned()),
        ("name".to_owned(), package_name.to_owned()),
        ("version".to_owned(), version.to_owned()),
        ("filetype".to_owned(), "sdist".to_owned()),
        ("pyversion".to_owned(), "source".to_owned()),
        ("sha256_digest".to_owned(), sha256_hex),
    ];
    text_fields.extend(
        metadata_fields
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned())),
    );

    for (name, value) in &text_fields {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
        );
        body.extend_from_slice(value.as_bytes());
        body.extend_from_slice(b"\r\n");
    }

    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"content\"; filename=\"{filename}\"\r\n",)
            .as_bytes(),
    );
    body.extend_from_slice(b"Content-Type: application/gzip\r\n\r\n");
    body.extend_from_slice(artifact_bytes);
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());

    let content_type = format!("multipart/form-data; boundary={boundary}");
    (content_type, body)
}

/// Upload a PyPI distribution via the legacy upload endpoint (optionally targeting a
/// specific repository slug) and return the response.
async fn upload_pypi_distribution(
    app: &axum::Router,
    token: &str,
    repository_slug: Option<&str>,
    package_name: &str,
    version: &str,
    artifact_bytes: &[u8],
) -> (StatusCode, Value) {
    let (content_type, body) =
        build_pypi_legacy_upload_multipart(package_name, version, artifact_bytes);
    let uri = match repository_slug {
        Some(slug) => format!("/pypi/legacy/{slug}/"),
        None => "/pypi/legacy/".to_owned(),
    };
    let auth_header = ["Bearer ", token].concat();
    let req = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(header::AUTHORIZATION, auth_header)
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(body))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json_or_empty(resp).await;
    (status, body)
}

/// Upload a PyPI distribution via the legacy upload endpoint with extra metadata
/// fields and return the response.
async fn upload_pypi_distribution_with_fields(
    app: &axum::Router,
    token: &str,
    repository_slug: Option<&str>,
    package_name: &str,
    version: &str,
    artifact_bytes: &[u8],
    metadata_fields: &[(&str, &str)],
) -> (StatusCode, Value) {
    let (content_type, body) = build_pypi_legacy_upload_multipart_with_fields(
        package_name,
        version,
        artifact_bytes,
        metadata_fields,
    );
    let uri = match repository_slug {
        Some(slug) => format!("/pypi/legacy/{slug}/"),
        None => "/pypi/legacy/".to_owned(),
    };
    let auth_header = ["Bearer ", token].concat();
    let req = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(header::AUTHORIZATION, auth_header)
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(body))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json_or_empty(resp).await;
    (status, body)
}

/// Read a PyPI project detail document from the Simple API JSON surface.
async fn get_pypi_simple_project_json(
    app: &axum::Router,
    token: Option<&str>,
    project: &str,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(Method::GET)
        .uri(format!("/pypi/simple/{project}/"))
        .header(header::ACCEPT, "application/vnd.pypi.simple.v1+json");

    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }

    let req = request.body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json_or_empty(resp).await;
    (status, body)
}

/// Read a PyPI project detail document from the Simple API HTML surface.
async fn get_pypi_simple_project_html(
    app: &axum::Router,
    token: Option<&str>,
    project: &str,
) -> (StatusCode, String) {
    let mut request = Request::builder()
        .method(Method::GET)
        .uri(format!("/pypi/simple/{project}/"))
        .header(header::ACCEPT, "text/html");

    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }

    let req = request.body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_text(resp).await;
    (status, body)
}

/// Build a minimal Composer publish multipart body and return `(content_type, body)`.
fn build_composer_publish_multipart(
    package_name: &str,
    version: &str,
    artifact_bytes: &[u8],
) -> (String, Vec<u8>) {
    let boundary = "----publaryn-composer-boundary-9e7f";
    let filename = format!("{}-{version}.zip", package_name.replace('/', "-"));
    let manifest = json!({
        "name": package_name,
        "version": version,
        "description": format!("Published {package_name} {version}"),
        "homepage": format!("https://packages.example.test/{package_name}"),
        "license": ["MIT"],
        "keywords": ["publaryn", "composer"],
        "support": {
            "source": format!("https://git.example.test/{package_name}"),
        },
    });
    let manifest_bytes =
        serde_json::to_vec(&manifest).expect("composer publish manifest should serialize");

    let mut body: Vec<u8> = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        b"Content-Disposition: form-data; name=\"composer.json\"; filename=\"composer.json\"\r\n",
    );
    body.extend_from_slice(b"Content-Type: application/json\r\n\r\n");
    body.extend_from_slice(&manifest_bytes);
    body.extend_from_slice(b"\r\n");

    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"dist.zip\"; filename=\"{filename}\"\r\n")
            .as_bytes(),
    );
    body.extend_from_slice(b"Content-Type: application/zip\r\n\r\n");
    body.extend_from_slice(artifact_bytes);
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());

    let content_type = format!("multipart/form-data; boundary={boundary}");
    (content_type, body)
}

/// Publish a Composer package version and return the JSON response.
async fn publish_composer_package(
    app: &axum::Router,
    token: &str,
    package_name: &str,
    version: &str,
    artifact_bytes: &[u8],
) -> (StatusCode, Value) {
    let (vendor, package) = package_name
        .split_once('/')
        .expect("composer package name should contain a vendor and package segment");
    let (content_type, body) =
        build_composer_publish_multipart(package_name, version, artifact_bytes);
    let req = Request::builder()
        .method(Method::PUT)
        .uri(format!("/composer/packages/{vendor}/{package}"))
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(body))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Yank a Composer package version and return the JSON response.
async fn yank_composer_package_version(
    app: &axum::Router,
    token: &str,
    vendor: &str,
    package: &str,
    version: &str,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::DELETE)
        .uri(format!(
            "/composer/packages/{vendor}/{package}/versions/{version}"
        ))
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Fetch Composer package metadata and return the JSON response.
async fn get_composer_package_metadata(
    app: &axum::Router,
    token: Option<&str>,
    vendor: &str,
    package: &str,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(Method::GET)
        .uri(format!("/composer/p/{vendor}/{package}.json"));
    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let req = request.body(Body::empty()).unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Build a minimal Maven POM document for integration tests.
fn build_maven_pom(group_id: &str, artifact_id: &str, version: &str) -> Vec<u8> {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0"
         xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
         xsi:schemaLocation="http://maven.apache.org/POM/4.0.0 https://maven.apache.org/xsd/maven-4.0.0.xsd">
  <modelVersion>4.0.0</modelVersion>
  <groupId>{group_id}</groupId>
  <artifactId>{artifact_id}</artifactId>
  <version>{version}</version>
  <name>{artifact_id}</name>
  <description>Published {artifact_id} {version}</description>
  <url>https://packages.example.test/{group_id}/{artifact_id}</url>
  <licenses>
    <license>
      <name>Apache-2.0</name>
    </license>
  </licenses>
  <scm>
    <url>https://git.example.test/{group_id}/{artifact_id}</url>
  </scm>
</project>"#
    )
    .into_bytes()
}

/// Upload a Maven repository file through the native Maven deploy adapter.
#[allow(clippy::too_many_arguments)]
async fn upload_maven_artifact(
    app: &axum::Router,
    token: &str,
    group_path: &str,
    artifact_id: &str,
    version: &str,
    filename: &str,
    content_type: &str,
    bytes: &[u8],
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::PUT)
        .uri(format!(
            "/maven/{group_path}/{artifact_id}/{version}/{filename}"
        ))
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(bytes.to_vec()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json_or_empty(resp).await;
    (status, body)
}

fn oci_digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};

    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

fn oci_blob_storage_key(digest: &str) -> String {
    let digest_hex = digest.split(':').nth(1).unwrap_or(digest);
    format!("oci/blobs/sha256/{digest_hex}")
}

fn build_oci_image_manifest(
    config_digest: &str,
    config_size: usize,
    layer_digest: &str,
    layer_size: usize,
) -> Vec<u8> {
    build_oci_image_manifest_with_options(
        config_digest,
        config_size,
        "application/vnd.oci.image.config.v1+json",
        layer_digest,
        layer_size,
        None,
        None,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_oci_image_manifest_with_options(
    config_digest: &str,
    config_size: usize,
    config_media_type: &str,
    layer_digest: &str,
    layer_size: usize,
    subject_digest: Option<&str>,
    subject_size: Option<usize>,
    annotations: Option<Value>,
) -> Vec<u8> {
    let mut manifest = json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.manifest.v1+json",
        "config": {
            "mediaType": config_media_type,
            "digest": config_digest,
            "size": config_size,
        },
        "layers": [
            {
                "mediaType": "application/vnd.oci.image.layer.v1.tar+gzip",
                "digest": layer_digest,
                "size": layer_size,
            }
        ]
    });

    if let Some(subject_digest) = subject_digest {
        manifest
            .as_object_mut()
            .expect("oci manifest should serialize as an object")
            .insert(
                "subject".into(),
                json!({
                    "mediaType": "application/vnd.oci.image.manifest.v1+json",
                    "digest": subject_digest,
                    "size": subject_size.expect("subject-sized manifests should provide a size"),
                }),
            );
    }

    if let Some(annotations) = annotations {
        manifest
            .as_object_mut()
            .expect("oci manifest should serialize as an object")
            .insert("annotations".into(), annotations);
    }

    serde_json::to_vec(&manifest).expect("oci manifest should serialize")
}

fn build_oci_artifact_manifest(
    artifact_type: &str,
    subject_digest: &str,
    subject_size: usize,
    blob_digest: &str,
    blob_size: usize,
    annotations: Option<Value>,
) -> Vec<u8> {
    let mut manifest = json!({
        "mediaType": "application/vnd.oci.artifact.manifest.v1+json",
        "artifactType": artifact_type,
        "blobs": [
            {
                "mediaType": "application/octet-stream",
                "digest": blob_digest,
                "size": blob_size,
            }
        ],
        "subject": {
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "digest": subject_digest,
            "size": subject_size,
        }
    });

    if let Some(annotations) = annotations {
        manifest
            .as_object_mut()
            .expect("oci artifact manifest should serialize as an object")
            .insert("annotations".into(), annotations);
    }

    serde_json::to_vec(&manifest).expect("oci artifact manifest should serialize")
}

async fn send_oci_request(
    app: &axum::Router,
    method: Method,
    uri: String,
    token: Option<&str>,
    content_type: Option<&str>,
    body: Vec<u8>,
) -> axum::response::Response {
    let mut request = Request::builder().method(method).uri(uri);

    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }

    if let Some(content_type) = content_type {
        request = request.header(header::CONTENT_TYPE, content_type);
    }

    let req = request.body(Body::from(body)).unwrap();
    app.clone().oneshot(req).await.unwrap()
}

async fn get_oci_referrers(
    app: &axum::Router,
    token: Option<&str>,
    package_name: &str,
    subject_digest: &str,
    query: Option<&str>,
) -> axum::response::Response {
    let uri = match query {
        Some(query) if !query.trim().is_empty() => {
            format!("/oci/v2/{package_name}/referrers/{subject_digest}?{query}")
        }
        _ => format!("/oci/v2/{package_name}/referrers/{subject_digest}"),
    };

    send_oci_request(app, Method::GET, uri, token, None, vec![]).await
}

async fn get_oci_catalog(
    app: &axum::Router,
    token: Option<&str>,
    query: Option<&str>,
) -> axum::response::Response {
    let uri = match query {
        Some(query) if !query.trim().is_empty() => format!("/oci/v2/_catalog?{query}"),
        _ => "/oci/v2/_catalog".to_owned(),
    };

    send_oci_request(app, Method::GET, uri, token, None, vec![]).await
}

async fn get_oci_tags_list(
    app: &axum::Router,
    token: Option<&str>,
    package_name: &str,
    query: Option<&str>,
) -> axum::response::Response {
    let uri = match query {
        Some(query) if !query.trim().is_empty() => {
            format!("/oci/v2/{package_name}/tags/list?{query}")
        }
        _ => format!("/oci/v2/{package_name}/tags/list"),
    };

    send_oci_request(app, Method::GET, uri, token, None, vec![]).await
}

fn extract_oci_next_link_uri(link_value: &str) -> String {
    link_value
        .split(';')
        .next()
        .and_then(|part| part.strip_prefix('<'))
        .and_then(|part| part.strip_suffix('>'))
        .expect("link header should wrap the next URI in angle brackets")
        .to_owned()
}

async fn publish_oci_image_manifest_tag(
    app: &axum::Router,
    token: &str,
    package_name: &str,
    tag: &str,
    config_bytes: &[u8],
    layer_bytes: &[u8],
) -> String {
    let (config_digest, config_resp) =
        upload_oci_blob_monolithic(app, token, package_name, config_bytes).await;
    assert_eq!(
        config_resp.status(),
        StatusCode::CREATED,
        "config blob upload should succeed"
    );

    let (layer_digest, layer_resp) =
        upload_oci_blob_monolithic(app, token, package_name, layer_bytes).await;
    assert_eq!(
        layer_resp.status(),
        StatusCode::CREATED,
        "layer blob upload should succeed"
    );

    let manifest_bytes = build_oci_image_manifest(
        &config_digest,
        config_bytes.len(),
        &layer_digest,
        layer_bytes.len(),
    );
    let manifest_digest = oci_digest(&manifest_bytes);
    let manifest_resp = send_oci_request(
        app,
        Method::PUT,
        format!("/oci/v2/{package_name}/manifests/{tag}"),
        Some(token),
        Some("application/vnd.oci.image.manifest.v1+json"),
        manifest_bytes,
    )
    .await;
    assert_eq!(
        manifest_resp.status(),
        StatusCode::CREATED,
        "manifest upload should succeed"
    );

    manifest_digest
}

async fn upload_oci_blob_monolithic(
    app: &axum::Router,
    token: &str,
    package_name: &str,
    bytes: &[u8],
) -> (String, axum::response::Response) {
    let digest = oci_digest(bytes);
    let response = send_oci_request(
        app,
        Method::POST,
        format!("/oci/v2/{package_name}/blobs/uploads/?digest={digest}"),
        Some(token),
        Some("application/octet-stream"),
        bytes.to_vec(),
    )
    .await;

    (digest, response)
}

fn build_rubygems_package(name: &str, version: &str) -> Vec<u8> {
    use std::io::{Cursor, Write};

    let gemspec = format!(
        r#"--- !ruby/object:Gem::Specification
name: {name}
version: !ruby/object:Gem::Version
  version: {version}
platform: ruby
authors:
  - Alice Example
summary: Published {name} {version}
description: Published {name} {version} in integration tests.
licenses:
  - MIT
homepage: https://packages.example.test/{name}
metadata:
  homepage_uri: https://packages.example.test/{name}
"#
    );

    let mut metadata_encoder =
        flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    metadata_encoder
        .write_all(gemspec.as_bytes())
        .expect("gemspec yaml should gzip");
    let metadata_gz = metadata_encoder
        .finish()
        .expect("metadata gzip should finalize");

    let mut data_encoder =
        flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    data_encoder
        .write_all(b"placeholder gem payload")
        .expect("data payload should gzip");
    let data_gz = data_encoder.finish().expect("data gzip should finalize");

    let mut gem_bytes = Vec::new();
    {
        let mut tar_builder = tar::Builder::new(&mut gem_bytes);

        let mut metadata_header = tar::Header::new_gnu();
        metadata_header.set_size(metadata_gz.len() as u64);
        metadata_header.set_mode(0o644);
        metadata_header.set_cksum();
        tar_builder
            .append_data(
                &mut metadata_header,
                "metadata.gz",
                Cursor::new(metadata_gz),
            )
            .expect("metadata.gz should be appended to gem tarball");

        let mut data_header = tar::Header::new_gnu();
        data_header.set_size(data_gz.len() as u64);
        data_header.set_mode(0o644);
        data_header.set_cksum();
        tar_builder
            .append_data(&mut data_header, "data.tar.gz", Cursor::new(data_gz))
            .expect("data.tar.gz should be appended to gem tarball");

        tar_builder.finish().expect("gem tarball should finalize");
    }

    gem_bytes
}

async fn push_rubygems_package(
    app: &axum::Router,
    token: &str,
    gem_bytes: &[u8],
) -> (StatusCode, String) {
    let req = Request::builder()
        .method(Method::POST)
        .uri("/rubygems/api/v1/gems")
        .header("x-gem-api-key", token)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .body(Body::from(gem_bytes.to_vec()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_text(resp).await;
    (status, body)
}

async fn yank_rubygems_version(
    app: &axum::Router,
    token: &str,
    gem_name: &str,
    version: &str,
) -> (StatusCode, String) {
    let req = Request::builder()
        .method(Method::DELETE)
        .uri("/rubygems/api/v1/gems/yank")
        .header("x-gem-api-key", token)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(format!("gem_name={gem_name}&version={version}")))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_text(resp).await;
    (status, body)
}

async fn get_rubygems_metadata(
    app: &axum::Router,
    token: Option<&str>,
    gem_name: &str,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(Method::GET)
        .uri(format!("/rubygems/api/v1/gems/{gem_name}"));

    if let Some(token) = token {
        request = request.header("x-gem-api-key", token);
    }

    let req = request.body(Body::empty()).unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

async fn get_rubygems_versions(
    app: &axum::Router,
    token: Option<&str>,
    gem_name: &str,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(Method::GET)
        .uri(format!("/rubygems/api/v1/versions/{gem_name}"));

    if let Some(token) = token {
        request = request.header("x-gem-api-key", token);
    }

    let req = request.body(Body::empty()).unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

fn build_nuget_package(id: &str, version: &str) -> Vec<u8> {
    use std::io::Write;

    let nuspec = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://schemas.microsoft.com/packaging/2013/05/nuspec.xsd">
  <metadata>
    <id>{id}</id>
    <version>{version}</version>
    <authors>Alice Example</authors>
    <description>Published {id} {version} in integration tests.</description>
    <summary>Published {id} {version}</summary>
    <projectUrl>https://packages.example.test/{id}</projectUrl>
    <tags>publaryn integration-test</tags>
    <license type="expression">MIT</license>
  </metadata>
</package>"#
    );

    let cursor = std::io::Cursor::new(Vec::new());
    let mut zip_writer = zip::ZipWriter::new(cursor);
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip_writer
        .start_file(format!("{id}.nuspec"), options)
        .expect("nuspec file should be created inside nupkg");
    zip_writer
        .write_all(nuspec.as_bytes())
        .expect("nuspec xml should be written to nupkg");

    zip_writer
        .finish()
        .expect("nupkg archive should finalize")
        .into_inner()
}

fn build_nuget_push_multipart(id: &str, version: &str, nupkg_bytes: &[u8]) -> (String, Vec<u8>) {
    let boundary = "----publaryn-nuget-boundary-1a2b";
    let filename = format!("{id}.{version}.nupkg");

    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"package\"; filename=\"{filename}\"\r\n")
            .as_bytes(),
    );
    body.extend_from_slice(b"Content-Type: application/octet-stream\r\n\r\n");
    body.extend_from_slice(nupkg_bytes);
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());

    (format!("multipart/form-data; boundary={boundary}"), body)
}

async fn push_nuget_package(
    app: &axum::Router,
    token: &str,
    id: &str,
    version: &str,
    nupkg_bytes: &[u8],
) -> (StatusCode, Value) {
    let (content_type, body) = build_nuget_push_multipart(id, version, nupkg_bytes);
    let req = Request::builder()
        .method(Method::PUT)
        .uri("/nuget/v2/package")
        .header("x-nuget-apikey", token)
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(body))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json_or_empty(resp).await;
    (status, body)
}

async fn get_nuget_version_listing(
    app: &axum::Router,
    token: Option<&str>,
    package_id: &str,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(Method::GET)
        .uri(format!("/nuget/v3-flatcontainer/{package_id}/index.json"));

    if let Some(token) = token {
        request = request.header("x-nuget-apikey", token);
    }

    let req = request.body(Body::empty()).unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json_or_empty(resp).await;
    (status, body)
}

async fn get_nuget_registration_index(
    app: &axum::Router,
    token: Option<&str>,
    package_id: &str,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(Method::GET)
        .uri(format!("/nuget/v3/registration/{package_id}/index.json"));

    if let Some(token) = token {
        request = request.header("x-nuget-apikey", token);
    }

    let req = request.body(Body::empty()).unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json_or_empty(resp).await;
    (status, body)
}

async fn set_nuget_listing_state(
    app: &axum::Router,
    token: &str,
    package_id: &str,
    version: &str,
    method: Method,
) -> StatusCode {
    let req = Request::builder()
        .method(method)
        .uri(format!("/nuget/v2/package/{package_id}/{version}"))
        .header("x-nuget-apikey", token)
        .body(Body::empty())
        .unwrap();

    app.clone().oneshot(req).await.unwrap().status()
}

async fn body_json_or_empty(resp: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(resp.into_body(), TEST_RESPONSE_BODY_LIMIT)
        .await
        .expect("read body");
    if bytes.is_empty() {
        return Value::Null;
    }
    serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()))
}

/// Insert a security finding for a release directly into the database.
async fn insert_security_finding(
    pool: &PgPool,
    release_id: uuid::Uuid,
    kind: &str,
    severity: &str,
    title: &str,
    is_resolved: bool,
) -> uuid::Uuid {
    let finding_id = uuid::Uuid::new_v4();

    sqlx::query(
        "INSERT INTO security_findings \
         (id, release_id, kind, severity, title, is_resolved, detected_at) \
         VALUES ($1, $2, $3::finding_kind, $4::security_severity, $5, $6, NOW())",
    )
    .bind(finding_id)
    .bind(release_id)
    .bind(kind)
    .bind(severity)
    .bind(title)
    .bind(is_resolved)
    .execute(pool)
    .await
    .expect("security finding should insert successfully");

    finding_id
}

/// Insert a dist-tag channel ref directly into the database.
async fn insert_channel_ref(
    pool: &PgPool,
    package_id: uuid::Uuid,
    tag: &str,
    release_id: uuid::Uuid,
    created_by: uuid::Uuid,
) -> uuid::Uuid {
    let channel_ref_id = uuid::Uuid::new_v4();

    sqlx::query(
        "INSERT INTO channel_refs \
         (id, package_id, ecosystem, name, release_id, created_by, created_at, updated_at) \
         VALUES ($1, $2, 'npm', $3, $4, $5, NOW(), NOW())",
    )
    .bind(channel_ref_id)
    .bind(package_id)
    .bind(tag)
    .bind(release_id)
    .bind(created_by)
    .execute(pool)
    .await
    .expect("channel ref should insert successfully");

    channel_ref_id
}

/// Insert an organization-scoped audit log at a fixed timestamp directly into the database.
async fn insert_org_audit_log(
    pool: &PgPool,
    org_id: uuid::Uuid,
    actor_user_id: uuid::Uuid,
    action: &str,
    metadata: Value,
    occurred_at: &str,
) -> uuid::Uuid {
    let audit_id = uuid::Uuid::new_v4();

    sqlx::query(
        "INSERT INTO audit_logs \
         (id, action, actor_user_id, target_org_id, metadata, occurred_at) \
         VALUES ($1, $2::audit_action, $3, $4, $5, $6::timestamptz)",
    )
    .bind(audit_id)
    .bind(action)
    .bind(actor_user_id)
    .bind(org_id)
    .bind(metadata)
    .bind(occurred_at)
    .execute(pool)
    .await
    .expect("audit log should insert successfully");

    audit_id
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

#[sqlx::test(migrations = "../../migrations")]
async fn test_platform_admin_jobs_requires_platform_admin_access(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, body) = list_platform_admin_jobs(&app, &alice_jwt, None).await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(body["error"]
        .as_str()
        .expect("error message should be present")
        .contains("audit:read"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_platform_admin_jobs_expose_summary_and_filterable_results(pool: PgPool) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    promote_user_to_platform_admin(&pool, "alice").await;
    let admin_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    sqlx::query("DELETE FROM background_jobs")
        .execute(&pool)
        .await
        .expect("background jobs should be clear before seeding admin-job tests");

    sqlx::query(
        "INSERT INTO background_jobs \
         (id, kind, payload, status, attempts, max_attempts, last_error, scheduled_at, locked_until, locked_by, started_at, completed_at, created_at) \
         VALUES \
         ($1, 'scan_artifact'::job_kind, $2, 'pending'::job_status, 0, 5, NULL, NOW() - INTERVAL '45 minutes', NULL, NULL, NULL, NULL, NOW() - INTERVAL '45 minutes'), \
         ($3, 'cleanup_oci_blobs'::job_kind, $4, 'running'::job_status, 1, 5, NULL, NOW() - INTERVAL '20 minutes', NOW() - INTERVAL '5 minutes', 'worker-a', NOW() - INTERVAL '20 minutes', NULL, NOW() - INTERVAL '20 minutes'), \
         ($5, 'index_package'::job_kind, $6, 'completed'::job_status, 1, 5, NULL, NOW() - INTERVAL '10 minutes', NULL, NULL, NOW() - INTERVAL '10 minutes', NOW() - INTERVAL '9 minutes', NOW() - INTERVAL '10 minutes'), \
         ($7, 'reindex_search'::job_kind, $8, 'dead'::job_status, 5, 5, 'boom', NOW() - INTERVAL '8 minutes', NULL, NULL, NOW() - INTERVAL '8 minutes', NOW() - INTERVAL '7 minutes', NOW() - INTERVAL '8 minutes')",
    )
    .bind(Uuid::new_v4())
    .bind(json!({ "artifact_id": "artifact-123" }))
    .bind(Uuid::new_v4())
    .bind(json!({ "digests": ["sha256:abc"] }))
    .bind(Uuid::new_v4())
    .bind(json!({ "package_id": Uuid::new_v4() }))
    .bind(Uuid::new_v4())
    .bind(json!({ "package_id": Uuid::new_v4() }))
    .execute(&pool)
    .await
    .expect("background jobs should seed successfully");

    let (status, body) =
        list_platform_admin_jobs(&app, &admin_jwt, Some("state=pending&kind=scan_artifact")).await;

    assert_eq!(status, StatusCode::OK, "response: {body}");
    assert_eq!(body["page"], 1);
    assert_eq!(body["per_page"], 50);
    assert_eq!(body["total"], 1);
    assert_eq!(body["filters"]["state"], "pending");
    assert_eq!(body["filters"]["kind"], "scan_artifact");
    assert_eq!(body["summary"]["by_status"]["pending"], 1);
    assert_eq!(body["summary"]["by_status"]["running"], 1);
    assert_eq!(body["summary"]["by_status"]["completed"], 1);
    assert_eq!(body["summary"]["by_status"]["failed"], 0);
    assert_eq!(body["summary"]["by_status"]["dead"], 1);
    assert_eq!(body["summary"]["by_kind"]["scan_artifact"], 1);
    assert_eq!(body["summary"]["by_kind"]["cleanup_oci_blobs"], 1);
    assert_eq!(body["summary"]["by_kind"]["index_package"], 1);
    assert_eq!(body["summary"]["by_kind"]["reindex_search"], 1);
    assert_eq!(body["summary"]["stale_jobs_count"], 1);
    assert!(
        body["summary"]["oldest_pending_age_minutes"]
            .as_i64()
            .expect("oldest pending age should be present")
            >= 44
    );

    let jobs = body["jobs"]
        .as_array()
        .expect("jobs response should be an array");
    assert_eq!(jobs.len(), 1, "response: {body}");
    assert_eq!(jobs[0]["kind"], "scan_artifact");
    assert_eq!(jobs[0]["status"], "pending");
    assert_eq!(jobs[0]["payload"]["artifact_id"], "artifact-123");
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_platform_stats_include_security_artifact_and_queue_metrics(pool: PgPool) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &alice_jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED, "response: {org_body}");
    let org_id = org_body["id"].as_str().expect("org id should be returned");

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "Acme Public",
        "acme-public",
        Some(org_id),
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, package_body) = create_package_with_options(
        &app,
        &alice_jwt,
        "npm",
        "stats-widget",
        "acme-public",
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );

    let alice_id: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE username = $1")
        .bind("alice")
        .fetch_one(&pool)
        .await
        .expect("alice user id should be queryable");
    let package_id = Uuid::parse_str(
        package_body["id"]
            .as_str()
            .expect("package id should be returned"),
    )
    .expect("package id should parse");
    let release_id = Uuid::new_v4();
    let artifact_id = Uuid::new_v4();

    sqlx::query(
        "INSERT INTO releases \
         (id, package_id, version, status, published_by, published_at, updated_at) \
         VALUES ($1, $2, $3, 'published'::release_status, $4, NOW(), NOW())",
    )
    .bind(release_id)
    .bind(package_id)
    .bind("1.0.0")
    .bind(alice_id)
    .execute(&pool)
    .await
    .expect("published release should insert successfully");

    sqlx::query(
        "INSERT INTO artifacts \
         (id, release_id, kind, filename, storage_key, content_type, size_bytes, sha256, uploaded_at) \
         VALUES ($1, $2, 'tarball'::artifact_kind, $3, $4, 'application/octet-stream', 42, $5, NOW())",
    )
    .bind(artifact_id)
    .bind(release_id)
    .bind("stats-widget-1.0.0.tgz")
    .bind("artifacts/stats-widget-1.0.0.tgz")
    .bind("abc123")
    .execute(&pool)
    .await
    .expect("artifact should insert successfully");

    insert_security_finding(
        &pool,
        release_id,
        "vulnerability",
        "high",
        "Stats regression finding",
        false,
    )
    .await;

    sqlx::query("DELETE FROM background_jobs")
        .execute(&pool)
        .await
        .expect("background jobs should be clear before seeding stats tests");

    sqlx::query(
        "INSERT INTO background_jobs \
         (id, kind, payload, status, attempts, max_attempts, scheduled_at, created_at) \
         VALUES ($1, 'scan_artifact'::job_kind, $2, 'pending'::job_status, 0, 5, NOW(), NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(json!({ "release_id": release_id }))
    .execute(&pool)
    .await
    .expect("pending background job should insert successfully");

    let (status, body) = get_platform_stats(&app).await;

    assert_eq!(status, StatusCode::OK, "response: {body}");
    assert_eq!(body["packages"], 1);
    assert_eq!(body["releases"], 1);
    assert_eq!(body["organizations"], 1);
    assert_eq!(body["security_findings_total"], 1);
    assert_eq!(body["security_findings_unresolved"], 1);
    assert_eq!(body["artifacts_stored"], 1);
    assert_eq!(body["job_queue_pending"], 1);
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
    let tokens = body["tokens"]
        .as_array()
        .expect("response should expose a tokens array");
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

    let req = Request::get("/v1/users/nobody")
        .body(Body::empty())
        .unwrap();
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
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let admin_jwt = login_user(&app, "bob", "super_secret_pw!").await;

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

    let (status, body) = add_org_member(&app, &jwt, "acme-corp", "bob", "admin").await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected add member response: {body}"
    );

    // Get org anonymously
    let req = Request::get("/v1/orgs/acme-corp")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["slug"], "acme-corp");
    assert_eq!(body["capabilities"]["can_manage"], false);
    assert_eq!(body["capabilities"]["can_manage_invitations"], false);
    assert_eq!(body["capabilities"]["can_manage_members"], false);
    assert_eq!(body["capabilities"]["can_manage_teams"], false);
    assert_eq!(body["capabilities"]["can_manage_repositories"], false);
    assert_eq!(body["capabilities"]["can_manage_namespaces"], false);
    assert_eq!(body["capabilities"]["can_view_member_directory"], false);
    assert_eq!(body["capabilities"]["can_view_audit_log"], false);
    assert_eq!(body["capabilities"]["can_transfer_ownership"], false);

    // Get org as the owner
    let req = Request::builder()
        .method(Method::GET)
        .uri("/v1/orgs/acme-corp")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["capabilities"]["can_manage"], true);
    assert_eq!(body["capabilities"]["can_manage_invitations"], true);
    assert_eq!(body["capabilities"]["can_manage_members"], true);
    assert_eq!(body["capabilities"]["can_manage_teams"], true);
    assert_eq!(body["capabilities"]["can_manage_repositories"], true);
    assert_eq!(body["capabilities"]["can_manage_namespaces"], true);
    assert_eq!(body["capabilities"]["can_view_member_directory"], true);
    assert_eq!(body["capabilities"]["can_view_audit_log"], true);
    assert_eq!(body["capabilities"]["can_transfer_ownership"], true);

    // Get org as an admin member
    let req = Request::builder()
        .method(Method::GET)
        .uri("/v1/orgs/acme-corp")
        .header(header::AUTHORIZATION, format!("Bearer {admin_jwt}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["capabilities"]["can_manage"], true);
    assert_eq!(body["capabilities"]["can_manage_invitations"], true);
    assert_eq!(body["capabilities"]["can_manage_members"], true);
    assert_eq!(body["capabilities"]["can_manage_teams"], true);
    assert_eq!(body["capabilities"]["can_manage_repositories"], true);
    assert_eq!(body["capabilities"]["can_manage_namespaces"], true);
    assert_eq!(body["capabilities"]["can_view_member_directory"], true);
    assert_eq!(body["capabilities"]["can_view_audit_log"], true);
    assert_eq!(body["capabilities"]["can_transfer_ownership"], false);

    let req = Request::builder()
        .method(Method::GET)
        .uri("/v1/users/me/organizations")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let organizations = body["organizations"]
        .as_array()
        .expect("organizations response should be an array");
    assert_eq!(organizations.len(), 1);
    assert_eq!(organizations[0]["slug"], "acme-corp");
    assert_eq!(organizations[0]["capabilities"]["can_manage"], true);
    assert_eq!(
        organizations[0]["capabilities"]["can_manage_invitations"],
        true
    );
    assert_eq!(organizations[0]["capabilities"]["can_manage_members"], true);
    assert_eq!(organizations[0]["capabilities"]["can_manage_teams"], true);
    assert_eq!(
        organizations[0]["capabilities"]["can_manage_repositories"],
        true
    );
    assert_eq!(
        organizations[0]["capabilities"]["can_manage_namespaces"],
        true
    );
    assert_eq!(
        organizations[0]["capabilities"]["can_view_member_directory"],
        true
    );
    assert_eq!(organizations[0]["capabilities"]["can_view_audit_log"], true);
    assert_eq!(
        organizations[0]["capabilities"]["can_transfer_ownership"],
        true
    );
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
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
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
        bob_memberships[0]["role"], "maintainer",
        "members response: {body}"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_org_member_and_team_directory_reads_require_org_membership(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let owner_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let outsider_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, _) = create_org(&app, &owner_jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_team(
        &app,
        &owner_jwt,
        "acme-corp",
        "Release Engineering",
        "release-engineering",
        Some("Owns package publication workflows"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let anonymous_members_req = Request::builder()
        .method(Method::GET)
        .uri("/v1/orgs/acme-corp/members")
        .body(Body::empty())
        .unwrap();
    let anonymous_members_resp = app.clone().oneshot(anonymous_members_req).await.unwrap();
    assert_eq!(anonymous_members_resp.status(), StatusCode::UNAUTHORIZED);

    let outsider_members_req = Request::builder()
        .method(Method::GET)
        .uri("/v1/orgs/acme-corp/members")
        .header(header::AUTHORIZATION, format!("Bearer {outsider_jwt}"))
        .body(Body::empty())
        .unwrap();
    let outsider_members_resp = app.clone().oneshot(outsider_members_req).await.unwrap();
    assert_eq!(outsider_members_resp.status(), StatusCode::FORBIDDEN);

    let member_members_req = Request::builder()
        .method(Method::GET)
        .uri("/v1/orgs/acme-corp/members")
        .header(header::AUTHORIZATION, format!("Bearer {owner_jwt}"))
        .body(Body::empty())
        .unwrap();
    let member_members_resp = app.clone().oneshot(member_members_req).await.unwrap();
    assert_eq!(member_members_resp.status(), StatusCode::OK);

    let anonymous_teams_req = Request::builder()
        .method(Method::GET)
        .uri("/v1/orgs/acme-corp/teams")
        .body(Body::empty())
        .unwrap();
    let anonymous_teams_resp = app.clone().oneshot(anonymous_teams_req).await.unwrap();
    assert_eq!(anonymous_teams_resp.status(), StatusCode::UNAUTHORIZED);

    let outsider_teams_req = Request::builder()
        .method(Method::GET)
        .uri("/v1/orgs/acme-corp/teams")
        .header(header::AUTHORIZATION, format!("Bearer {outsider_jwt}"))
        .body(Body::empty())
        .unwrap();
    let outsider_teams_resp = app.clone().oneshot(outsider_teams_req).await.unwrap();
    assert_eq!(outsider_teams_resp.status(), StatusCode::FORBIDDEN);

    let member_teams_req = Request::builder()
        .method(Method::GET)
        .uri("/v1/orgs/acme-corp/teams")
        .header(header::AUTHORIZATION, format!("Bearer {owner_jwt}"))
        .body(Body::empty())
        .unwrap();
    let member_teams_resp = app.clone().oneshot(member_teams_req).await.unwrap();
    assert_eq!(member_teams_resp.status(), StatusCode::OK);

    let anonymous_search_req = Request::builder()
        .method(Method::GET)
        .uri("/v1/orgs/acme-corp/members/search?query=al")
        .body(Body::empty())
        .unwrap();
    let anonymous_search_resp = app.clone().oneshot(anonymous_search_req).await.unwrap();
    assert_eq!(anonymous_search_resp.status(), StatusCode::UNAUTHORIZED);

    let outsider_search_req = Request::builder()
        .method(Method::GET)
        .uri("/v1/orgs/acme-corp/members/search?query=al")
        .header(header::AUTHORIZATION, format!("Bearer {outsider_jwt}"))
        .body(Body::empty())
        .unwrap();
    let outsider_search_resp = app.clone().oneshot(outsider_search_req).await.unwrap();
    assert_eq!(outsider_search_resp.status(), StatusCode::FORBIDDEN);

    let member_search_req = Request::builder()
        .method(Method::GET)
        .uri("/v1/orgs/acme-corp/members/search?query=al")
        .header(header::AUTHORIZATION, format!("Bearer {owner_jwt}"))
        .body(Body::empty())
        .unwrap();
    let member_search_resp = app.clone().oneshot(member_search_req).await.unwrap();
    assert_eq!(member_search_resp.status(), StatusCode::OK);
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_private_org_member_directory_restricts_viewers_without_hiding_private_packages(
    pool: PgPool,
) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    register_user(&app, "charlie", "charlie@test.dev", "super_secret_pw!").await;
    let owner_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let admin_jwt = login_user(&app, "bob", "super_secret_pw!").await;
    let viewer_jwt = login_user(&app, "charlie", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &owner_jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id");

    let (status, _) = add_org_member(&app, &owner_jwt, "acme-corp", "bob", "admin").await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = add_org_member(&app, &owner_jwt, "acme-corp", "charlie", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_team(
        &app,
        &owner_jwt,
        "acme-corp",
        "Release Engineering",
        "release-engineering",
        Some("Owns package publication workflows"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, repository_body) = create_repository_with_options(
        &app,
        &owner_jwt,
        "Acme Private Packages",
        "acme-private-packages",
        Some(org_id),
        Some("private"),
        Some("private"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, package_body) = create_package(
        &app,
        &owner_jwt,
        "npm",
        "acme-private-widget",
        "acme-private-packages",
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );

    let viewer_before_req = Request::builder()
        .method(Method::GET)
        .uri("/v1/orgs/acme-corp")
        .header(header::AUTHORIZATION, format!("Bearer {viewer_jwt}"))
        .body(Body::empty())
        .unwrap();
    let viewer_before_resp = app.clone().oneshot(viewer_before_req).await.unwrap();
    assert_eq!(viewer_before_resp.status(), StatusCode::OK);
    let viewer_before_body = body_json(viewer_before_resp).await;
    assert_eq!(
        viewer_before_body["capabilities"]["can_view_member_directory"],
        true
    );

    let (status, updated_org_body) = update_org_profile(
        &app,
        &owner_jwt,
        "acme-corp",
        json!({ "member_directory_is_private": true }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected org update response: {updated_org_body}"
    );
    assert_eq!(updated_org_body["message"], "Organization updated");
    assert_eq!(updated_org_body["member_directory_is_private"], true);

    let viewer_org_req = Request::builder()
        .method(Method::GET)
        .uri("/v1/orgs/acme-corp")
        .header(header::AUTHORIZATION, format!("Bearer {viewer_jwt}"))
        .body(Body::empty())
        .unwrap();
    let viewer_org_resp = app.clone().oneshot(viewer_org_req).await.unwrap();
    assert_eq!(viewer_org_resp.status(), StatusCode::OK);
    let viewer_org_body = body_json(viewer_org_resp).await;
    assert_eq!(viewer_org_body["member_directory_is_private"], true);
    assert_eq!(
        viewer_org_body["capabilities"]["can_view_member_directory"],
        false
    );

    let admin_org_req = Request::builder()
        .method(Method::GET)
        .uri("/v1/orgs/acme-corp")
        .header(header::AUTHORIZATION, format!("Bearer {admin_jwt}"))
        .body(Body::empty())
        .unwrap();
    let admin_org_resp = app.clone().oneshot(admin_org_req).await.unwrap();
    assert_eq!(admin_org_resp.status(), StatusCode::OK);
    let admin_org_body = body_json(admin_org_resp).await;
    assert_eq!(
        admin_org_body["capabilities"]["can_view_member_directory"],
        true
    );

    let viewer_members_req = Request::builder()
        .method(Method::GET)
        .uri("/v1/orgs/acme-corp/members")
        .header(header::AUTHORIZATION, format!("Bearer {viewer_jwt}"))
        .body(Body::empty())
        .unwrap();
    let viewer_members_resp = app.clone().oneshot(viewer_members_req).await.unwrap();
    assert_eq!(viewer_members_resp.status(), StatusCode::FORBIDDEN);

    let viewer_teams_req = Request::builder()
        .method(Method::GET)
        .uri("/v1/orgs/acme-corp/teams")
        .header(header::AUTHORIZATION, format!("Bearer {viewer_jwt}"))
        .body(Body::empty())
        .unwrap();
    let viewer_teams_resp = app.clone().oneshot(viewer_teams_req).await.unwrap();
    assert_eq!(viewer_teams_resp.status(), StatusCode::FORBIDDEN);

    let viewer_search_req = Request::builder()
        .method(Method::GET)
        .uri("/v1/orgs/acme-corp/members/search?query=al")
        .header(header::AUTHORIZATION, format!("Bearer {viewer_jwt}"))
        .body(Body::empty())
        .unwrap();
    let viewer_search_resp = app.clone().oneshot(viewer_search_req).await.unwrap();
    assert_eq!(viewer_search_resp.status(), StatusCode::FORBIDDEN);

    let admin_members_req = Request::builder()
        .method(Method::GET)
        .uri("/v1/orgs/acme-corp/members")
        .header(header::AUTHORIZATION, format!("Bearer {admin_jwt}"))
        .body(Body::empty())
        .unwrap();
    let admin_members_resp = app.clone().oneshot(admin_members_req).await.unwrap();
    assert_eq!(admin_members_resp.status(), StatusCode::OK);

    let viewer_packages_req = Request::builder()
        .method(Method::GET)
        .uri("/v1/orgs/acme-corp/packages")
        .header(header::AUTHORIZATION, format!("Bearer {viewer_jwt}"))
        .body(Body::empty())
        .unwrap();
    let viewer_packages_resp = app.clone().oneshot(viewer_packages_req).await.unwrap();
    assert_eq!(viewer_packages_resp.status(), StatusCode::OK);
    let viewer_packages_body = body_json(viewer_packages_resp).await;
    assert_eq!(
        viewer_packages_body["packages"][0]["name"],
        "acme-private-widget"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_update_org_profile_success(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, _) = create_org(&app, &jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = update_org_profile(
        &app,
        &jwt,
        "acme-corp",
        json!({
            "description": "Maintains all Acme package distribution.",
            "website": " https://packages.acme.test ",
            "email": " registry@acme.test ",
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected org update response: {body}"
    );
    assert_eq!(body["message"], "Organization updated");

    let req = Request::get("/v1/orgs/acme-corp")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(
        body["description"],
        "Maintains all Acme package distribution."
    );
    assert_eq!(body["website"], "https://packages.acme.test");
    assert_eq!(body["email"], "registry@acme.test");
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_update_org_profile_requires_org_admin(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let owner_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, _) = create_org(&app, &owner_jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = add_org_member(&app, &owner_jwt, "acme-corp", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = update_org_profile(
        &app,
        &bob_jwt,
        "acme-corp",
        json!({
            "description": "Should not be allowed",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(body["error"]
        .as_str()
        .expect("error should be present")
        .contains("owner or admin"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_org_repository_list_respects_visibility_and_package_counts(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id should be returned");

    let (status, public_repo_body) = create_repository_with_options(
        &app,
        &jwt,
        "Acme Public",
        "acme-public",
        Some(org_id),
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected public repository response: {public_repo_body}"
    );

    let (status, internal_repo_body) = create_repository_with_options(
        &app,
        &jwt,
        "Acme Internal",
        "acme-internal",
        Some(org_id),
        Some("private"),
        Some("internal_org"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected internal repository response: {internal_repo_body}"
    );

    let (status, body) = create_package_with_options(
        &app,
        &jwt,
        "npm",
        "acme-public-widget",
        "acme-public",
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {body}"
    );

    let (status, body) = create_package_with_options(
        &app,
        &jwt,
        "npm",
        "acme-private-widget",
        "acme-public",
        Some("private"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {body}"
    );

    let (status, body) = create_package_with_options(
        &app,
        &jwt,
        "npm",
        "acme-internal-widget",
        "acme-internal",
        Some("internal_org"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {body}"
    );

    let (status, owner_body) = list_org_repositories(&app, Some(&jwt), "acme-corp").await;
    assert_eq!(status, StatusCode::OK);
    let owner_repositories = owner_body["repositories"]
        .as_array()
        .expect("owner repositories response should be an array");
    assert_eq!(
        owner_repositories.len(),
        2,
        "owner repositories response: {owner_body}"
    );

    let public_repository = owner_repositories
        .iter()
        .find(|repo| repo["slug"] == "acme-public")
        .expect("public repository should be returned to the owner");
    assert_eq!(public_repository["kind"], "public");
    assert_eq!(public_repository["visibility"], "public");
    assert_eq!(public_repository["package_count"], 2);

    let internal_repository = owner_repositories
        .iter()
        .find(|repo| repo["slug"] == "acme-internal")
        .expect("internal repository should be returned to the owner");
    assert_eq!(internal_repository["kind"], "private");
    assert_eq!(internal_repository["visibility"], "internal_org");
    assert_eq!(internal_repository["package_count"], 1);

    let (status, anonymous_body) = list_org_repositories(&app, None, "acme-corp").await;
    assert_eq!(status, StatusCode::OK);
    let anonymous_repositories = anonymous_body["repositories"]
        .as_array()
        .expect("anonymous repositories response should be an array");
    assert_eq!(
        anonymous_repositories.len(),
        1,
        "anonymous repositories response: {anonymous_body}"
    );
    assert_eq!(anonymous_repositories[0]["slug"], "acme-public");
    assert_eq!(anonymous_repositories[0]["visibility"], "public");
    assert_eq!(anonymous_repositories[0]["package_count"], 1);

    let (status, owner_coverage_body) =
        list_org_repository_package_coverage(&app, Some(&jwt), "acme-corp").await;
    assert_eq!(status, StatusCode::OK);
    let owner_coverage = owner_coverage_body["repositories"]
        .as_array()
        .expect("owner repository coverage should be an array");
    assert_eq!(owner_coverage.len(), 2, "response: {owner_coverage_body}");
    let owner_public_coverage = owner_coverage
        .iter()
        .find(|repo| repo["repository_slug"] == "acme-public")
        .expect("public repository coverage should be present for owner");
    let owner_public_packages = owner_public_coverage["packages"]
        .as_array()
        .expect("owner public coverage packages should be an array");
    assert_eq!(
        owner_public_packages.len(),
        2,
        "response: {owner_coverage_body}"
    );
    for package_name in ["acme-public-widget", "acme-private-widget"] {
        assert!(
            owner_public_packages
                .iter()
                .any(|package| package["name"] == package_name),
            "expected {package_name} in response: {owner_coverage_body}"
        );
    }
    let owner_internal_coverage = owner_coverage
        .iter()
        .find(|repo| repo["repository_slug"] == "acme-internal")
        .expect("internal repository coverage should be present for owner");
    let owner_internal_packages = owner_internal_coverage["packages"]
        .as_array()
        .expect("owner internal coverage packages should be an array");
    assert_eq!(
        owner_internal_packages.len(),
        1,
        "response: {owner_coverage_body}"
    );
    assert_eq!(owner_internal_packages[0]["name"], "acme-internal-widget");

    let (status, anonymous_coverage_body) =
        list_org_repository_package_coverage(&app, None, "acme-corp").await;
    assert_eq!(status, StatusCode::OK);
    let anonymous_coverage = anonymous_coverage_body["repositories"]
        .as_array()
        .expect("anonymous repository coverage should be an array");
    assert_eq!(
        anonymous_coverage.len(),
        1,
        "response: {anonymous_coverage_body}"
    );
    assert_eq!(anonymous_coverage[0]["repository_slug"], "acme-public");
    let anonymous_coverage_packages = anonymous_coverage[0]["packages"]
        .as_array()
        .expect("anonymous repository coverage packages should be an array");
    assert_eq!(
        anonymous_coverage_packages.len(),
        1,
        "response: {anonymous_coverage_body}"
    );
    assert_eq!(anonymous_coverage_packages[0]["name"], "acme-public-widget");
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_org_workspace_bootstrap_aggregates_initial_workspace_data(pool: PgPool) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    register_user(&app, "charlie", "charlie@test.dev", "super_secret_pw!").await;
    let owner_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &owner_jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id should be returned");
    let org_uuid = Uuid::parse_str(org_id).expect("org id should be a uuid");

    let (status, team_body) = create_team(
        &app,
        &owner_jwt,
        "acme-corp",
        "Release Engineering",
        "release-engineering",
        Some("Owns release workflows."),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected team response: {team_body}"
    );

    let (status, member_body) =
        add_org_member(&app, &owner_jwt, "acme-corp", "bob", "viewer").await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected org member response: {member_body}"
    );

    let (status, team_member_body) =
        add_team_member_to_team(&app, &owner_jwt, "acme-corp", "release-engineering", "bob").await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected team member response: {team_member_body}"
    );

    let (status, public_repository_body) = create_repository_with_options(
        &app,
        &owner_jwt,
        "Acme Public",
        "acme-public",
        Some(org_id),
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected public repository response: {public_repository_body}"
    );

    let (status, internal_repository_body) = create_repository_with_options(
        &app,
        &owner_jwt,
        "Acme Internal",
        "acme-internal",
        Some(org_id),
        Some("private"),
        Some("internal_org"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected internal repository response: {internal_repository_body}"
    );

    let (status, public_package_body) = create_package_with_options(
        &app,
        &owner_jwt,
        "npm",
        "acme-public-widget",
        "acme-public",
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected public package response: {public_package_body}"
    );

    let (status, internal_package_body) = create_package_with_options(
        &app,
        &owner_jwt,
        "npm",
        "acme-internal-widget",
        "acme-internal",
        Some("internal_org"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected internal package response: {internal_package_body}"
    );

    let (status, release_body) =
        create_release_for_package(&app, &owner_jwt, "npm", "acme-public-widget", "1.0.0").await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected release response: {release_body}"
    );

    let public_release_id = get_release_id(&pool, "npm", "acme-public-widget", "1.0.0").await;
    insert_security_finding(
        &pool,
        public_release_id,
        "vulnerability",
        "high",
        "Public bootstrap issue",
        false,
    )
    .await;

    let (status, namespace_body) =
        create_namespace_claim(&app, &owner_jwt, "npm", "@acme", Some(org_id)).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected namespace response: {namespace_body}"
    );
    let namespace_claim_id = namespace_body["id"]
        .as_str()
        .expect("namespace claim id should be returned");

    let (status, team_package_access_body) = grant_team_package_access(
        &app,
        &owner_jwt,
        "acme-corp",
        "release-engineering",
        "npm",
        "acme-public-widget",
        &["write_metadata"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected team package access response: {team_package_access_body}"
    );

    let (status, team_repository_access_body) = grant_team_repository_access(
        &app,
        &owner_jwt,
        "acme-corp",
        "release-engineering",
        "acme-internal",
        &["publish"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected team repository access response: {team_repository_access_body}"
    );

    let (status, team_namespace_access_body) = grant_team_namespace_access(
        &app,
        &owner_jwt,
        "acme-corp",
        "release-engineering",
        namespace_claim_id,
        &["admin"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected team namespace access response: {team_namespace_access_body}"
    );

    let (status, invitation_body) =
        send_org_invitation(&app, &owner_jwt, "acme-corp", "charlie", "viewer", 7).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected invitation response: {invitation_body}"
    );

    let (status, owner_bootstrap_body) =
        get_org_workspace_bootstrap(&app, Some(&owner_jwt), "acme-corp").await;
    assert_eq!(status, StatusCode::OK, "response: {owner_bootstrap_body}");
    assert_eq!(owner_bootstrap_body["org"]["slug"], "acme-corp");
    assert_eq!(
        owner_bootstrap_body["org"]["capabilities"]["can_manage_invitations"],
        true
    );
    assert_eq!(
        owner_bootstrap_body["teams"]
            .as_array()
            .expect("teams should be an array")
            .len(),
        1
    );
    assert_eq!(
        owner_bootstrap_body["repositories"]
            .as_array()
            .expect("repositories should be an array")
            .len(),
        2
    );
    assert_eq!(
        owner_bootstrap_body["repository_package_coverage"]
            .as_array()
            .expect("repository coverage should be an array")
            .len(),
        2
    );
    assert_eq!(
        owner_bootstrap_body["packages"]
            .as_array()
            .expect("packages should be an array")
            .len(),
        2
    );
    assert_eq!(
        owner_bootstrap_body["namespaces"]
            .as_array()
            .expect("namespaces should be an array")
            .len(),
        1
    );
    assert_eq!(
        owner_bootstrap_body["invitations"]
            .as_array()
            .expect("invitations should be an array")
            .len(),
        1
    );
    assert_eq!(
        owner_bootstrap_body["security"]["summary"]["open_findings"],
        1
    );
    assert_eq!(
        owner_bootstrap_body["security"]["summary"]["affected_packages"],
        1
    );
    assert_eq!(
        owner_bootstrap_body["team_management"]["members_by_team_slug"]["release-engineering"]
            .as_array()
            .expect("team members should be an array")
            .len(),
        1
    );
    assert_eq!(
        owner_bootstrap_body["team_management"]["members_by_team_slug"]["release-engineering"][0]
            ["username"],
        "bob"
    );
    assert_eq!(
        owner_bootstrap_body["team_management"]["package_access_by_team_slug"]
            ["release-engineering"]
            .as_array()
            .expect("team package access should be an array")
            .len(),
        1
    );
    assert_eq!(
        owner_bootstrap_body["team_management"]["package_access_by_team_slug"]
            ["release-engineering"][0]["name"],
        "acme-public-widget"
    );
    assert_eq!(
        owner_bootstrap_body["team_management"]["repository_access_by_team_slug"]
            ["release-engineering"]
            .as_array()
            .expect("team repository access should be an array")
            .len(),
        1
    );
    assert_eq!(
        owner_bootstrap_body["team_management"]["repository_access_by_team_slug"]
            ["release-engineering"][0]["slug"],
        "acme-internal"
    );
    assert_eq!(
        owner_bootstrap_body["team_management"]["namespace_access_by_team_slug"]
            ["release-engineering"]
            .as_array()
            .expect("team namespace access should be an array")
            .len(),
        1
    );
    assert_eq!(
        owner_bootstrap_body["team_management"]["namespace_access_by_team_slug"]
            ["release-engineering"][0]["namespace"],
        "@acme"
    );
    assert!(
        owner_bootstrap_body["repository_package_coverage"]
            .as_array()
            .expect("repository coverage should be an array")
            .iter()
            .any(|entry| entry["repository_slug"] == "acme-internal"),
        "expected internal repository coverage in response: {owner_bootstrap_body}"
    );

    let (status, anonymous_bootstrap_body) =
        get_org_workspace_bootstrap(&app, None, "acme-corp").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "response: {anonymous_bootstrap_body}"
    );
    assert_eq!(
        anonymous_bootstrap_body["teams"]
            .as_array()
            .expect("anonymous teams should be an array")
            .len(),
        0
    );
    assert_eq!(
        anonymous_bootstrap_body["invitations"]
            .as_array()
            .expect("anonymous invitations should be an array")
            .len(),
        0
    );
    assert_eq!(
        anonymous_bootstrap_body["repositories"]
            .as_array()
            .expect("anonymous repositories should be an array")
            .len(),
        1
    );
    assert_eq!(
        anonymous_bootstrap_body["packages"]
            .as_array()
            .expect("anonymous packages should be an array")
            .len(),
        1
    );
    assert_eq!(
        anonymous_bootstrap_body["repository_package_coverage"]
            .as_array()
            .expect("anonymous repository coverage should be an array")
            .len(),
        1
    );
    assert_eq!(
        anonymous_bootstrap_body["namespaces"]
            .as_array()
            .expect("anonymous namespaces should be an array")
            .len(),
        1
    );
    assert_eq!(
        anonymous_bootstrap_body["security"]["summary"]["open_findings"],
        1
    );
    assert_eq!(
        anonymous_bootstrap_body["security"]["summary"]["affected_packages"],
        1
    );
    assert_eq!(
        anonymous_bootstrap_body["team_management"]["members_by_team_slug"],
        json!({})
    );
    assert_eq!(
        anonymous_bootstrap_body["team_management"]["package_access_by_team_slug"],
        json!({})
    );
    assert_eq!(
        anonymous_bootstrap_body["team_management"]["repository_access_by_team_slug"],
        json!({})
    );
    assert_eq!(
        anonymous_bootstrap_body["team_management"]["namespace_access_by_team_slug"],
        json!({})
    );

    let namespace_owner_org_id = anonymous_bootstrap_body["namespaces"][0]["owner_org_id"]
        .as_str()
        .expect("namespace owner org id should be returned");
    assert_eq!(namespace_owner_org_id, org_uuid.to_string());
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_org_admin_can_update_repository_details(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id should be returned");

    let (status, repository_body) = create_repository_with_options(
        &app,
        &jwt,
        "Acme Public",
        "acme-public",
        Some(org_id),
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, body) = update_repository_detail(
        &app,
        &jwt,
        "acme-public",
        json!({
            "description": "Canonical public repository for Acme releases.",
            "visibility": "unlisted",
            "upstream_url": "https://git.example.test/acme/public",
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected repository update response: {body}"
    );
    assert_eq!(body["message"], "Repository updated");

    let (status, repository_body) = get_repository_detail(&app, Some(&jwt), "acme-public").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        repository_body["description"],
        "Canonical public repository for Acme releases."
    );
    assert_eq!(repository_body["visibility"], "unlisted");
    assert_eq!(
        repository_body["upstream_url"],
        "https://git.example.test/acme/public"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_repository_creation_rejects_proxy_and_virtual_kinds(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, proxy_body) = create_repository_with_options(
        &app,
        &jwt,
        "Acme Proxy",
        "acme-proxy",
        None,
        Some("proxy"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "unexpected proxy repository response: {proxy_body}"
    );
    let proxy_error = proxy_body["error"]
        .as_str()
        .expect("proxy repository rejection should return an error");
    assert!(proxy_error.contains("public, private, staging, and release"));
    assert!(proxy_error.contains("Proxy and virtual repositories"));

    let (status, virtual_body) = create_repository_with_options(
        &app,
        &jwt,
        "Acme Virtual",
        "acme-virtual",
        None,
        Some("virtual"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "unexpected virtual repository response: {virtual_body}"
    );
    let virtual_error = virtual_body["error"]
        .as_str()
        .expect("virtual repository rejection should return an error");
    assert!(virtual_error.contains("public, private, staging, and release"));
    assert!(virtual_error.contains("Proxy and virtual repositories"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_repository_package_list_respects_visibility(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let owner_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let member_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &owner_jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id should be returned");

    let (status, body) = add_org_member(&app, &owner_jwt, "acme-corp", "bob", "viewer").await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected member response: {body}"
    );

    let (status, public_repo_body) = create_repository_with_options(
        &app,
        &owner_jwt,
        "Acme Public",
        "acme-public",
        Some(org_id),
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected public repository response: {public_repo_body}"
    );

    let (status, internal_repo_body) = create_repository_with_options(
        &app,
        &owner_jwt,
        "Acme Internal",
        "acme-internal",
        Some(org_id),
        Some("private"),
        Some("internal_org"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected internal repository response: {internal_repo_body}"
    );

    for (name, visibility) in [
        ("acme-public-widget", Some("public")),
        ("acme-unlisted-widget", Some("unlisted")),
        ("acme-private-widget", Some("private")),
    ] {
        let (status, body) =
            create_package_with_options(&app, &owner_jwt, "npm", name, "acme-public", visibility)
                .await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "unexpected package response: {body}"
        );
    }

    let (status, body) = create_package_with_options(
        &app,
        &owner_jwt,
        "npm",
        "acme-internal-widget",
        "acme-internal",
        Some("internal_org"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {body}"
    );

    let (status, anonymous_public_body) = list_repository_packages(&app, None, "acme-public").await;
    assert_eq!(status, StatusCode::OK);
    let anonymous_public_packages = anonymous_public_body["packages"]
        .as_array()
        .expect("anonymous public package list should be an array");
    assert_eq!(
        anonymous_public_packages.len(),
        1,
        "response: {anonymous_public_body}"
    );
    assert_eq!(anonymous_public_packages[0]["name"], "acme-public-widget");

    let (status, member_public_body) =
        list_repository_packages(&app, Some(&member_jwt), "acme-public").await;
    assert_eq!(status, StatusCode::OK);
    let member_public_packages = member_public_body["packages"]
        .as_array()
        .expect("member public package list should be an array");
    assert_eq!(
        member_public_packages.len(),
        3,
        "response: {member_public_body}"
    );

    for package_name in [
        "acme-public-widget",
        "acme-unlisted-widget",
        "acme-private-widget",
    ] {
        assert!(
            member_public_packages
                .iter()
                .any(|package| package["name"] == package_name),
            "expected {package_name} in response: {member_public_body}"
        );
    }

    let (status, anonymous_internal_body) =
        list_repository_packages(&app, None, "acme-internal").await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "response: {anonymous_internal_body}"
    );

    let (status, member_internal_body) =
        list_repository_packages(&app, Some(&member_jwt), "acme-internal").await;
    assert_eq!(status, StatusCode::OK);
    let member_internal_packages = member_internal_body["packages"]
        .as_array()
        .expect("member internal package list should be an array");
    assert_eq!(
        member_internal_packages.len(),
        1,
        "response: {member_internal_body}"
    );
    assert_eq!(member_internal_packages[0]["name"], "acme-internal-widget");
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_repository_detail_respects_direct_read_visibility(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let owner_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let member_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &owner_jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id should be returned");

    let (status, body) = add_org_member(&app, &owner_jwt, "acme-corp", "bob", "viewer").await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected member response: {body}"
    );

    let (status, body) = create_repository_with_options(
        &app,
        &owner_jwt,
        "Acme Public",
        "acme-public",
        Some(org_id),
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {body}"
    );

    let (status, public_body) = get_repository_detail(&app, None, "acme-public").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected public repository response: {public_body}"
    );
    assert_eq!(public_body["slug"], "acme-public");
    assert_eq!(public_body["owner_org_slug"], "acme-corp");
    assert_eq!(public_body["owner_org_name"], "Acme Corp");

    let (status, body) = update_repository_detail(
        &app,
        &owner_jwt,
        "acme-public",
        json!({ "visibility": "unlisted" }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected repository update response: {body}"
    );

    let (status, unlisted_body) = get_repository_detail(&app, None, "acme-public").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected unlisted repository response: {unlisted_body}"
    );
    assert_eq!(unlisted_body["visibility"], "unlisted");

    let (status, body) = create_repository_with_options(
        &app,
        &owner_jwt,
        "Acme Internal",
        "acme-internal",
        Some(org_id),
        Some("private"),
        Some("internal_org"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {body}"
    );

    let (status, anonymous_internal_body) =
        get_repository_detail(&app, None, "acme-internal").await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "unexpected anonymous internal response: {anonymous_internal_body}"
    );

    let (status, member_internal_body) =
        get_repository_detail(&app, Some(&member_jwt), "acme-internal").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected member internal response: {member_internal_body}"
    );
    assert_eq!(member_internal_body["owner_org_slug"], "acme-corp");
    assert_eq!(member_internal_body["owner_org_name"], "Acme Corp");
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_repository_detail_exposes_capabilities_for_org_admins_and_viewers(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let owner_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let viewer_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &owner_jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id should be returned");

    let (status, body) = add_org_member(&app, &owner_jwt, "acme-corp", "bob", "viewer").await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected member response: {body}"
    );

    let (status, body) = create_repository_with_options(
        &app,
        &owner_jwt,
        "Acme Public",
        "acme-public",
        Some(org_id),
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {body}"
    );

    let (status, anonymous_body) = get_repository_detail(&app, None, "acme-public").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected anonymous response: {anonymous_body}"
    );
    assert_eq!(anonymous_body["can_manage"], false);
    assert_eq!(anonymous_body["can_create_packages"], false);

    let (status, viewer_body) = get_repository_detail(&app, Some(&viewer_jwt), "acme-public").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected viewer response: {viewer_body}"
    );
    assert_eq!(viewer_body["can_manage"], false);
    assert_eq!(viewer_body["can_create_packages"], false);

    let (status, owner_body) = get_repository_detail(&app, Some(&owner_jwt), "acme-public").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected owner response: {owner_body}"
    );
    assert_eq!(owner_body["can_manage"], true);
    assert_eq!(owner_body["can_create_packages"], true);
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_repository_detail_exposes_capabilities_for_user_owned_repository_owner(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, body) =
        create_repository(&app, &jwt, "Alice Releases", "alice-releases", None).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {body}"
    );

    let (status, anonymous_body) = get_repository_detail(&app, None, "alice-releases").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected anonymous response: {anonymous_body}"
    );
    assert_eq!(anonymous_body["can_manage"], false);
    assert_eq!(anonymous_body["can_create_packages"], false);

    let (status, owner_body) = get_repository_detail(&app, Some(&jwt), "alice-releases").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected owner response: {owner_body}"
    );
    assert_eq!(owner_body["can_manage"], true);
    assert_eq!(owner_body["can_create_packages"], true);
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_repository_detail_capabilities_follow_token_scopes(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, body) =
        create_repository(&app, &jwt, "Alice Releases", "alice-releases", None).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {body}"
    );

    let (status, repository_token_body) =
        create_personal_access_token(&app, &jwt, "repository-manage", &["repositories:write"])
            .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository token response: {repository_token_body}"
    );
    let repository_token = repository_token_body["token"]
        .as_str()
        .expect("repository token should be returned")
        .to_owned();

    let (status, package_token_body) =
        create_personal_access_token(&app, &jwt, "package-manage", &["packages:write"]).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package token response: {package_token_body}"
    );
    let package_token = package_token_body["token"]
        .as_str()
        .expect("package token should be returned")
        .to_owned();

    let (status, repository_token_detail) =
        get_repository_detail(&app, Some(&repository_token), "alice-releases").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected repository-scope token response: {repository_token_detail}"
    );
    assert_eq!(repository_token_detail["can_manage"], true);
    assert_eq!(repository_token_detail["can_create_packages"], false);

    let (status, package_token_detail) =
        get_repository_detail(&app, Some(&package_token), "alice-releases").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected package-scope token response: {package_token_detail}"
    );
    assert_eq!(package_token_detail["can_manage"], false);
    assert_eq!(package_token_detail["can_create_packages"], true);
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_org_security_findings_respect_visibility_and_aggregate_severities(pool: PgPool) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let owner_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let member_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &owner_jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id should be returned");

    let (status, body) = add_org_member(&app, &owner_jwt, "acme-corp", "bob", "viewer").await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected member response: {body}"
    );

    let (status, body) = create_repository_with_options(
        &app,
        &owner_jwt,
        "Acme Public",
        "acme-public",
        Some(org_id),
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected public repository response: {body}"
    );

    let (status, body) = create_repository_with_options(
        &app,
        &owner_jwt,
        "Acme Internal",
        "acme-internal",
        Some(org_id),
        Some("private"),
        Some("internal_org"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected internal repository response: {body}"
    );

    let (status, body) = create_package_with_options(
        &app,
        &owner_jwt,
        "npm",
        "acme-public-widget",
        "acme-public",
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected public package response: {body}"
    );

    let (status, body) = create_package_with_options(
        &app,
        &owner_jwt,
        "npm",
        "acme-private-widget",
        "acme-public",
        Some("private"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected private package response: {body}"
    );

    let (status, body) = create_package_with_options(
        &app,
        &owner_jwt,
        "npm",
        "acme-internal-widget",
        "acme-internal",
        Some("internal_org"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected internal package response: {body}"
    );

    for package_name in [
        "acme-public-widget",
        "acme-private-widget",
        "acme-internal-widget",
    ] {
        let (status, body) =
            create_release_for_package(&app, &owner_jwt, "npm", package_name, "1.0.0").await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "unexpected release response for {package_name}: {body}"
        );
    }

    let public_release_id = get_release_id(&pool, "npm", "acme-public-widget", "1.0.0").await;
    let private_release_id = get_release_id(&pool, "npm", "acme-private-widget", "1.0.0").await;
    let internal_release_id = get_release_id(&pool, "npm", "acme-internal-widget", "1.0.0").await;

    insert_security_finding(
        &pool,
        public_release_id,
        "vulnerability",
        "critical",
        "Critical public issue",
        false,
    )
    .await;
    insert_security_finding(
        &pool,
        public_release_id,
        "policy_violation",
        "low",
        "Low public issue",
        false,
    )
    .await;
    insert_security_finding(
        &pool,
        private_release_id,
        "secrets_exposed",
        "high",
        "Private secret exposure",
        false,
    )
    .await;
    insert_security_finding(
        &pool,
        private_release_id,
        "policy_violation",
        "medium",
        "Resolved private policy issue",
        true,
    )
    .await;
    insert_security_finding(
        &pool,
        internal_release_id,
        "archive_bomb",
        "medium",
        "Internal archive bomb",
        false,
    )
    .await;

    let (status, anonymous_body) = list_org_security_findings(&app, None, "acme-corp").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected anonymous org security response: {anonymous_body}"
    );
    assert_eq!(anonymous_body["summary"]["open_findings"], 2);
    assert_eq!(anonymous_body["summary"]["affected_packages"], 1);
    assert_eq!(anonymous_body["summary"]["severities"]["critical"], 1);
    assert_eq!(anonymous_body["summary"]["severities"]["low"], 1);
    assert_eq!(anonymous_body["summary"]["severities"]["high"], 0);
    let anonymous_packages = anonymous_body["packages"]
        .as_array()
        .expect("anonymous packages response should be an array");
    assert_eq!(anonymous_packages.len(), 1, "response: {anonymous_body}");
    assert_eq!(anonymous_packages[0]["name"], "acme-public-widget");
    assert_eq!(anonymous_packages[0]["worst_severity"], "critical");
    assert_eq!(anonymous_packages[0]["open_findings"], 2);

    let (status, member_body) =
        list_org_security_findings(&app, Some(&member_jwt), "acme-corp").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected member org security response: {member_body}"
    );
    assert_eq!(member_body["summary"]["open_findings"], 4);
    assert_eq!(member_body["summary"]["affected_packages"], 3);
    assert_eq!(member_body["summary"]["severities"]["critical"], 1);
    assert_eq!(member_body["summary"]["severities"]["high"], 1);
    assert_eq!(member_body["summary"]["severities"]["medium"], 1);
    assert_eq!(member_body["summary"]["severities"]["low"], 1);
    let member_packages = member_body["packages"]
        .as_array()
        .expect("member packages response should be an array");
    assert_eq!(member_packages.len(), 3, "response: {member_body}");
    assert_eq!(member_packages[0]["name"], "acme-public-widget");
    assert_eq!(member_packages[0]["worst_severity"], "critical");
    assert_eq!(member_packages[1]["name"], "acme-private-widget");
    assert_eq!(member_packages[1]["worst_severity"], "high");
    assert_eq!(member_packages[2]["name"], "acme-internal-widget");
    assert_eq!(member_packages[2]["worst_severity"], "medium");
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_org_security_findings_return_empty_summary_without_open_findings(pool: PgPool) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let owner_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &owner_jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id should be returned");

    let (status, body) = create_repository_with_options(
        &app,
        &owner_jwt,
        "Acme Public",
        "acme-public",
        Some(org_id),
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {body}"
    );

    let (status, body) = create_package_with_options(
        &app,
        &owner_jwt,
        "npm",
        "acme-public-widget",
        "acme-public",
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {body}"
    );

    let (status, body) =
        create_release_for_package(&app, &owner_jwt, "npm", "acme-public-widget", "1.0.0").await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected release response: {body}"
    );

    let release_id = get_release_id(&pool, "npm", "acme-public-widget", "1.0.0").await;
    insert_security_finding(
        &pool,
        release_id,
        "policy_violation",
        "low",
        "Resolved policy issue",
        true,
    )
    .await;

    let (status, body) = list_org_security_findings(&app, None, "acme-corp").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["summary"]["open_findings"], 0);
    assert_eq!(body["summary"]["affected_packages"], 0);
    assert_eq!(body["summary"]["severities"]["critical"], 0);
    assert_eq!(body["summary"]["severities"]["low"], 0);
    assert_eq!(body["packages"], json!([]));
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_org_security_findings_support_filters_validation_and_csv_export(pool: PgPool) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let owner_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let member_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &owner_jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id should be returned");

    let (status, body) = add_org_member(&app, &owner_jwt, "acme-corp", "bob", "viewer").await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected member response: {body}"
    );

    let (status, body) = create_repository_with_options(
        &app,
        &owner_jwt,
        "Acme Public",
        "acme-public",
        Some(org_id),
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected public repository response: {body}"
    );

    let (status, body) = create_repository_with_options(
        &app,
        &owner_jwt,
        "Acme Internal",
        "acme-internal",
        Some(org_id),
        Some("private"),
        Some("internal_org"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected internal repository response: {body}"
    );

    let (status, body) = create_package_with_options(
        &app,
        &owner_jwt,
        "npm",
        "acme-public-widget",
        "acme-public",
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected public package response: {body}"
    );

    let (status, body) = create_package_with_options(
        &app,
        &owner_jwt,
        "npm",
        "acme-private-widget",
        "acme-public",
        Some("private"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected private package response: {body}"
    );

    let (status, body) = create_package_with_options(
        &app,
        &owner_jwt,
        "npm",
        "acme-internal-widget",
        "acme-internal",
        Some("internal_org"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected internal package response: {body}"
    );

    for package_name in [
        "acme-public-widget",
        "acme-private-widget",
        "acme-internal-widget",
    ] {
        let (status, body) =
            create_release_for_package(&app, &owner_jwt, "npm", package_name, "1.0.0").await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "unexpected release response for {package_name}: {body}"
        );
    }

    let public_release_id = get_release_id(&pool, "npm", "acme-public-widget", "1.0.0").await;
    let private_release_id = get_release_id(&pool, "npm", "acme-private-widget", "1.0.0").await;
    let internal_release_id = get_release_id(&pool, "npm", "acme-internal-widget", "1.0.0").await;

    insert_security_finding(
        &pool,
        public_release_id,
        "vulnerability",
        "critical",
        "Critical public issue",
        false,
    )
    .await;
    insert_security_finding(
        &pool,
        public_release_id,
        "policy_violation",
        "low",
        "Low public issue",
        false,
    )
    .await;
    insert_security_finding(
        &pool,
        private_release_id,
        "secrets_exposed",
        "high",
        "Private secret exposure",
        false,
    )
    .await;
    insert_security_finding(
        &pool,
        internal_release_id,
        "archive_bomb",
        "medium",
        "Internal archive bomb",
        false,
    )
    .await;

    let (status, critical_body) = list_org_security_findings_with_query(
        &app,
        Some(&member_jwt),
        "acme-corp",
        Some("severity=critical"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected critical-filtered response: {critical_body}"
    );
    assert_eq!(critical_body["summary"]["open_findings"], 1);
    assert_eq!(critical_body["summary"]["affected_packages"], 1);
    assert_eq!(critical_body["summary"]["severities"]["critical"], 1);
    assert_eq!(critical_body["summary"]["severities"]["low"], 0);
    let critical_packages = critical_body["packages"]
        .as_array()
        .expect("critical packages response should be an array");
    assert_eq!(critical_packages.len(), 1, "response: {critical_body}");
    assert_eq!(critical_packages[0]["name"], "acme-public-widget");
    assert_eq!(critical_packages[0]["open_findings"], 1);
    assert_eq!(critical_packages[0]["worst_severity"], "critical");

    let (status, package_body) = list_org_security_findings_with_query(
        &app,
        Some(&member_jwt),
        "acme-corp",
        Some("ecosystem=npm&package=internal"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected package-filtered response: {package_body}"
    );
    assert_eq!(package_body["summary"]["open_findings"], 1);
    assert_eq!(package_body["summary"]["severities"]["medium"], 1);
    let package_rows = package_body["packages"]
        .as_array()
        .expect("package-filtered response should be an array");
    assert_eq!(package_rows.len(), 1, "response: {package_body}");
    assert_eq!(package_rows[0]["name"], "acme-internal-widget");

    let (status, invalid_body) = list_org_security_findings_with_query(
        &app,
        Some(&member_jwt),
        "acme-corp",
        Some("severity=catastrophic"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "unexpected invalid-filter response: {invalid_body}"
    );
    assert!(invalid_body["error"]
        .as_str()
        .expect("error should be present")
        .contains("Unknown security severity filter"));

    let member_export_response = export_org_security_findings_csv(
        &app,
        Some(&member_jwt),
        "acme-corp",
        Some("ecosystem=npm&package=private"),
    )
    .await;
    assert_eq!(member_export_response.status(), StatusCode::OK);
    assert_eq!(
        member_export_response.headers()[header::CONTENT_TYPE],
        "text/csv; charset=utf-8"
    );
    assert!(
        member_export_response.headers()[header::CONTENT_DISPOSITION]
            .to_str()
            .expect("content disposition should be valid text")
            .contains("org-security-findings-acme-corp.csv")
    );

    let member_export_body = body_text(member_export_response).await;
    let member_export_lines = member_export_body.lines().collect::<Vec<_>>();
    assert_eq!(
        member_export_lines.len(),
        2,
        "unexpected CSV body: {member_export_body}"
    );
    assert!(member_export_lines[0].starts_with("package_id,ecosystem,name,"));
    assert!(member_export_body.contains("acme-private-widget"));
    assert!(member_export_body.contains(",1,high,"));
    assert!(!member_export_body.contains("acme-public-widget"));
    assert!(!member_export_body.contains("acme-internal-widget"));

    let anonymous_export_response =
        export_org_security_findings_csv(&app, None, "acme-corp", Some("severity=critical")).await;
    assert_eq!(anonymous_export_response.status(), StatusCode::OK);

    let anonymous_export_body = body_text(anonymous_export_response).await;
    let anonymous_export_lines = anonymous_export_body.lines().collect::<Vec<_>>();
    assert_eq!(
        anonymous_export_lines.len(),
        2,
        "unexpected anonymous CSV body: {anonymous_export_body}"
    );
    assert!(anonymous_export_body.contains("acme-public-widget"));
    assert!(!anonymous_export_body.contains("acme-private-widget"));
    assert!(!anonymous_export_body.contains("acme-internal-widget"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_org_security_findings_surface_review_teams_and_actor_triage_access(pool: PgPool) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    register_user(&app, "carol", "carol@test.dev", "super_secret_pw!").await;
    register_user(&app, "dana", "dana@test.dev", "super_secret_pw!").await;

    let owner_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let package_reviewer_jwt = login_user(&app, "bob", "super_secret_pw!").await;
    let plain_member_jwt = login_user(&app, "dana", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &owner_jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id should be returned");

    for username in ["bob", "carol", "dana"] {
        let (status, body) =
            add_org_member(&app, &owner_jwt, "acme-corp", username, "viewer").await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "unexpected org member response for {username}: {body}"
        );
    }

    let (status, body) = create_repository_with_options(
        &app,
        &owner_jwt,
        "Acme Public",
        "acme-public",
        Some(org_id),
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {body}"
    );

    let (status, body) = create_package_with_options(
        &app,
        &owner_jwt,
        "npm",
        "acme-widget",
        "acme-public",
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {body}"
    );

    let (status, body) =
        create_release_for_package(&app, &owner_jwt, "npm", "acme-widget", "1.0.0").await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected release response: {body}"
    );

    let release_id = get_release_id(&pool, "npm", "acme-widget", "1.0.0").await;
    insert_security_finding(
        &pool,
        release_id,
        "vulnerability",
        "critical",
        "Critical package issue",
        false,
    )
    .await;

    let (status, body) = create_team(
        &app,
        &owner_jwt,
        "acme-corp",
        "Security Reviewers",
        "security-reviewers",
        Some("Handles package-level security review"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package reviewer team response: {body}"
    );

    let (status, body) = create_team(
        &app,
        &owner_jwt,
        "acme-corp",
        "Repository Security",
        "repository-security",
        Some("Handles repository-wide security review"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository reviewer team response: {body}"
    );

    let (status, body) =
        add_team_member_to_team(&app, &owner_jwt, "acme-corp", "security-reviewers", "bob").await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected security team member response: {body}"
    );

    let (status, body) = add_team_member_to_team(
        &app,
        &owner_jwt,
        "acme-corp",
        "repository-security",
        "carol",
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository team member response: {body}"
    );

    let (status, body) = grant_team_package_access(
        &app,
        &owner_jwt,
        "acme-corp",
        "security-reviewers",
        "npm",
        "acme-widget",
        &["security_review"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected package access response: {body}"
    );

    let (status, body) = grant_team_repository_access(
        &app,
        &owner_jwt,
        "acme-corp",
        "repository-security",
        "acme-public",
        &["security_review"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected repository access response: {body}"
    );

    let (status, anonymous_body) = list_org_security_findings(&app, None, "acme-corp").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected anonymous org security response: {anonymous_body}"
    );
    let anonymous_packages = anonymous_body["packages"]
        .as_array()
        .expect("anonymous packages response should be an array");
    assert_eq!(anonymous_packages.len(), 1, "response: {anonymous_body}");
    assert_eq!(anonymous_packages[0]["reviewer_teams"], json!([]));
    assert_eq!(anonymous_packages[0]["can_manage_security"], false);

    let (status, plain_member_body) =
        list_org_security_findings(&app, Some(&plain_member_jwt), "acme-corp").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected plain-member org security response: {plain_member_body}"
    );
    let plain_member_packages = plain_member_body["packages"]
        .as_array()
        .expect("plain-member packages response should be an array");
    assert_eq!(
        plain_member_packages.len(),
        1,
        "response: {plain_member_body}"
    );
    assert_eq!(plain_member_packages[0]["can_manage_security"], false);
    assert_eq!(
        plain_member_packages[0]["reviewer_teams"],
        json!([
            {
                "id": plain_member_packages[0]["reviewer_teams"][0]["id"],
                "slug": "repository-security",
                "name": "Repository Security",
            },
            {
                "id": plain_member_packages[0]["reviewer_teams"][1]["id"],
                "slug": "security-reviewers",
                "name": "Security Reviewers",
            },
        ])
    );

    let (status, reviewer_body) =
        list_org_security_findings(&app, Some(&package_reviewer_jwt), "acme-corp").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected reviewer org security response: {reviewer_body}"
    );
    let reviewer_packages = reviewer_body["packages"]
        .as_array()
        .expect("reviewer packages response should be an array");
    assert_eq!(reviewer_packages.len(), 1, "response: {reviewer_body}");
    assert_eq!(reviewer_packages[0]["can_manage_security"], true);
    assert_eq!(
        reviewer_packages[0]["reviewer_teams"],
        plain_member_packages[0]["reviewer_teams"]
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
async fn test_org_audit_requires_audit_capable_membership(pool: PgPool) {
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
        .contains("owner, admin, or auditor"));

    let (status, _) = add_org_member(&app, &owner_jwt, "acme-corp", "bob", "auditor").await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = list_org_audit(&app, &bob_jwt, "acme-corp", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["page"], 1);
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
    assert!(logs
        .iter()
        .all(|log| log["metadata"]["username"] != "charlie"));
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
async fn test_org_audit_supports_actor_filtering(pool: PgPool) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    register_user(&app, "charlie", "charlie@test.dev", "super_secret_pw!").await;
    register_user(&app, "dana", "dana@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, _) = create_org(&app, &alice_jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = add_org_member(&app, &alice_jwt, "acme-corp", "bob", "admin").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = add_org_member(&app, &alice_jwt, "acme-corp", "charlie", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = add_org_member(&app, &bob_jwt, "acme-corp", "dana", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = add_org_member(&app, &bob_jwt, "acme-corp", "dana", "maintainer").await;
    assert_eq!(status, StatusCode::OK);

    let bob_user_id: String = sqlx::query_scalar("SELECT id::text FROM users WHERE username = $1")
        .bind("bob")
        .fetch_one(&pool)
        .await
        .expect("bob user id should be queryable");

    let first_page_query = format!("actor_user_id={bob_user_id}&per_page=1&page=1");
    let (status, first_page_body) =
        list_org_audit(&app, &alice_jwt, "acme-corp", Some(&first_page_query)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first_page_body["page"], 1);
    assert_eq!(first_page_body["per_page"], 1);
    assert_eq!(first_page_body["has_next"], true);

    let first_page_logs = first_page_body["logs"]
        .as_array()
        .expect("first page logs response should be an array");
    assert_eq!(first_page_logs.len(), 1);
    assert_eq!(first_page_logs[0]["actor_username"], "bob");
    assert_eq!(first_page_logs[0]["actor_user_id"], bob_user_id);

    let second_page_query = format!("actor_user_id={bob_user_id}&per_page=1&page=2");
    let (status, second_page_body) =
        list_org_audit(&app, &alice_jwt, "acme-corp", Some(&second_page_query)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(second_page_body["page"], 2);
    assert_eq!(second_page_body["per_page"], 1);
    assert_eq!(second_page_body["has_next"], false);

    let second_page_logs = second_page_body["logs"]
        .as_array()
        .expect("second page logs response should be an array");
    assert_eq!(second_page_logs.len(), 1);
    assert_eq!(second_page_logs[0]["actor_username"], "bob");
    assert_eq!(second_page_logs[0]["actor_user_id"], bob_user_id);

    let mut actions = first_page_logs
        .iter()
        .chain(second_page_logs.iter())
        .map(|log| {
            assert_eq!(log["actor_username"], "bob");
            assert_eq!(log["actor_user_id"], bob_user_id);
            assert_eq!(log["target_username"], "dana");

            log["action"]
                .as_str()
                .expect("filtered audit action should be present")
                .to_owned()
        })
        .collect::<Vec<_>>();
    actions.sort();

    assert_eq!(
        actions,
        vec!["org_member_add".to_owned(), "org_role_change".to_owned()]
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_org_audit_supports_date_range_filtering(pool: PgPool) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &alice_jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);

    let org_id = uuid::Uuid::parse_str(org_body["id"].as_str().expect("org id should be returned"))
        .expect("org id should parse");
    let alice_user_id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE username = $1")
        .bind("alice")
        .fetch_one(&pool)
        .await
        .expect("alice user id should be queryable");

    insert_org_audit_log(
        &pool,
        org_id,
        alice_user_id,
        "team_create",
        json!({ "team_slug": "early-team", "team_name": "Early team" }),
        "2024-01-10T10:00:00Z",
    )
    .await;
    insert_org_audit_log(
        &pool,
        org_id,
        alice_user_id,
        "org_update",
        json!({ "org_slug": "acme-corp", "org_name": "Acme Corp" }),
        "2024-01-15T12:30:00Z",
    )
    .await;
    insert_org_audit_log(
        &pool,
        org_id,
        alice_user_id,
        "team_delete",
        json!({ "team_slug": "late-team", "team_name": "Late team" }),
        "2024-01-20T18:45:00Z",
    )
    .await;

    let (status, body) = list_org_audit(
        &app,
        &alice_jwt,
        "acme-corp",
        Some("occurred_from=2024-01-12&occurred_until=2024-01-18&per_page=20"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected date-filtered audit response: {body}"
    );

    let logs = body["logs"]
        .as_array()
        .expect("logs response should be an array");
    assert_eq!(
        logs.len(),
        1,
        "unexpected date-filtered audit response: {body}"
    );
    assert_eq!(logs[0]["action"], "org_update");
    assert_eq!(logs[0]["occurred_at"], "2024-01-15T12:30:00Z");
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_org_audit_rejects_inverted_date_ranges(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, _) = create_org(&app, &alice_jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = list_org_audit(
        &app,
        &alice_jwt,
        "acme-corp",
        Some("occurred_from=2024-01-31&occurred_until=2024-01-01"),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "unexpected inverted-date audit response: {body}"
    );
    assert!(body["error"]
        .as_str()
        .expect("error should be present")
        .contains("occurred_from"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_org_audit_csv_export_respects_filters_and_returns_attachment_headers(pool: PgPool) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &alice_jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);

    let org_id = uuid::Uuid::parse_str(org_body["id"].as_str().expect("org id should be returned"))
        .expect("org id should parse");
    let alice_user_id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE username = $1")
        .bind("alice")
        .fetch_one(&pool)
        .await
        .expect("alice user id should be queryable");

    insert_org_audit_log(
        &pool,
        org_id,
        alice_user_id,
        "team_create",
        json!({ "team_slug": "early-team", "team_name": "Early team" }),
        "2024-01-10T10:00:00Z",
    )
    .await;
    insert_org_audit_log(
        &pool,
        org_id,
        alice_user_id,
        "org_update",
        json!({ "org_slug": "acme-corp", "org_name": "Acme Corp" }),
        "2024-01-15T12:30:00Z",
    )
    .await;
    insert_org_audit_log(
        &pool,
        org_id,
        alice_user_id,
        "team_delete",
        json!({ "team_slug": "late-team", "team_name": "Late team" }),
        "2024-01-20T18:45:00Z",
    )
    .await;

    let resp = export_org_audit_csv(
        &app,
        &alice_jwt,
        "acme-corp",
        Some("action=org_update&occurred_from=2024-01-12&occurred_until=2024-01-18"),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get(header::CONTENT_TYPE)
            .expect("content type should be present")
            .to_str()
            .expect("content type should be utf-8"),
        "text/csv; charset=utf-8"
    );
    assert_eq!(
        resp.headers()
            .get(header::CONTENT_DISPOSITION)
            .expect("content disposition should be present")
            .to_str()
            .expect("content disposition should be utf-8"),
        "attachment; filename=\"org-audit-acme-corp.csv\""
    );

    let body = body_text(resp).await;
    let lines = body.lines().collect::<Vec<_>>();

    assert_eq!(lines.len(), 2, "unexpected CSV export body: {body}");
    assert_eq!(
        lines[0],
        "id,occurred_at,action,actor_user_id,actor_username,actor_display_name,actor_token_id,target_user_id,target_username,target_display_name,target_org_id,target_package_id,target_release_id,metadata_json"
    );
    assert!(lines[1].contains(",2024-01-15T12:30:00+00:00,org_update,"));
    assert!(lines[1].contains("alice"));
    assert!(!body.contains("team_create"));
    assert!(!body.contains("team_delete"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_org_audit_csv_export_rejects_inverted_date_ranges(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, _) = create_org(&app, &alice_jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);

    let resp = export_org_audit_csv(
        &app,
        &alice_jwt,
        "acme-corp",
        Some("occurred_from=2024-01-31&occurred_until=2024-01-01"),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let body = body_json(resp).await;
    assert!(body["error"]
        .as_str()
        .expect("error should be present")
        .contains("occurred_from"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_org_audit_csv_export_requires_audit_capable_membership(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, _) = create_org(&app, &alice_jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = add_org_member(&app, &alice_jwt, "acme-corp", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let resp = export_org_audit_csv(&app, &bob_jwt, "acme-corp", None).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    let body = body_json(resp).await;
    assert!(body["error"]
        .as_str()
        .expect("error should be present")
        .contains("owner, admin, or auditor"));

    let (status, _) = add_org_member(&app, &alice_jwt, "acme-corp", "bob", "auditor").await;
    assert_eq!(status, StatusCode::OK);

    let resp = export_org_audit_csv(&app, &bob_jwt, "acme-corp", None).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_org_audit_reports_pagination_metadata(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, _) = create_org(&app, &jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);

    for index in 1..=6 {
        let username = format!("auditmember{:02}", index);
        let email = format!("auditmember{:02}@test.dev", index);

        let (status, _) = register_user(&app, &username, &email, "super_secret_pw!").await;
        assert_eq!(status, StatusCode::OK);

        let (status, _) = add_org_member(&app, &jwt, "acme-corp", &username, "viewer").await;
        assert_eq!(status, StatusCode::CREATED);
    }

    let (status, first_page_body) = list_org_audit(
        &app,
        &jwt,
        "acme-corp",
        Some("action=org_member_add&per_page=5&page=1"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first_page_body["page"], 1);
    assert_eq!(first_page_body["per_page"], 5);
    assert_eq!(first_page_body["has_next"], true);

    let first_page_logs = first_page_body["logs"]
        .as_array()
        .expect("first page logs response should be an array");
    assert_eq!(first_page_logs.len(), 5);
    assert!(first_page_logs
        .iter()
        .all(|log| log["action"] == "org_member_add"));

    let (status, second_page_body) = list_org_audit(
        &app,
        &jwt,
        "acme-corp",
        Some("action=org_member_add&per_page=5&page=2"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(second_page_body["page"], 2);
    assert_eq!(second_page_body["per_page"], 5);
    assert_eq!(second_page_body["has_next"], false);

    let second_page_logs = second_page_body["logs"]
        .as_array()
        .expect("second page logs response should be an array");
    assert_eq!(second_page_logs.len(), 1);
    assert!(second_page_logs
        .iter()
        .all(|log| log["action"] == "org_member_add"));

    let mut usernames = first_page_logs
        .iter()
        .chain(second_page_logs.iter())
        .map(|log| {
            log["metadata"]["username"]
                .as_str()
                .expect("audit username metadata should be present")
                .to_owned()
        })
        .collect::<Vec<_>>();
    usernames.sort();

    assert_eq!(
        usernames,
        vec![
            "auditmember01".to_owned(),
            "auditmember02".to_owned(),
            "auditmember03".to_owned(),
            "auditmember04".to_owned(),
            "auditmember05".to_owned(),
            "auditmember06".to_owned(),
        ]
    );
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
    let (status, _) =
        register_user(&app, "owner_user", "owner@example.com", "Str0ngP@ssword!").await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) =
        register_user(&app, "target_user", "target@example.com", "Str0ngP@ssword!").await;
    assert_eq!(status, StatusCode::OK);

    let owner_jwt = login_user(&app, "owner_user", "Str0ngP@ssword!").await;

    let (status, _) = create_org(&app, &owner_jwt, "Transfer Org", "transfer-org").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = add_org_member(
        &app,
        &owner_jwt,
        "transfer-org",
        "target_user",
        "maintainer",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = transfer_ownership(&app, &owner_jwt, "transfer-org", "target_user").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected ownership transfer response: {body}"
    );

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

#[sqlx::test(migrations = "../../migrations")]
async fn test_org_audit_includes_org_updates(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id should be returned");

    let (status, body) = update_org_profile(
        &app,
        &jwt,
        "acme-corp",
        json!({
            "description": "Central package platform for Acme.",
            "website": "https://packages.acme.test",
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected org update response: {body}"
    );

    let (status, body) = list_org_audit(
        &app,
        &jwt,
        "acme-corp",
        Some("action=org_update&per_page=10"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let logs = body["logs"]
        .as_array()
        .expect("logs response should be an array");
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0]["action"], "org_update");
    assert_eq!(logs[0]["actor_username"], "alice");
    assert_eq!(logs[0]["target_org_id"], org_id);
    assert_eq!(logs[0]["metadata"]["org_slug"], "acme-corp");
    assert_eq!(logs[0]["metadata"]["org_name"], "Acme Corp");
    assert_eq!(
        logs[0]["metadata"]["changed_fields"],
        json!(["description", "website"])
    );
    assert_eq!(
        logs[0]["metadata"]["changes"]["description"]["before"],
        Value::Null
    );
    assert_eq!(
        logs[0]["metadata"]["changes"]["description"]["after"],
        "Central package platform for Acme."
    );
    assert_eq!(
        logs[0]["metadata"]["changes"]["website"]["before"],
        Value::Null
    );
    assert_eq!(
        logs[0]["metadata"]["changes"]["website"]["after"],
        "https://packages.acme.test"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_org_audit_includes_team_governance_events(pool: PgPool) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let owner_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &owner_jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id should be returned");

    let (status, _) = add_org_member(&app, &owner_jwt, "acme-corp", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, create_body) = create_team(
        &app,
        &owner_jwt,
        "acme-corp",
        "Release Engineering",
        "release-engineering",
        Some("Owns package publication workflows"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected team create response: {create_body}"
    );

    let (status, update_body) = update_team_for_org(
        &app,
        &owner_jwt,
        "acme-corp",
        "release-engineering",
        json!({
            "name": "Release Operations",
            "description": "Coordinates releases and publication",
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected team update response: {update_body}"
    );

    let (status, add_member_body) =
        add_team_member_to_team(&app, &owner_jwt, "acme-corp", "release-engineering", "bob").await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected team member add response: {add_member_body}"
    );

    let (status, remove_member_body) =
        remove_team_member_from_team(&app, &owner_jwt, "acme-corp", "release-engineering", "bob")
            .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected team member removal response: {remove_member_body}"
    );

    let (status, delete_body) =
        delete_team_for_org(&app, &owner_jwt, "acme-corp", "release-engineering").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected team delete response: {delete_body}"
    );

    let (status, team_create_audit) = list_org_audit(
        &app,
        &owner_jwt,
        "acme-corp",
        Some("action=team_create&per_page=10"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let create_logs = team_create_audit["logs"]
        .as_array()
        .expect("team_create audit logs should be an array");
    assert_eq!(create_logs.len(), 1, "response: {team_create_audit}");
    assert_eq!(create_logs[0]["target_org_id"].as_str(), Some(org_id));
    assert_eq!(
        create_logs[0]["metadata"]["team_slug"],
        "release-engineering"
    );
    assert_eq!(
        create_logs[0]["metadata"]["team_name"],
        "Release Engineering"
    );
    assert_eq!(
        create_logs[0]["metadata"]["description"],
        "Owns package publication workflows"
    );

    let (status, team_update_audit) = list_org_audit(
        &app,
        &owner_jwt,
        "acme-corp",
        Some("action=team_update&per_page=10"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let update_logs = team_update_audit["logs"]
        .as_array()
        .expect("team_update audit logs should be an array");
    assert_eq!(update_logs.len(), 1, "response: {team_update_audit}");
    assert_eq!(update_logs[0]["target_org_id"].as_str(), Some(org_id));
    assert_eq!(
        update_logs[0]["metadata"]["team_slug"],
        "release-engineering"
    );
    assert_eq!(
        update_logs[0]["metadata"]["previous_name"],
        "Release Engineering"
    );
    assert_eq!(update_logs[0]["metadata"]["name"], "Release Operations");
    assert_eq!(
        update_logs[0]["metadata"]["description"],
        "Coordinates releases and publication"
    );

    let (status, team_member_add_audit) = list_org_audit(
        &app,
        &owner_jwt,
        "acme-corp",
        Some("action=team_member_add&per_page=10"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let add_logs = team_member_add_audit["logs"]
        .as_array()
        .expect("team_member_add audit logs should be an array");
    assert_eq!(add_logs.len(), 1, "response: {team_member_add_audit}");
    assert_eq!(add_logs[0]["target_org_id"].as_str(), Some(org_id));
    assert_eq!(add_logs[0]["target_username"], "bob");
    assert_eq!(add_logs[0]["metadata"]["username"], "bob");
    assert_eq!(add_logs[0]["metadata"]["team_name"], "Release Operations");

    let (status, team_member_remove_audit) = list_org_audit(
        &app,
        &owner_jwt,
        "acme-corp",
        Some("action=team_member_remove&per_page=10"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let remove_logs = team_member_remove_audit["logs"]
        .as_array()
        .expect("team_member_remove audit logs should be an array");
    assert_eq!(remove_logs.len(), 1, "response: {team_member_remove_audit}");
    assert_eq!(remove_logs[0]["target_org_id"].as_str(), Some(org_id));
    assert_eq!(remove_logs[0]["target_username"], "bob");
    assert_eq!(remove_logs[0]["metadata"]["username"], "bob");
    assert_eq!(
        remove_logs[0]["metadata"]["team_name"],
        "Release Operations"
    );

    let (status, team_delete_audit) = list_org_audit(
        &app,
        &owner_jwt,
        "acme-corp",
        Some("action=team_delete&per_page=10"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let delete_logs = team_delete_audit["logs"]
        .as_array()
        .expect("team_delete audit logs should be an array");
    assert_eq!(delete_logs.len(), 1, "response: {team_delete_audit}");
    assert_eq!(delete_logs[0]["target_org_id"].as_str(), Some(org_id));
    assert_eq!(
        delete_logs[0]["metadata"]["team_name"],
        "Release Operations"
    );
    assert_eq!(delete_logs[0]["metadata"]["removed_member_count"], 0);

    let export_resp = export_org_audit_csv(
        &app,
        &owner_jwt,
        "acme-corp",
        Some("action=team_member_add"),
    )
    .await;
    assert_eq!(export_resp.status(), StatusCode::OK);
    let export_body = body_text(export_resp).await;
    assert!(export_body.contains(",team_member_add,"));
    assert!(export_body.contains("bob"));
    assert!(!export_body.contains(",team_member_remove,"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_org_audit_includes_invitation_lifecycle_events(pool: PgPool) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    register_user(&app, "charlie", "charlie@test.dev", "super_secret_pw!").await;
    register_user(&app, "dana", "dana@test.dev", "super_secret_pw!").await;

    let owner_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;
    let charlie_jwt = login_user(&app, "charlie", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &owner_jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id should be returned");

    let (status, bob_invite_body) =
        send_org_invitation(&app, &owner_jwt, "acme-corp", "bob", "viewer", 7).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected bob invitation response: {bob_invite_body}"
    );
    let bob_invitation_id = bob_invite_body["id"]
        .as_str()
        .expect("bob invitation id should be returned")
        .to_owned();

    let (status, charlie_invite_body) =
        send_org_invitation(&app, &owner_jwt, "acme-corp", "charlie", "maintainer", 7).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected charlie invitation response: {charlie_invite_body}"
    );
    let charlie_invitation_id = charlie_invite_body["id"]
        .as_str()
        .expect("charlie invitation id should be returned")
        .to_owned();

    let (status, dana_invite_body) =
        send_org_invitation(&app, &owner_jwt, "acme-corp", "dana", "auditor", 7).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected dana invitation response: {dana_invite_body}"
    );
    let dana_invitation_id = dana_invite_body["id"]
        .as_str()
        .expect("dana invitation id should be returned")
        .to_owned();

    let (status, revoke_body) =
        revoke_org_invitation_for_org(&app, &owner_jwt, "acme-corp", &dana_invitation_id).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected revoke response: {revoke_body}"
    );
    assert_eq!(revoke_body["message"], "Invitation revoked");

    let (status, accept_body) =
        accept_org_invitation_for_current_user(&app, &bob_jwt, &bob_invitation_id).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected accept response: {accept_body}"
    );
    assert_eq!(accept_body["message"], "Invitation accepted");

    let (status, decline_body) =
        decline_org_invitation_for_current_user(&app, &charlie_jwt, &charlie_invitation_id).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected decline response: {decline_body}"
    );
    assert_eq!(decline_body["message"], "Invitation declined");

    let (status, invite_create_audit) = list_org_audit(
        &app,
        &owner_jwt,
        "acme-corp",
        Some("action=org_invitation_create&per_page=10"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let create_logs = invite_create_audit["logs"]
        .as_array()
        .expect("org_invitation_create audit logs should be an array");
    assert_eq!(create_logs.len(), 3, "response: {invite_create_audit}");
    assert!(create_logs
        .iter()
        .all(|log| log["target_org_id"].as_str() == Some(org_id)));
    let mut created_targets = create_logs
        .iter()
        .map(|log| {
            format!(
                "{}:{}",
                log["target_username"]
                    .as_str()
                    .expect("target username should be present"),
                log["metadata"]["role"]
                    .as_str()
                    .expect("role metadata should be present")
            )
        })
        .collect::<Vec<_>>();
    created_targets.sort();
    assert_eq!(
        created_targets,
        vec![
            "bob:viewer".to_owned(),
            "charlie:maintainer".to_owned(),
            "dana:auditor".to_owned(),
        ]
    );

    let (status, revoke_audit) = list_org_audit(
        &app,
        &owner_jwt,
        "acme-corp",
        Some("action=org_invitation_revoke&per_page=10"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let revoke_logs = revoke_audit["logs"]
        .as_array()
        .expect("org_invitation_revoke audit logs should be an array");
    assert_eq!(revoke_logs.len(), 1, "response: {revoke_audit}");
    assert_eq!(revoke_logs[0]["target_org_id"].as_str(), Some(org_id));
    assert_eq!(revoke_logs[0]["target_username"], "dana");
    assert_eq!(revoke_logs[0]["metadata"]["role"], "auditor");
    assert_eq!(
        revoke_logs[0]["metadata"]["invitation_id"],
        dana_invitation_id
    );

    let (status, accept_audit) = list_org_audit(
        &app,
        &owner_jwt,
        "acme-corp",
        Some("action=org_invitation_accept&per_page=10"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let accept_logs = accept_audit["logs"]
        .as_array()
        .expect("org_invitation_accept audit logs should be an array");
    assert_eq!(accept_logs.len(), 1, "response: {accept_audit}");
    assert_eq!(accept_logs[0]["target_org_id"].as_str(), Some(org_id));
    assert_eq!(accept_logs[0]["target_username"], "bob");
    assert_eq!(accept_logs[0]["metadata"]["role"], "viewer");
    assert_eq!(accept_logs[0]["metadata"]["org_name"], "Acme Corp");
    assert_eq!(
        accept_logs[0]["metadata"]["invitation_id"],
        bob_invitation_id
    );

    let (status, decline_audit) = list_org_audit(
        &app,
        &owner_jwt,
        "acme-corp",
        Some("action=org_invitation_decline&per_page=10"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let decline_logs = decline_audit["logs"]
        .as_array()
        .expect("org_invitation_decline audit logs should be an array");
    assert_eq!(decline_logs.len(), 1, "response: {decline_audit}");
    assert_eq!(decline_logs[0]["target_org_id"].as_str(), Some(org_id));
    assert_eq!(decline_logs[0]["target_username"], "charlie");
    assert_eq!(decline_logs[0]["metadata"]["role"], "maintainer");
    assert_eq!(
        decline_logs[0]["metadata"]["invitation_id"],
        charlie_invitation_id
    );

    let export_resp = export_org_audit_csv(
        &app,
        &owner_jwt,
        "acme-corp",
        Some("action=org_invitation_create"),
    )
    .await;
    assert_eq!(export_resp.status(), StatusCode::OK);
    let export_body = body_text(export_resp).await;
    assert!(export_body.contains(",org_invitation_create,"));
    assert!(export_body.contains("bob"));
    assert!(export_body.contains("charlie"));
    assert!(export_body.contains("dana"));
    assert!(!export_body.contains(",org_invitation_decline,"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_org_admin_can_list_active_and_inactive_invitations(pool: PgPool) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    register_user(&app, "charlie", "charlie@test.dev", "super_secret_pw!").await;
    register_user(&app, "dana", "dana@test.dev", "super_secret_pw!").await;
    register_user(&app, "erin", "erin@test.dev", "super_secret_pw!").await;

    let owner_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;
    let charlie_jwt = login_user(&app, "charlie", "super_secret_pw!").await;

    let (status, _) = create_org(&app, &owner_jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, bob_invite_body) =
        send_org_invitation(&app, &owner_jwt, "acme-corp", "bob", "viewer", 7).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected bob invitation response: {bob_invite_body}"
    );
    let bob_invitation_id = bob_invite_body["id"]
        .as_str()
        .expect("bob invitation id should be returned")
        .to_owned();

    let (status, charlie_invite_body) =
        send_org_invitation(&app, &owner_jwt, "acme-corp", "charlie", "maintainer", 7).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected charlie invitation response: {charlie_invite_body}"
    );
    let charlie_invitation_id = charlie_invite_body["id"]
        .as_str()
        .expect("charlie invitation id should be returned")
        .to_owned();

    let (status, dana_invite_body) =
        send_org_invitation(&app, &owner_jwt, "acme-corp", "dana", "auditor", 7).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected dana invitation response: {dana_invite_body}"
    );
    let dana_invitation_id = dana_invite_body["id"]
        .as_str()
        .expect("dana invitation id should be returned")
        .to_owned();

    let (status, erin_invite_body) =
        send_org_invitation(&app, &owner_jwt, "acme-corp", "erin", "billing_manager", 7).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected erin invitation response: {erin_invite_body}"
    );
    let erin_invitation_id = erin_invite_body["id"]
        .as_str()
        .expect("erin invitation id should be returned")
        .to_owned();

    let (status, accept_body) =
        accept_org_invitation_for_current_user(&app, &bob_jwt, &bob_invitation_id).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected accept response: {accept_body}"
    );

    let (status, decline_body) =
        decline_org_invitation_for_current_user(&app, &charlie_jwt, &charlie_invitation_id).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected decline response: {decline_body}"
    );

    let (status, revoke_body) =
        revoke_org_invitation_for_org(&app, &owner_jwt, "acme-corp", &dana_invitation_id).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected revoke response: {revoke_body}"
    );

    let (status, active_body) =
        list_org_invitations_for_org(&app, &owner_jwt, "acme-corp", false).await;
    assert_eq!(status, StatusCode::OK);
    let active_invitations = active_body["invitations"]
        .as_array()
        .expect("active invitation list should be an array");
    assert_eq!(active_invitations.len(), 1, "response: {active_body}");
    assert_eq!(active_invitations[0]["id"], erin_invitation_id);
    assert_eq!(active_invitations[0]["invited_user"]["username"], "erin");
    assert_eq!(active_invitations[0]["invited_by"]["username"], "alice");
    assert_eq!(active_invitations[0]["role"], "billing_manager");
    assert_eq!(active_invitations[0]["status"], "pending");
    assert_eq!(active_invitations[0]["accepted_at"], Value::Null);
    assert_eq!(active_invitations[0]["declined_at"], Value::Null);
    assert_eq!(active_invitations[0]["revoked_at"], Value::Null);

    let (status, all_body) =
        list_org_invitations_for_org(&app, &owner_jwt, "acme-corp", true).await;
    assert_eq!(status, StatusCode::OK);
    let all_invitations = all_body["invitations"]
        .as_array()
        .expect("full invitation list should be an array");
    assert_eq!(all_invitations.len(), 4, "response: {all_body}");

    let mut invitation_states = all_invitations
        .iter()
        .map(|invitation| {
            (
                invitation["invited_user"]["username"]
                    .as_str()
                    .expect("invited username should be present")
                    .to_owned(),
                invitation["status"]
                    .as_str()
                    .expect("status should be present")
                    .to_owned(),
            )
        })
        .collect::<Vec<_>>();
    invitation_states.sort();
    assert_eq!(
        invitation_states,
        vec![
            ("bob".to_owned(), "accepted".to_owned()),
            ("charlie".to_owned(), "declined".to_owned()),
            ("dana".to_owned(), "revoked".to_owned()),
            ("erin".to_owned(), "pending".to_owned()),
        ]
    );

    let accepted_invitation = all_invitations
        .iter()
        .find(|invitation| invitation["invited_user"]["username"] == "bob")
        .expect("accepted invitation should be present");
    assert_ne!(accepted_invitation["accepted_at"], Value::Null);
    assert_eq!(accepted_invitation["declined_at"], Value::Null);
    assert_eq!(accepted_invitation["revoked_at"], Value::Null);

    let declined_invitation = all_invitations
        .iter()
        .find(|invitation| invitation["invited_user"]["username"] == "charlie")
        .expect("declined invitation should be present");
    assert_eq!(declined_invitation["accepted_at"], Value::Null);
    assert_ne!(declined_invitation["declined_at"], Value::Null);
    assert_eq!(declined_invitation["revoked_at"], Value::Null);

    let revoked_invitation = all_invitations
        .iter()
        .find(|invitation| invitation["invited_user"]["username"] == "dana")
        .expect("revoked invitation should be present");
    assert_eq!(revoked_invitation["accepted_at"], Value::Null);
    assert_eq!(revoked_invitation["declined_at"], Value::Null);
    assert_ne!(revoked_invitation["revoked_at"], Value::Null);

    let pending_invitation = all_invitations
        .iter()
        .find(|invitation| invitation["invited_user"]["username"] == "erin")
        .expect("pending invitation should be present");
    assert_eq!(pending_invitation["id"], erin_invitation_id);
    assert_eq!(pending_invitation["accepted_at"], Value::Null);
    assert_eq!(pending_invitation["declined_at"], Value::Null);
    assert_eq!(pending_invitation["revoked_at"], Value::Null);
}

// ══════════════════════════════════════════════════════════════════════════════
// Namespace claims
// ══════════════════════════════════════════════════════════════════════════════

#[sqlx::test(migrations = "../../migrations")]
async fn test_org_admin_can_create_and_list_namespace_claims(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, acme_body) = create_org(&app, &jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let acme_org_id = acme_body["id"]
        .as_str()
        .expect("acme org id should be returned");

    let (status, beta_body) = create_org(&app, &jwt, "Beta Corp", "beta-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let beta_org_id = beta_body["id"]
        .as_str()
        .expect("beta org id should be returned");

    let (status, body) =
        create_namespace_claim(&app, &jwt, "npm", "@acme", Some(acme_org_id)).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected namespace create response: {body}"
    );
    assert_eq!(body["ecosystem"], "npm");
    assert_eq!(body["namespace"], "@acme");
    assert_eq!(body["owner_org_id"], acme_org_id);
    assert_eq!(body["is_verified"], false);

    let (status, body) =
        create_namespace_claim(&app, &jwt, "pypi", "acme-internal", Some(beta_org_id)).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected second namespace create response: {body}"
    );

    let query = format!("owner_org_id={acme_org_id}");
    let (status, body) = list_namespace_claims(&app, Some(&query)).await;
    assert_eq!(status, StatusCode::OK);

    let namespaces = body["namespaces"]
        .as_array()
        .expect("namespaces response should be an array");
    assert_eq!(namespaces.len(), 1, "filtered namespaces response: {body}");
    assert_eq!(namespaces[0]["ecosystem"], "npm");
    assert_eq!(namespaces[0]["namespace"], "@acme");
    assert_eq!(namespaces[0]["owner_org_id"], acme_org_id);

    let (status, body) = list_org_audit(
        &app,
        &jwt,
        "acme-corp",
        Some("action=namespace_claim_create&per_page=10"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let logs = body["logs"]
        .as_array()
        .expect("logs response should be an array");
    assert_eq!(logs.len(), 1, "unexpected org audit response: {body}");
    assert_eq!(logs[0]["action"], "namespace_claim_create");
    assert_eq!(logs[0]["target_org_id"], acme_org_id);
    assert_eq!(logs[0]["metadata"]["ecosystem"], "npm");
    assert_eq!(logs[0]["metadata"]["namespace"], "@acme");
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_namespace_claim_creation_requires_org_admin_membership(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, acme_body) = create_org(&app, &alice_jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let acme_org_id = acme_body["id"]
        .as_str()
        .expect("acme org id should be returned");

    let (status, body) =
        create_namespace_claim(&app, &bob_jwt, "npm", "@acme", Some(acme_org_id)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(
        body["error"].as_str().is_some(),
        "unexpected error response: {body}"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_duplicate_namespace_claims_are_rejected(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, acme_body) = create_org(&app, &jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let acme_org_id = acme_body["id"]
        .as_str()
        .expect("acme org id should be returned");

    let (status, beta_body) = create_org(&app, &jwt, "Beta Corp", "beta-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let beta_org_id = beta_body["id"]
        .as_str()
        .expect("beta org id should be returned");

    let (status, body) =
        create_namespace_claim(&app, &jwt, "npm", "@acme", Some(acme_org_id)).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected namespace create response: {body}"
    );

    let (status, body) =
        create_namespace_claim(&app, &jwt, "npm", "@acme", Some(beta_org_id)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(body["error"]
        .as_str()
        .expect("error should be present")
        .contains("Namespace claim already exists"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_org_admin_can_delete_namespace_claim_and_audit_it(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id should be returned");

    let (status, create_body) =
        create_namespace_claim(&app, &jwt, "npm", "@acme", Some(org_id)).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected namespace create response: {create_body}"
    );
    let claim_id = create_body["id"]
        .as_str()
        .expect("namespace claim id should be returned");

    let (status, delete_body) = delete_namespace_claim(&app, &jwt, claim_id).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected namespace delete response: {delete_body}"
    );
    assert_eq!(delete_body["message"], "Namespace claim deleted");

    let query = format!("owner_org_id={org_id}");
    let (status, list_body) = list_namespace_claims(&app, Some(&query)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        list_body["namespaces"]
            .as_array()
            .expect("namespaces response should be an array")
            .len(),
        0,
        "namespace claim should be removed: {list_body}"
    );

    let (status, audit_body) = list_org_audit(
        &app,
        &jwt,
        "acme-corp",
        Some("action=namespace_claim_delete&per_page=10"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let logs = audit_body["logs"]
        .as_array()
        .expect("logs response should be an array");
    assert_eq!(logs.len(), 1, "unexpected org audit response: {audit_body}");
    assert_eq!(logs[0]["action"], "namespace_claim_delete");
    assert_eq!(logs[0]["target_org_id"], org_id);
    assert_eq!(logs[0]["metadata"]["ecosystem"], "npm");
    assert_eq!(logs[0]["metadata"]["namespace"], "@acme");
    assert_eq!(logs[0]["metadata"]["namespace_claim_id"], claim_id);
    assert_eq!(logs[0]["metadata"]["was_verified"], false);
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_namespace_claim_delete_requires_owner_or_org_admin(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    register_user(&app, "carol", "carol@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;
    let carol_jwt = login_user(&app, "carol", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &alice_jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id should be returned");

    let (status, _) = add_org_member(&app, &alice_jwt, "acme-corp", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, org_claim_body) =
        create_namespace_claim(&app, &alice_jwt, "npm", "@acme", Some(org_id)).await;
    assert_eq!(status, StatusCode::CREATED);
    let org_claim_id = org_claim_body["id"]
        .as_str()
        .expect("org namespace claim id should be returned");

    let (status, forbidden_body) = delete_namespace_claim(&app, &bob_jwt, org_claim_id).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(
        forbidden_body["error"]
            .as_str()
            .expect("error should be present")
            .contains("manage this namespace claim"),
        "unexpected error response: {forbidden_body}"
    );

    let (status, user_claim_body) =
        create_namespace_claim(&app, &carol_jwt, "pypi", "carol", None).await;
    assert_eq!(status, StatusCode::CREATED);
    let user_claim_id = user_claim_body["id"]
        .as_str()
        .expect("user namespace claim id should be returned");

    let (status, forbidden_body) = delete_namespace_claim(&app, &alice_jwt, user_claim_id).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(
        forbidden_body["error"]
            .as_str()
            .expect("error should be present")
            .contains("your own account"),
        "unexpected error response: {forbidden_body}"
    );

    let (status, delete_body) = delete_namespace_claim(&app, &carol_jwt, user_claim_id).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected self-delete response: {delete_body}"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_user_owned_namespace_claim_can_transfer_to_controlled_org(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id should be returned");

    let (status, create_body) = create_namespace_claim(&app, &jwt, "npm", "@alice", None).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected namespace create response: {create_body}"
    );
    let claim_id = create_body["id"]
        .as_str()
        .expect("namespace claim id should be returned");

    let (status, transfer_body) = transfer_namespace_claim(&app, &jwt, claim_id, "acme-corp").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected namespace transfer response: {transfer_body}"
    );
    assert_eq!(
        transfer_body["message"],
        "Namespace claim ownership transferred"
    );
    assert_eq!(transfer_body["namespace_claim"]["namespace"], "@alice");
    assert_eq!(transfer_body["owner"]["slug"], "acme-corp");
    let owner_user_id = create_body["owner_user_id"]
        .as_str()
        .expect("user-owned namespace claim should expose the owner user id");

    let (status, user_list_body) =
        list_namespace_claims(&app, Some(&format!("owner_user_id={owner_user_id}"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        user_list_body["namespaces"]
            .as_array()
            .expect("namespaces response should be an array")
            .len(),
        0,
        "transferred namespace claim should no longer be user-owned: {user_list_body}"
    );

    let (status, org_list_body) =
        list_namespace_claims(&app, Some(&format!("owner_org_id={org_id}"))).await;
    assert_eq!(status, StatusCode::OK);
    let namespaces = org_list_body["namespaces"]
        .as_array()
        .expect("namespaces response should be an array");
    assert_eq!(
        namespaces.len(),
        1,
        "transferred claim should be org-owned: {org_list_body}"
    );
    assert_eq!(namespaces[0]["namespace"], "@alice");

    let (status, audit_body) = list_org_audit(
        &app,
        &jwt,
        "acme-corp",
        Some("action=namespace_claim_transfer&per_page=10"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let logs = audit_body["logs"]
        .as_array()
        .expect("logs response should be an array");
    assert_eq!(logs.len(), 1, "unexpected org audit response: {audit_body}");
    assert_eq!(logs[0]["action"], "namespace_claim_transfer");
    assert_eq!(logs[0]["target_org_id"], org_id);
    assert_eq!(logs[0]["metadata"]["namespace_claim_id"], claim_id);
    assert_eq!(logs[0]["metadata"]["previous_owner_type"], "user");
    assert_eq!(logs[0]["metadata"]["new_owner_org_slug"], "acme-corp");
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_org_admin_can_transfer_namespace_claim_between_controlled_orgs(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, source_body) = create_org(&app, &jwt, "Source Corp", "source-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let source_org_id = source_body["id"]
        .as_str()
        .expect("source org id should be returned");

    let (status, target_body) = create_org(&app, &jwt, "Target Corp", "target-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let target_org_id = target_body["id"]
        .as_str()
        .expect("target org id should be returned");

    let (status, create_body) =
        create_namespace_claim(&app, &jwt, "npm", "@source", Some(source_org_id)).await;
    assert_eq!(status, StatusCode::CREATED);
    let claim_id = create_body["id"]
        .as_str()
        .expect("namespace claim id should be returned");

    let (status, transfer_body) =
        transfer_namespace_claim(&app, &jwt, claim_id, "target-corp").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected namespace transfer response: {transfer_body}"
    );
    assert_eq!(transfer_body["owner"]["slug"], "target-corp");

    let (status, source_list_body) =
        list_namespace_claims(&app, Some(&format!("owner_org_id={source_org_id}"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        source_list_body["namespaces"]
            .as_array()
            .expect("namespaces response should be an array")
            .len(),
        0,
        "source organization should no longer own the claim: {source_list_body}"
    );

    let (status, target_list_body) =
        list_namespace_claims(&app, Some(&format!("owner_org_id={target_org_id}"))).await;
    assert_eq!(status, StatusCode::OK);
    let namespaces = target_list_body["namespaces"]
        .as_array()
        .expect("namespaces response should be an array");
    assert_eq!(
        namespaces.len(),
        1,
        "target organization should own the claim: {target_list_body}"
    );
    assert_eq!(namespaces[0]["namespace"], "@source");
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_namespace_claim_transfer_requires_source_and_target_org_control(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, source_body) = create_org(&app, &alice_jwt, "Source Corp", "source-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let source_org_id = source_body["id"]
        .as_str()
        .expect("source org id should be returned");

    let (status, _) = add_org_member(&app, &alice_jwt, "source-corp", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, create_body) =
        create_namespace_claim(&app, &alice_jwt, "npm", "@source", Some(source_org_id)).await;
    assert_eq!(status, StatusCode::CREATED);
    let claim_id = create_body["id"]
        .as_str()
        .expect("namespace claim id should be returned");

    let (status, target_body) = create_org(&app, &bob_jwt, "Target Corp", "target-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let target_org_id = target_body["id"]
        .as_str()
        .expect("target org id should be returned");

    let (status, forbidden_body) =
        transfer_namespace_claim(&app, &bob_jwt, claim_id, "target-corp").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(
        forbidden_body["error"]
            .as_str()
            .expect("error should be present")
            .contains("transfer this namespace claim"),
        "unexpected error response: {forbidden_body}"
    );

    let (status, other_claim_body) =
        create_namespace_claim(&app, &bob_jwt, "pypi", "bob", None).await;
    assert_eq!(status, StatusCode::CREATED);
    let other_claim_id = other_claim_body["id"]
        .as_str()
        .expect("user namespace claim id should be returned");

    let (status, forbidden_body) =
        transfer_namespace_claim(&app, &alice_jwt, other_claim_id, "source-corp").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(
        forbidden_body["error"]
            .as_str()
            .expect("error should be present")
            .contains("your own account"),
        "unexpected error response: {forbidden_body}"
    );

    let (status, same_target_body) =
        transfer_namespace_claim(&app, &bob_jwt, other_claim_id, "target-corp").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "first transfer should succeed: {same_target_body}"
    );

    let (status, duplicate_target_body) =
        transfer_namespace_claim(&app, &bob_jwt, other_claim_id, "target-corp").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        duplicate_target_body["error"]
            .as_str()
            .expect("error should be present")
            .contains("already owned by the target organization"),
        "unexpected duplicate target response: {duplicate_target_body}"
    );

    let (status, final_list_body) =
        list_namespace_claims(&app, Some(&format!("owner_org_id={target_org_id}"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        final_list_body["namespaces"]
            .as_array()
            .expect("namespaces response should be an array")
            .len(),
        1
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_namespace_claim_transfer_requires_explicit_scope(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, create_body) = create_namespace_claim(&app, &jwt, "npm", "@alice", None).await;
    assert_eq!(status, StatusCode::CREATED);
    let claim_id = create_body["id"]
        .as_str()
        .expect("namespace claim id should be returned");

    let (status, token_body) =
        create_personal_access_token(&app, &jwt, "namespace-write-only", &["namespaces:write"])
            .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected token response: {token_body}"
    );
    let token = token_body["token"]
        .as_str()
        .expect("token should be returned")
        .to_owned();

    let (status, forbidden_body) =
        transfer_namespace_claim(&app, &token, claim_id, "acme-corp").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        forbidden_body["error"],
        "This operation requires the 'namespaces:transfer' scope"
    );

    let (status, org_list_body) = list_namespace_claims(
        &app,
        Some(&format!(
            "owner_org_id={}",
            org_body["id"].as_str().expect("org id should be returned")
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        org_list_body["namespaces"]
            .as_array()
            .expect("namespaces response should be an array")
            .len(),
        0,
        "claim should remain user-owned when transfer scope is missing: {org_list_body}"
    );
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
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let teams = body["teams"]
        .as_array()
        .expect("teams response should be an array");
    assert_eq!(teams.len(), 1);
    assert_eq!(teams[0]["slug"], "release-engineering");
    assert_eq!(
        teams[0]["description"],
        "Owns package publication workflows"
    );

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
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let teams = body["teams"]
        .as_array()
        .expect("teams response should be an array");
    assert_eq!(teams[0]["name"], "Release Operations");
    assert_eq!(
        teams[0]["description"],
        "Coordinates releases and publication"
    );

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
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let teams = body["teams"]
        .as_array()
        .expect("teams response should be an array");
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
    let members = body["members"]
        .as_array()
        .expect("members response should be an array");
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
    let members = body["members"]
        .as_array()
        .expect("members response should be an array");
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

    let (status, _) =
        create_repository(&app, &jwt, "Acme Packages", "acme-packages", Some(org_id)).await;
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
    assert_eq!(
        package_access[0]["permissions"],
        json!(["publish", "write_metadata"])
    );

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
    assert!(
        package_access.is_empty(),
        "package access should be removed"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_team_package_access_rejects_empty_permissions(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id should be returned");

    let (status, _) =
        create_repository(&app, &jwt, "Acme Packages", "acme-packages", Some(org_id)).await;
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

    let (status, _) =
        create_repository(&app, &jwt, "Acme Packages", "acme-packages", Some(org_id)).await;
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

    let (status, _) =
        create_repository(&app, &jwt, "Acme Packages", "acme-packages", Some(org_id)).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) =
        create_repository(&app, &jwt, "Personal Packages", "personal-packages", None).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) =
        create_package(&app, &jwt, "npm", "personal-widget", "personal-packages").await;
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
        .body(Body::from(
            json!({ "permissions": ["publish"] }).to_string(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body = body_json(resp).await;
    assert!(body["error"]
        .as_str()
        .expect("error should be present")
        .contains("same organization"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_team_repository_access_roundtrip_and_audit_filter(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id should be returned");

    let (status, repository_body) = create_repository_with_options(
        &app,
        &jwt,
        "Acme Packages",
        "acme-packages",
        Some(org_id),
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, _) = create_team(
        &app,
        &jwt,
        "acme-corp",
        "Release Engineering",
        "release-engineering",
        Some("Owns repository-wide delegated package workflows."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, grant_body) = grant_team_repository_access(
        &app,
        &jwt,
        "acme-corp",
        "release-engineering",
        "acme-packages",
        &["publish", "write_metadata"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected team repository access response: {grant_body}"
    );
    assert_eq!(grant_body["message"], "Team repository access updated");
    assert_eq!(
        grant_body["permissions"],
        json!(["publish", "write_metadata"])
    );

    let (status, audit_body) = list_org_audit(
        &app,
        &jwt,
        "acme-corp",
        Some("action=team_repository_access_update&per_page=10"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let logs = audit_body["logs"]
        .as_array()
        .expect("logs response should be an array");
    assert_eq!(logs.len(), 1, "response: {audit_body}");
    assert_eq!(logs[0]["action"], "team_repository_access_update");
    assert_eq!(logs[0]["metadata"]["team_slug"], "release-engineering");
    assert_eq!(logs[0]["metadata"]["repository_slug"], "acme-packages");
    assert_eq!(logs[0]["metadata"]["repository_name"], "Acme Packages");
    assert_eq!(
        logs[0]["metadata"]["permissions"],
        json!(["publish", "write_metadata"])
    );

    let (status, list_body) =
        list_team_repository_access(&app, &jwt, "acme-corp", "release-engineering").await;
    assert_eq!(status, StatusCode::OK);
    let repository_access = list_body["repository_access"]
        .as_array()
        .expect("repository access response should be an array");
    assert_eq!(list_body["team"]["slug"], "release-engineering");
    assert_eq!(repository_access.len(), 1);
    assert_eq!(repository_access[0]["slug"], "acme-packages");
    assert_eq!(repository_access[0]["name"], "Acme Packages");
    assert_eq!(repository_access[0]["kind"], "public");
    assert_eq!(repository_access[0]["visibility"], "public");
    assert_eq!(
        repository_access[0]["permissions"],
        json!(["publish", "write_metadata"])
    );

    let (status, update_body) = grant_team_repository_access(
        &app,
        &jwt,
        "acme-corp",
        "release-engineering",
        "acme-packages",
        &["admin"],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(update_body["permissions"], json!(["admin"]));

    let (status, remove_body) = remove_team_repository_access(
        &app,
        &jwt,
        "acme-corp",
        "release-engineering",
        "acme-packages",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(remove_body["message"], "Team repository access removed");

    let (status, final_list_body) =
        list_team_repository_access(&app, &jwt, "acme-corp", "release-engineering").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(final_list_body["repository_access"], json!([]));
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_team_repository_access_rejects_repositories_outside_org(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id should be returned");

    let (status, _) = create_repository_with_options(
        &app,
        &jwt,
        "Acme Packages",
        "acme-packages",
        Some(org_id),
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_repository_with_options(
        &app,
        &jwt,
        "Personal Packages",
        "personal-packages",
        None,
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_team(
        &app,
        &jwt,
        "acme-corp",
        "Release Engineering",
        "release-engineering",
        Some("Owns repository-wide delegated package workflows."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = grant_team_repository_access(
        &app,
        &jwt,
        "acme-corp",
        "release-engineering",
        "personal-packages",
        &["publish"],
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(body["error"]
        .as_str()
        .expect("error should be present")
        .contains("same organization"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_team_namespace_access_roundtrip_and_audit_filter(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id should be returned");

    let (status, claim_body) =
        create_namespace_claim(&app, &jwt, "npm", "@acme", Some(org_id)).await;
    assert_eq!(status, StatusCode::CREATED);
    let claim_id = claim_body["id"]
        .as_str()
        .expect("namespace claim id should be returned");

    let (status, _) = create_team(
        &app,
        &jwt,
        "acme-corp",
        "Release Engineering",
        "release-engineering",
        Some("Owns delegated namespace workflows."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, grant_body) = grant_team_namespace_access(
        &app,
        &jwt,
        "acme-corp",
        "release-engineering",
        claim_id,
        &["admin", "transfer_ownership"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected team namespace access response: {grant_body}"
    );
    assert_eq!(grant_body["message"], "Team namespace access updated");
    assert_eq!(
        grant_body["permissions"],
        json!(["admin", "transfer_ownership"])
    );

    let (status, audit_body) = list_org_audit(
        &app,
        &jwt,
        "acme-corp",
        Some("action=team_namespace_access_update&per_page=10"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let logs = audit_body["logs"]
        .as_array()
        .expect("logs response should be an array");
    assert_eq!(logs.len(), 1, "response: {audit_body}");
    assert_eq!(logs[0]["action"], "team_namespace_access_update");
    assert_eq!(logs[0]["metadata"]["team_slug"], "release-engineering");
    assert_eq!(logs[0]["metadata"]["namespace"], "@acme");
    assert_eq!(
        logs[0]["metadata"]["permissions"],
        json!(["admin", "transfer_ownership"])
    );

    let (status, list_body) =
        list_team_namespace_access(&app, &jwt, "acme-corp", "release-engineering").await;
    assert_eq!(status, StatusCode::OK);
    let namespace_access = list_body["namespace_access"]
        .as_array()
        .expect("namespace access response should be an array");
    assert_eq!(list_body["team"]["slug"], "release-engineering");
    assert_eq!(namespace_access.len(), 1);
    assert_eq!(namespace_access[0]["namespace"], "@acme");
    assert_eq!(namespace_access[0]["ecosystem"], "npm");
    assert_eq!(
        namespace_access[0]["permissions"],
        json!(["admin", "transfer_ownership"])
    );

    let (status, update_body) = grant_team_namespace_access(
        &app,
        &jwt,
        "acme-corp",
        "release-engineering",
        claim_id,
        &["admin"],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(update_body["permissions"], json!(["admin"]));

    let (status, remove_body) =
        remove_team_namespace_access(&app, &jwt, "acme-corp", "release-engineering", claim_id)
            .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(remove_body["message"], "Team namespace access removed");

    let (status, final_list_body) =
        list_team_namespace_access(&app, &jwt, "acme-corp", "release-engineering").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(final_list_body["namespace_access"], json!([]));
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_team_namespace_access_rejects_claims_outside_org(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id should be returned");

    let (status, _) = create_namespace_claim(&app, &jwt, "npm", "@acme", Some(org_id)).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, personal_claim_body) =
        create_namespace_claim(&app, &jwt, "pypi", "alice", None).await;
    assert_eq!(status, StatusCode::CREATED);
    let personal_claim_id = personal_claim_body["id"]
        .as_str()
        .expect("namespace claim id should be returned");

    let (status, _) = create_team(
        &app,
        &jwt,
        "acme-corp",
        "Release Engineering",
        "release-engineering",
        Some("Owns delegated namespace workflows."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = grant_team_namespace_access(
        &app,
        &jwt,
        "acme-corp",
        "release-engineering",
        personal_claim_id,
        &["admin"],
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(body["error"]
        .as_str()
        .expect("error should be present")
        .contains("same organization"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_team_namespace_admin_permission_allows_namespace_delete(pool: PgPool) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &alice_jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id should be returned");

    let (status, claim_body) =
        create_namespace_claim(&app, &alice_jwt, "npm", "@acme", Some(org_id)).await;
    assert_eq!(status, StatusCode::CREATED);
    let claim_id = claim_body["id"]
        .as_str()
        .expect("namespace claim id should be returned");

    let (status, _) = add_org_member(&app, &alice_jwt, "acme-corp", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = create_team(
        &app,
        &alice_jwt,
        "acme-corp",
        "Namespace Team",
        "namespace-team",
        Some("Can manage namespace claims."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) =
        add_team_member_to_team(&app, &alice_jwt, "acme-corp", "namespace-team", "bob").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, before_grant_body) = list_namespace_claims_authenticated(
        &app,
        &bob_jwt,
        Some(&format!("owner_org_id={org_id}")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let claims = before_grant_body["namespaces"]
        .as_array()
        .expect("namespace response should be an array");
    assert_eq!(claims[0]["can_manage"], false);

    let (status, _) = grant_team_namespace_access(
        &app,
        &alice_jwt,
        "acme-corp",
        "namespace-team",
        claim_id,
        &["admin"],
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, updated_org_body) = update_org_profile(
        &app,
        &alice_jwt,
        "acme-corp",
        json!({ "mfa_required": true }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated_org_body["mfa_required"], true);

    let (status, after_grant_body) = list_namespace_claims_authenticated(
        &app,
        &bob_jwt,
        Some(&format!("owner_org_id={org_id}")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let claims = after_grant_body["namespaces"]
        .as_array()
        .expect("namespace response should be an array");
    assert_eq!(claims[0]["can_manage"], false);
    assert_eq!(claims[0]["can_transfer"], false);

    let (status, forbidden_body) = delete_namespace_claim(&app, &bob_jwt, claim_id).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(
        forbidden_body["error"]
            .as_str()
            .expect("error should be present")
            .contains("This organization requires MFA for elevated members before write actions are allowed"),
        "unexpected error response: {forbidden_body}"
    );

    sqlx::query("UPDATE users SET mfa_enabled = true WHERE username = 'bob'")
        .execute(&pool)
        .await
        .expect("should enable MFA for bob");

    let (status, after_mfa_body) = list_namespace_claims_authenticated(
        &app,
        &bob_jwt,
        Some(&format!("owner_org_id={org_id}")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let claims = after_mfa_body["namespaces"]
        .as_array()
        .expect("namespace response should be an array");
    assert_eq!(claims[0]["can_manage"], true);
    assert_eq!(claims[0]["can_transfer"], true);

    let (status, delete_body) = delete_namespace_claim(&app, &bob_jwt, claim_id).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected namespace delete response: {delete_body}"
    );
    assert_eq!(delete_body["message"], "Namespace claim deleted");
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_team_namespace_transfer_permission_allows_namespace_transfer(pool: PgPool) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, source_org_body) = create_org(&app, &alice_jwt, "Source Org", "source-org").await;
    assert_eq!(status, StatusCode::CREATED);
    let source_org_id = source_org_body["id"].as_str().expect("source org id");
    let source_org_uuid = Uuid::parse_str(source_org_id).expect("source org id should parse");

    let (status, target_org_body) = create_org(&app, &bob_jwt, "Target Org", "target-org").await;
    assert_eq!(status, StatusCode::CREATED);
    let target_org_id = target_org_body["id"].as_str().expect("target org id");
    let target_org_uuid = Uuid::parse_str(target_org_id).expect("target org id should parse");

    let (status, claim_body) =
        create_namespace_claim(&app, &alice_jwt, "npm", "@source", Some(source_org_id)).await;
    assert_eq!(status, StatusCode::CREATED);
    let claim_id = claim_body["id"]
        .as_str()
        .expect("namespace claim id should be returned");
    let claim_uuid = Uuid::parse_str(claim_id).expect("claim id should parse");

    let (status, _) = add_org_member(&app, &alice_jwt, "source-org", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = create_team(
        &app,
        &alice_jwt,
        "source-org",
        "Transfer Team",
        "transfer-team",
        Some("Can transfer namespace ownership into controlled organizations."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) =
        add_team_member_to_team(&app, &alice_jwt, "source-org", "transfer-team", "bob").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, before_grant_body) = list_namespace_claims_authenticated(
        &app,
        &bob_jwt,
        Some(&format!("owner_org_id={source_org_id}")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let claims = before_grant_body["namespaces"]
        .as_array()
        .expect("namespace response should be an array");
    assert_eq!(claims[0]["can_transfer"], false);

    let (status, grant_body) = grant_team_namespace_access(
        &app,
        &alice_jwt,
        "source-org",
        "transfer-team",
        claim_id,
        &["transfer_ownership"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected team namespace access response: {grant_body}"
    );

    let (status, updated_org_body) = update_org_profile(
        &app,
        &alice_jwt,
        "source-org",
        json!({ "mfa_required": true }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated_org_body["mfa_required"], true);

    let grants_before_transfer: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM team_namespace_access WHERE namespace_claim_id = $1",
    )
    .bind(claim_uuid)
    .fetch_one(&pool)
    .await
    .expect("team namespace access count before transfer");
    assert_eq!(grants_before_transfer, 1);

    let (status, after_grant_body) = list_namespace_claims_authenticated(
        &app,
        &bob_jwt,
        Some(&format!("owner_org_id={source_org_id}")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let claims = after_grant_body["namespaces"]
        .as_array()
        .expect("namespace response should be an array");
    assert_eq!(claims[0]["can_manage"], false);
    assert_eq!(claims[0]["can_transfer"], false);

    let (status, forbidden_body) =
        transfer_namespace_claim(&app, &bob_jwt, claim_id, "target-org").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(
        forbidden_body["error"]
            .as_str()
            .expect("error should be present")
            .contains("This organization requires MFA for elevated members before write actions are allowed"),
        "unexpected error response: {forbidden_body}"
    );

    sqlx::query("UPDATE users SET mfa_enabled = true WHERE username = 'bob'")
        .execute(&pool)
        .await
        .expect("should enable MFA for bob");

    let (status, after_mfa_body) = list_namespace_claims_authenticated(
        &app,
        &bob_jwt,
        Some(&format!("owner_org_id={source_org_id}")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let claims = after_mfa_body["namespaces"]
        .as_array()
        .expect("namespace response should be an array");
    assert_eq!(claims[0]["can_transfer"], true);

    let (status, transfer_body) =
        transfer_namespace_claim(&app, &bob_jwt, claim_id, "target-org").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected namespace transfer response: {transfer_body}"
    );
    assert_eq!(transfer_body["owner"]["slug"], "target-org");

    let grants_after_transfer: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM team_namespace_access WHERE namespace_claim_id = $1",
    )
    .bind(claim_uuid)
    .fetch_one(&pool)
    .await
    .expect("team namespace access count after transfer");
    assert_eq!(grants_after_transfer, 0);

    let (status, target_list_body) =
        list_namespace_claims(&app, Some(&format!("owner_org_id={target_org_id}"))).await;
    assert_eq!(status, StatusCode::OK);
    let claims = target_list_body["namespaces"]
        .as_array()
        .expect("namespace response should be an array");
    assert_eq!(claims.len(), 1);
    assert_eq!(claims[0]["namespace"], "@source");

    let audit_row = sqlx::query(
        "SELECT target_org_id, metadata \
         FROM audit_logs \
         WHERE action = 'namespace_claim_transfer'::audit_action \
         ORDER BY occurred_at DESC \
         LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("namespace transfer audit row should exist");
    let audit_target_org_id: Option<Uuid> = audit_row
        .try_get("target_org_id")
        .expect("audit target org id should be readable");
    let audit_metadata: Value = audit_row
        .try_get("metadata")
        .expect("audit metadata should be readable");

    assert_eq!(audit_target_org_id, Some(target_org_uuid));
    assert_eq!(
        audit_metadata["previous_owner_org_id"],
        json!(source_org_uuid)
    );
    assert_eq!(audit_metadata["new_owner_org_id"], json!(target_org_uuid));
    assert_eq!(audit_metadata["revoked_team_grants"], 1);
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_team_repository_write_metadata_permission_allows_package_creation_and_metadata_updates_but_not_repository_settings(
    pool: PgPool,
) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, source_org_body) = create_org(&app, &alice_jwt, "Source Org", "source-org").await;
    assert_eq!(status, StatusCode::CREATED);
    let source_org_id = source_org_body["id"].as_str().expect("source org id");

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "Source Packages",
        "source-packages",
        Some(source_org_id),
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, _) = add_org_member(&app, &alice_jwt, "source-org", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_team(
        &app,
        &alice_jwt,
        "source-org",
        "Metadata Editors",
        "metadata-editors",
        Some("Can create packages and update package metadata within a repository."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) =
        add_team_member_to_team(&app, &alice_jwt, "source-org", "metadata-editors", "bob").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, grant_body) = grant_team_repository_access(
        &app,
        &alice_jwt,
        "source-org",
        "metadata-editors",
        "source-packages",
        &["write_metadata"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected team repository access response: {grant_body}"
    );

    let (status, repository_detail_before) =
        get_repository_detail(&app, Some(&bob_jwt), "source-packages").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(repository_detail_before["can_manage"], false);
    assert_eq!(repository_detail_before["can_create_packages"], true);

    let (status, updated_org_body) = update_org_profile(
        &app,
        &alice_jwt,
        "source-org",
        json!({ "mfa_required": true }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated_org_body["mfa_required"], true);

    let (status, repository_detail_mfa_required) =
        get_repository_detail(&app, Some(&bob_jwt), "source-packages").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(repository_detail_mfa_required["can_manage"], false);
    assert_eq!(repository_detail_mfa_required["can_create_packages"], false);

    let (status, mfa_forbidden_body) = create_package(
        &app,
        &bob_jwt,
        "npm",
        "repo-metadata-widget",
        "source-packages",
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(
        mfa_forbidden_body["error"]
            .as_str()
            .expect("error should be present")
            .contains("This organization requires MFA for elevated members before write actions are allowed"),
        "unexpected error response: {mfa_forbidden_body}"
    );

    sqlx::query("UPDATE users SET mfa_enabled = true WHERE username = 'bob'")
        .execute(&pool)
        .await
        .expect("should enable MFA for bob");

    let (status, repository_detail_after_mfa) =
        get_repository_detail(&app, Some(&bob_jwt), "source-packages").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(repository_detail_after_mfa["can_manage"], false);
    assert_eq!(repository_detail_after_mfa["can_create_packages"], true);

    let (status, create_body) = create_package(
        &app,
        &bob_jwt,
        "npm",
        "repo-metadata-widget",
        "source-packages",
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {create_body}"
    );

    let (status, update_body) = update_package_metadata(
        &app,
        &bob_jwt,
        "npm",
        "repo-metadata-widget",
        json!({
            "description": "Managed through repository-scoped metadata delegation.",
            "homepage": "https://packages.example.test/repo-metadata-widget",
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected package update response: {update_body}"
    );

    let (status, package_detail) =
        get_package_detail(&app, Some(&bob_jwt), "npm", "repo-metadata-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(package_detail["owner_org_slug"], "source-org");
    assert_eq!(package_detail["can_manage_metadata"], true);
    assert_eq!(package_detail["can_manage_releases"], false);
    assert_eq!(
        package_detail["description"],
        "Managed through repository-scoped metadata delegation."
    );

    let (status, denied_repository_update_body) = update_repository_detail(
        &app,
        &bob_jwt,
        "source-packages",
        json!({
            "description": "This should remain reserved for repository admins.",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(denied_repository_update_body["error"]
        .as_str()
        .expect("error should be present")
        .contains("modify this repository"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_team_repository_publish_permission_allows_release_creation_for_repository_packages(
    pool: PgPool,
) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, source_org_body) = create_org(&app, &alice_jwt, "Source Org", "source-org").await;
    assert_eq!(status, StatusCode::CREATED);
    let source_org_id = source_org_body["id"].as_str().expect("source org id");

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "Source Packages",
        "source-packages",
        Some(source_org_id),
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, package_body) = create_package_with_options(
        &app,
        &alice_jwt,
        "npm",
        "release-widget",
        "source-packages",
        Some("private"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );

    let (status, _) = add_org_member(&app, &alice_jwt, "source-org", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_team(
        &app,
        &alice_jwt,
        "source-org",
        "Release Engineering",
        "release-engineering",
        Some("Can publish releases for packages inside the repository."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) =
        add_team_member_to_team(&app, &alice_jwt, "source-org", "release-engineering", "bob").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, grant_body) = grant_team_repository_access(
        &app,
        &alice_jwt,
        "source-org",
        "release-engineering",
        "source-packages",
        &["publish"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected team repository access response: {grant_body}"
    );

    let (status, detail_before_release) =
        get_package_detail(&app, Some(&bob_jwt), "npm", "release-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail_before_release["can_manage_metadata"], false);
    assert_eq!(detail_before_release["can_manage_releases"], true);

    let (status, release_body) =
        create_release_for_package(&app, &bob_jwt, "npm", "release-widget", "1.0.0").await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected release response: {release_body}"
    );
    assert_eq!(release_body["version"], "1.0.0");
    assert_eq!(release_body["status"], "quarantine");

    let (status, denied_update_body) = update_package_metadata(
        &app,
        &bob_jwt,
        "npm",
        "release-widget",
        json!({
            "description": "Should not be writable with publish-only repository delegation.",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(denied_update_body["error"]
        .as_str()
        .expect("error should be present")
        .contains("update this package's metadata"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_team_repository_transfer_permission_allows_package_transfer(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, source_org_body) = create_org(&app, &alice_jwt, "Source Org", "source-org").await;
    assert_eq!(status, StatusCode::CREATED);
    let source_org_id = source_org_body["id"].as_str().expect("source org id");

    let (status, _) = create_org(&app, &bob_jwt, "Target Org", "target-org").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "Source Packages",
        "source-packages",
        Some(source_org_id),
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, package_body) = create_package_with_options(
        &app,
        &alice_jwt,
        "npm",
        "transfer-widget",
        "source-packages",
        Some("private"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );

    let (status, _) = add_org_member(&app, &alice_jwt, "source-org", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_team(
        &app,
        &alice_jwt,
        "source-org",
        "Transfer Team",
        "transfer-team",
        Some("Can transfer packages owned by this repository into controlled organizations."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) =
        add_team_member_to_team(&app, &alice_jwt, "source-org", "transfer-team", "bob").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, detail_before_grant) =
        get_package_detail(&app, Some(&bob_jwt), "npm", "transfer-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail_before_grant["can_transfer"], false);

    let (status, grant_body) = grant_team_repository_access(
        &app,
        &alice_jwt,
        "source-org",
        "transfer-team",
        "source-packages",
        &["transfer_ownership"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected team repository access response: {grant_body}"
    );

    let (status, detail_after_grant) =
        get_package_detail(&app, Some(&bob_jwt), "npm", "transfer-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail_after_grant["can_transfer"], true);

    let (status, transfer_body) =
        transfer_package_ownership(&app, &bob_jwt, "npm", "transfer-widget", "target-org").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected package transfer response: {transfer_body}"
    );
    assert_eq!(transfer_body["message"], "Package ownership transferred");
    assert_eq!(transfer_body["owner"]["slug"], "target-org");
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_team_repository_admin_permission_allows_repository_updates_and_team_delete_cleans_up_access(
    pool: PgPool,
) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, source_org_body) = create_org(&app, &alice_jwt, "Source Org", "source-org").await;
    assert_eq!(status, StatusCode::CREATED);
    let source_org_id = source_org_body["id"].as_str().expect("source org id");
    let source_org_uuid =
        uuid::Uuid::parse_str(source_org_id).expect("source org id should parse as UUID");

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "Source Packages",
        "source-packages",
        Some(source_org_id),
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );
    let repository_id = uuid::Uuid::parse_str(
        repository_body["id"]
            .as_str()
            .expect("repository id should be returned"),
    )
    .expect("repository id should parse as UUID");

    let (status, _) = add_org_member(&app, &alice_jwt, "source-org", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_team(
        &app,
        &alice_jwt,
        "source-org",
        "Repository Admins",
        "repository-admins",
        Some("Can manage repository configuration and delegated package workflows."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) =
        add_team_member_to_team(&app, &alice_jwt, "source-org", "repository-admins", "bob").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, grant_body) = grant_team_repository_access(
        &app,
        &alice_jwt,
        "source-org",
        "repository-admins",
        "source-packages",
        &["admin"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected team repository access response: {grant_body}"
    );

    let access_rows_before_delete: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM team_repository_access WHERE repository_id = $1",
    )
    .bind(repository_id)
    .fetch_one(&pool)
    .await
    .expect("team repository access count before team delete");
    assert_eq!(access_rows_before_delete, 1);

    let (status, repository_detail_before) =
        get_repository_detail(&app, Some(&bob_jwt), "source-packages").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(repository_detail_before["can_manage"], true);
    assert_eq!(repository_detail_before["can_create_packages"], true);

    let (status, updated_org_body) = update_org_profile(
        &app,
        &alice_jwt,
        "source-org",
        json!({ "mfa_required": true }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated_org_body["mfa_required"], true);

    let (status, repository_detail_mfa_required) =
        get_repository_detail(&app, Some(&bob_jwt), "source-packages").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(repository_detail_mfa_required["can_manage"], false);
    assert_eq!(repository_detail_mfa_required["can_create_packages"], false);

    let (status, mfa_forbidden_body) = update_repository_detail(
        &app,
        &bob_jwt,
        "source-packages",
        json!({
            "description": "Managed by the repository-admins team.",
            "upstream_url": "https://github.com/acme/source-packages",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(
        mfa_forbidden_body["error"]
            .as_str()
            .expect("error should be present")
            .contains("This organization requires MFA for elevated members before write actions are allowed"),
        "unexpected error response: {mfa_forbidden_body}"
    );

    sqlx::query("UPDATE users SET mfa_enabled = true WHERE username = 'bob'")
        .execute(&pool)
        .await
        .expect("should enable MFA for bob");

    let (status, repository_detail_after_mfa) =
        get_repository_detail(&app, Some(&bob_jwt), "source-packages").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(repository_detail_after_mfa["can_manage"], true);
    assert_eq!(repository_detail_after_mfa["can_create_packages"], true);

    let (status, update_body) = update_repository_detail(
        &app,
        &bob_jwt,
        "source-packages",
        json!({
            "description": "Managed by the repository-admins team.",
            "upstream_url": "https://github.com/acme/source-packages",
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected repository update response: {update_body}"
    );
    assert_eq!(update_body["message"], "Repository updated");

    let (status, repository_detail_after) =
        get_repository_detail(&app, Some(&alice_jwt), "source-packages").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        repository_detail_after["description"],
        "Managed by the repository-admins team."
    );
    assert_eq!(
        repository_detail_after["upstream_url"],
        "https://github.com/acme/source-packages"
    );

    sqlx::query("UPDATE users SET mfa_enabled = true WHERE username = 'alice'")
        .execute(&pool)
        .await
        .expect("should enable MFA for alice");

    let req = Request::builder()
        .method(Method::DELETE)
        .uri("/v1/orgs/source-org/teams/repository-admins")
        .header(header::AUTHORIZATION, format!("Bearer {alice_jwt}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let delete_body = body_json(resp).await;
    assert_eq!(delete_body["message"], "Team deleted");

    let access_rows_after_delete: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM team_repository_access WHERE repository_id = $1",
    )
    .bind(repository_id)
    .fetch_one(&pool)
    .await
    .expect("team repository access count after team delete");
    assert_eq!(access_rows_after_delete, 0);

    let delete_audit_metadata: Value = sqlx::query_scalar(
        "SELECT metadata \
         FROM audit_logs \
         WHERE action = 'team_delete'::audit_action AND target_org_id = $1 \
         ORDER BY occurred_at DESC \
         LIMIT 1",
    )
    .bind(source_org_uuid)
    .fetch_one(&pool)
    .await
    .expect("team delete audit row should exist");
    assert_eq!(delete_audit_metadata["team_slug"], "repository-admins");
    assert_eq!(delete_audit_metadata["removed_repository_access_count"], 1);
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_team_repository_access_rows_cascade_on_repository_delete(pool: PgPool) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id should be returned");

    let (status, repository_body) = create_repository_with_options(
        &app,
        &jwt,
        "Acme Packages",
        "acme-packages",
        Some(org_id),
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );
    let repository_id = uuid::Uuid::parse_str(
        repository_body["id"]
            .as_str()
            .expect("repository id should be returned"),
    )
    .expect("repository id should parse as UUID");

    let (status, _) = create_team(
        &app,
        &jwt,
        "acme-corp",
        "Release Engineering",
        "release-engineering",
        Some("Owns repository-wide delegated package workflows."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, grant_body) = grant_team_repository_access(
        &app,
        &jwt,
        "acme-corp",
        "release-engineering",
        "acme-packages",
        &["publish"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected team repository access response: {grant_body}"
    );

    let access_rows_before_delete: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM team_repository_access WHERE repository_id = $1",
    )
    .bind(repository_id)
    .fetch_one(&pool)
    .await
    .expect("team repository access count before repository delete");
    assert_eq!(access_rows_before_delete, 1);

    sqlx::query("DELETE FROM repositories WHERE id = $1")
        .bind(repository_id)
        .execute(&pool)
        .await
        .expect("repository should delete successfully");

    let access_rows_after_delete: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM team_repository_access WHERE repository_id = $1",
    )
    .bind(repository_id)
    .fetch_one(&pool)
    .await
    .expect("team repository access count after repository delete");
    assert_eq!(access_rows_after_delete, 0);
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_org_repository_list_surfaces_transfer_capability(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &alice_jwt, "Acme Org", "acme-org").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id should be returned");

    let (status, repository_body) = create_repository(
        &app,
        &alice_jwt,
        "Acme Packages",
        "acme-packages",
        Some(org_id),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, owner_body) = list_org_repositories(&app, Some(&alice_jwt), "acme-org").await;
    assert_eq!(status, StatusCode::OK);
    let owner_repositories = owner_body["repositories"]
        .as_array()
        .expect("owner repositories response should be an array");
    assert_eq!(owner_repositories.len(), 1);
    assert_eq!(owner_repositories[0]["slug"], "acme-packages");
    assert_eq!(owner_repositories[0]["can_transfer"], true);

    let (status, anonymous_body) = list_org_repositories(&app, None, "acme-org").await;
    assert_eq!(status, StatusCode::OK);
    let anonymous_repositories = anonymous_body["repositories"]
        .as_array()
        .expect("anonymous repositories response should be an array");
    assert_eq!(anonymous_repositories.len(), 1);
    assert_eq!(anonymous_repositories[0]["slug"], "acme-packages");
    assert_eq!(anonymous_repositories[0]["can_transfer"], false);
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_repository_ownership_transfer_success_clears_team_access_and_updates_org_views(
    pool: PgPool,
) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, source_org_body) = create_org(&app, &alice_jwt, "Source Org", "source-org").await;
    assert_eq!(status, StatusCode::CREATED);
    let source_org_id = source_org_body["id"].as_str().expect("source org id");
    let source_org_uuid =
        uuid::Uuid::parse_str(source_org_id).expect("source org id should parse as UUID");

    let (status, target_org_body) = create_org(&app, &alice_jwt, "Target Org", "target-org").await;
    assert_eq!(status, StatusCode::CREATED);
    let target_org_id = target_org_body["id"].as_str().expect("target org id");
    let target_org_uuid =
        uuid::Uuid::parse_str(target_org_id).expect("target org id should parse as UUID");

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "Source Packages",
        "source-packages",
        Some(source_org_id),
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );
    let repository_id = uuid::Uuid::parse_str(
        repository_body["id"]
            .as_str()
            .expect("repository id should be returned"),
    )
    .expect("repository id should parse as UUID");

    let (status, _) = create_team(
        &app,
        &alice_jwt,
        "source-org",
        "Release Engineering",
        "release-engineering",
        Some("Owns temporary repository-scoped grants."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, grant_body) = grant_team_repository_access(
        &app,
        &alice_jwt,
        "source-org",
        "release-engineering",
        "source-packages",
        &["publish", "write_metadata"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected team repository access response: {grant_body}"
    );

    let team_grants_before_transfer: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM team_repository_access WHERE repository_id = $1",
    )
    .bind(repository_id)
    .fetch_one(&pool)
    .await
    .expect("team repository access count before transfer");
    assert_eq!(team_grants_before_transfer, 2);

    let (status, detail_before_transfer) =
        get_repository_detail(&app, Some(&alice_jwt), "source-packages").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail_before_transfer["owner_org_slug"], "source-org");
    assert_eq!(detail_before_transfer["can_transfer"], true);

    let (status, transfer_body) =
        transfer_repository_ownership(&app, &alice_jwt, "source-packages", "target-org").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected repository transfer response: {transfer_body}"
    );
    assert_eq!(transfer_body["message"], "Repository ownership transferred");
    assert_eq!(transfer_body["owner"]["slug"], "target-org");

    let team_grants_after_transfer: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM team_repository_access WHERE repository_id = $1",
    )
    .bind(repository_id)
    .fetch_one(&pool)
    .await
    .expect("team repository access count after transfer");
    assert_eq!(team_grants_after_transfer, 0);

    let (status, detail_after_transfer) =
        get_repository_detail(&app, Some(&alice_jwt), "source-packages").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail_after_transfer["owner_org_slug"], "target-org");
    assert_eq!(detail_after_transfer["can_transfer"], true);

    let (status, source_org_repositories) =
        list_org_repositories(&app, Some(&alice_jwt), "source-org").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        source_org_repositories["repositories"]
            .as_array()
            .expect("source org repositories response should be an array")
            .len(),
        0
    );

    let (status, target_org_repositories) =
        list_org_repositories(&app, Some(&alice_jwt), "target-org").await;
    assert_eq!(status, StatusCode::OK);
    let target_repositories = target_org_repositories["repositories"]
        .as_array()
        .expect("target org repositories response should be an array");
    assert_eq!(target_repositories.len(), 1);
    assert_eq!(target_repositories[0]["slug"], "source-packages");
    assert_eq!(target_repositories[0]["can_transfer"], true);

    let audit_row = sqlx::query(
        "SELECT target_org_id, metadata \
         FROM audit_logs \
         WHERE action = 'repository_transfer'::audit_action \
         ORDER BY occurred_at DESC \
         LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("repository transfer audit row should exist");
    let audit_target_org_id: Option<uuid::Uuid> = audit_row
        .try_get("target_org_id")
        .expect("audit target org id should be readable");
    let audit_metadata: Value = audit_row
        .try_get("metadata")
        .expect("audit metadata should be readable");

    assert_eq!(audit_target_org_id, Some(target_org_uuid));
    assert_eq!(audit_metadata["repository_id"], json!(repository_id));
    assert_eq!(audit_metadata["repository_slug"], "source-packages");
    assert_eq!(
        audit_metadata["previous_owner_org_id"],
        json!(source_org_uuid)
    );
    assert_eq!(audit_metadata["new_owner_org_id"], json!(target_org_uuid));
    assert_eq!(audit_metadata["new_owner_org_slug"], "target-org");
    assert_eq!(audit_metadata["revoked_team_grants"], 2);
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_repository_ownership_transfer_requires_source_transfer_access(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, repository_body) =
        create_repository(&app, &alice_jwt, "Alice Packages", "alice-packages", None).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, _) = create_org(&app, &bob_jwt, "Bob Org", "bob-org").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, detail_before_transfer) =
        get_repository_detail(&app, Some(&bob_jwt), "alice-packages").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail_before_transfer["owner_username"], "alice");
    assert_eq!(detail_before_transfer["can_transfer"], false);

    let (status, transfer_body) =
        transfer_repository_ownership(&app, &bob_jwt, "alice-packages", "bob-org").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(transfer_body["error"]
        .as_str()
        .expect("error should be present")
        .contains("transfer ownership"));

    let (status, detail_after_transfer) =
        get_repository_detail(&app, Some(&alice_jwt), "alice-packages").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail_after_transfer["owner_username"], "alice");
    assert_eq!(detail_after_transfer["owner_org_slug"], Value::Null);
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_repository_ownership_transfer_requires_target_org_admin_membership(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, _) = create_org(&app, &bob_jwt, "Bob Org", "bob-org").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, repository_body) =
        create_repository(&app, &alice_jwt, "Alice Packages", "alice-packages", None).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, detail_before_transfer) =
        get_repository_detail(&app, Some(&alice_jwt), "alice-packages").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail_before_transfer["owner_username"], "alice");
    assert_eq!(detail_before_transfer["can_transfer"], true);

    let (status, transfer_body) =
        transfer_repository_ownership(&app, &alice_jwt, "alice-packages", "bob-org").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(transfer_body["error"]
        .as_str()
        .expect("error should be present")
        .contains("target organization"));

    let (status, detail_after_transfer) =
        get_repository_detail(&app, Some(&alice_jwt), "alice-packages").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail_after_transfer["owner_username"], "alice");
    assert_eq!(detail_after_transfer["owner_org_slug"], Value::Null);
    assert_eq!(detail_after_transfer["can_transfer"], true);
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_team_repository_transfer_permission_allows_repository_transfer(pool: PgPool) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, source_org_body) = create_org(&app, &alice_jwt, "Source Org", "source-org").await;
    assert_eq!(status, StatusCode::CREATED);
    let source_org_id = source_org_body["id"].as_str().expect("source org id");

    let (status, _) = create_org(&app, &bob_jwt, "Target Org", "target-org").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "Source Packages",
        "source-packages",
        Some(source_org_id),
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, _) = add_org_member(&app, &alice_jwt, "source-org", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_team(
        &app,
        &alice_jwt,
        "source-org",
        "Transfer Team",
        "transfer-team",
        Some("Can transfer repository ownership into controlled organizations."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) =
        add_team_member_to_team(&app, &alice_jwt, "source-org", "transfer-team", "bob").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, detail_before_grant) =
        get_repository_detail(&app, Some(&bob_jwt), "source-packages").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail_before_grant["can_transfer"], false);

    let (status, grant_body) = grant_team_repository_access(
        &app,
        &alice_jwt,
        "source-org",
        "transfer-team",
        "source-packages",
        &["transfer_ownership"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected team repository access response: {grant_body}"
    );

    let (status, detail_after_grant) =
        get_repository_detail(&app, Some(&bob_jwt), "source-packages").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail_after_grant["can_transfer"], true);

    let (status, updated_org_body) = update_org_profile(
        &app,
        &alice_jwt,
        "source-org",
        json!({ "mfa_required": true }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated_org_body["mfa_required"], true);

    let (status, detail_mfa_required) =
        get_repository_detail(&app, Some(&bob_jwt), "source-packages").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail_mfa_required["can_transfer"], false);

    let (status, mfa_forbidden_body) =
        transfer_repository_ownership(&app, &bob_jwt, "source-packages", "target-org").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(
        mfa_forbidden_body["error"]
            .as_str()
            .expect("error should be present")
            .contains("This organization requires MFA for elevated members before write actions are allowed"),
        "unexpected error response: {mfa_forbidden_body}"
    );

    sqlx::query("UPDATE users SET mfa_enabled = true WHERE username = 'bob'")
        .execute(&pool)
        .await
        .expect("should enable MFA for bob");

    let (status, detail_after_mfa) =
        get_repository_detail(&app, Some(&bob_jwt), "source-packages").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail_after_mfa["can_transfer"], true);

    let (status, transfer_body) =
        transfer_repository_ownership(&app, &bob_jwt, "source-packages", "target-org").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected repository transfer response: {transfer_body}"
    );
    assert_eq!(transfer_body["message"], "Repository ownership transferred");
    assert_eq!(transfer_body["owner"]["slug"], "target-org");

    let (status, detail_after_transfer) =
        get_repository_detail(&app, Some(&bob_jwt), "source-packages").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail_after_transfer["owner_org_slug"], "target-org");
    assert_eq!(detail_after_transfer["can_transfer"], true);
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_package_ownership_transfer_success_updates_package_detail(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, _) = create_org(&app, &alice_jwt, "Acme Org", "acme-org").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, repository_body) =
        create_repository(&app, &alice_jwt, "Alice Packages", "alice-packages", None).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, package_body) =
        create_package(&app, &alice_jwt, "npm", "acme-widget", "alice-packages").await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );

    let (status, detail_before) =
        get_package_detail(&app, Some(&alice_jwt), "npm", "acme-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail_before["owner_username"], "alice");
    assert_eq!(detail_before["owner_org_slug"], Value::Null);
    assert_eq!(detail_before["can_transfer"], true);

    let (status, transfer_body) =
        transfer_package_ownership(&app, &alice_jwt, "npm", "acme-widget", "acme-org").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected package transfer response: {transfer_body}"
    );
    assert_eq!(transfer_body["message"], "Package ownership transferred");
    assert_eq!(transfer_body["owner"]["type"], "organization");
    assert_eq!(transfer_body["owner"]["slug"], "acme-org");

    let (status, detail_after) =
        get_package_detail(&app, Some(&alice_jwt), "npm", "acme-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail_after["owner_username"], Value::Null);
    assert_eq!(detail_after["owner_org_slug"], "acme-org");
    assert_eq!(detail_after["can_transfer"], true);

    let (status, anonymous_detail) = get_package_detail(&app, None, "npm", "acme-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(anonymous_detail["owner_org_slug"], "acme-org");
    assert_eq!(anonymous_detail["can_transfer"], false);
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_org_package_list_surfaces_transfer_capability(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &alice_jwt, "Acme Org", "acme-org").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id should be returned");

    let (status, repository_body) = create_repository(
        &app,
        &alice_jwt,
        "Acme Packages",
        "acme-packages",
        Some(org_id),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, package_body) =
        create_package(&app, &alice_jwt, "npm", "acme-widget", "acme-packages").await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );

    let (status, owner_body) = list_org_packages(&app, Some(&alice_jwt), "acme-org").await;
    assert_eq!(status, StatusCode::OK);
    let owner_packages = owner_body["packages"]
        .as_array()
        .expect("owner packages response should be an array");
    assert_eq!(owner_packages.len(), 1);
    assert_eq!(owner_packages[0]["name"], "acme-widget");
    assert_eq!(owner_packages[0]["can_transfer"], true);

    let (status, anonymous_body) = list_org_packages(&app, None, "acme-org").await;
    assert_eq!(status, StatusCode::OK);
    let anonymous_packages = anonymous_body["packages"]
        .as_array()
        .expect("anonymous packages response should be an array");
    assert_eq!(anonymous_packages.len(), 1);
    assert_eq!(anonymous_packages[0]["name"], "acme-widget");
    assert_eq!(anonymous_packages[0]["can_transfer"], false);
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_package_ownership_transfer_requires_source_transfer_access(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, _) = create_org(&app, &alice_jwt, "Alice Org", "alice-org").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, repository_body) =
        create_repository(&app, &bob_jwt, "Bob Packages", "bob-packages", None).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, package_body) =
        create_package(&app, &bob_jwt, "npm", "bob-widget", "bob-packages").await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );

    let (status, detail) = get_package_detail(&app, Some(&alice_jwt), "npm", "bob-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["owner_username"], "bob");
    assert_eq!(detail["can_transfer"], false);

    let (status, body) =
        transfer_package_ownership(&app, &alice_jwt, "npm", "bob-widget", "alice-org").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(body["error"]
        .as_str()
        .expect("error should be present")
        .contains("transfer ownership"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_package_ownership_transfer_requires_target_org_admin_membership(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, _) = create_org(&app, &bob_jwt, "Bob Org", "bob-org").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, repository_body) = create_repository(
        &app,
        &alice_jwt,
        "Alice Packages",
        "alice-source-packages",
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, package_body) = create_package(
        &app,
        &alice_jwt,
        "npm",
        "alice-widget",
        "alice-source-packages",
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );

    let (status, detail_before) =
        get_package_detail(&app, Some(&alice_jwt), "npm", "alice-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail_before["owner_username"], "alice");
    assert_eq!(detail_before["can_transfer"], true);

    let (status, body) =
        transfer_package_ownership(&app, &alice_jwt, "npm", "alice-widget", "bob-org").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(body["error"]
        .as_str()
        .expect("error should be present")
        .contains("target organization"));

    let (status, detail_after) =
        get_package_detail(&app, Some(&alice_jwt), "npm", "alice-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail_after["owner_username"], "alice");
    assert_eq!(detail_after["owner_org_slug"], Value::Null);
    assert_eq!(detail_after["can_transfer"], true);
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_team_publish_permission_allows_release_creation_but_not_metadata_updates(
    pool: PgPool,
) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, source_org_body) = create_org(&app, &alice_jwt, "Source Org", "source-org").await;
    assert_eq!(status, StatusCode::CREATED);
    let source_org_id = source_org_body["id"].as_str().expect("source org id");

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "Source Packages",
        "source-packages",
        Some(source_org_id),
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, package_body) = create_package_with_options(
        &app,
        &alice_jwt,
        "npm",
        "release-widget",
        "source-packages",
        Some("private"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );

    let (status, _) = add_org_member(&app, &alice_jwt, "source-org", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_team(
        &app,
        &alice_jwt,
        "source-org",
        "Release Engineering",
        "release-engineering",
        Some("Owns release execution without metadata control."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) =
        add_team_member_to_team(&app, &alice_jwt, "source-org", "release-engineering", "bob").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, grant_body) = grant_team_package_access(
        &app,
        &alice_jwt,
        "source-org",
        "release-engineering",
        "npm",
        "release-widget",
        &["publish"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected team package access response: {grant_body}"
    );

    let (status, detail_before_release) =
        get_package_detail(&app, Some(&bob_jwt), "npm", "release-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail_before_release["can_manage_metadata"], false);
    assert_eq!(detail_before_release["can_manage_releases"], true);

    let (status, release_body) =
        create_release_for_package(&app, &bob_jwt, "npm", "release-widget", "1.0.0").await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected release response: {release_body}"
    );
    assert_eq!(release_body["version"], "1.0.0");
    assert_eq!(release_body["status"], "quarantine");

    let (status, denied_update_body) = update_package_metadata(
        &app,
        &bob_jwt,
        "npm",
        "release-widget",
        json!({
            "description": "Should not be writable with publish-only delegation.",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(denied_update_body["error"]
        .as_str()
        .expect("error should be present")
        .contains("update this package's metadata"));

    let (status, detail_after) =
        get_package_detail(&app, Some(&alice_jwt), "npm", "release-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail_after["description"], Value::Null);
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_package_detail_surfaces_team_access_only_to_org_members(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    register_user(&app, "charlie", "charlie@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;
    let charlie_jwt = login_user(&app, "charlie", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &alice_jwt, "Acme Org", "acme-org").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id");

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "Acme Packages",
        "acme-packages",
        Some(org_id),
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, package_body) =
        create_package(&app, &alice_jwt, "npm", "acme-widget", "acme-packages").await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );

    let (status, _) = add_org_member(&app, &alice_jwt, "acme-org", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_team(
        &app,
        &alice_jwt,
        "acme-org",
        "Release Engineering",
        "release-engineering",
        Some("Publishes and transfers package releases."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, grant_body) = grant_team_package_access(
        &app,
        &alice_jwt,
        "acme-org",
        "release-engineering",
        "npm",
        "acme-widget",
        &["publish", "transfer_ownership"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected team package access response: {grant_body}"
    );

    let (status, owner_detail) =
        get_package_detail(&app, Some(&alice_jwt), "npm", "acme-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        owner_detail["team_access"][0]["team_slug"],
        "release-engineering"
    );
    assert_eq!(
        owner_detail["team_access"][0]["team_name"],
        "Release Engineering"
    );
    assert_eq!(
        owner_detail["team_access"][0]["permissions"],
        json!(["publish", "transfer_ownership"])
    );

    let (status, member_detail) =
        get_package_detail(&app, Some(&bob_jwt), "npm", "acme-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(member_detail["team_access"], owner_detail["team_access"]);

    let (status, anonymous_detail) = get_package_detail(&app, None, "npm", "acme-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(anonymous_detail["team_access"], Value::Null);

    let (status, unrelated_detail) =
        get_package_detail(&app, Some(&charlie_jwt), "npm", "acme-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(unrelated_detail["team_access"], Value::Null);
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_private_org_member_directory_hides_package_team_access_from_non_admin_members(
    pool: PgPool,
) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &alice_jwt, "Acme Org", "acme-org").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id");

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "Acme Packages",
        "acme-packages",
        Some(org_id),
        Some("private"),
        Some("private"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, package_body) =
        create_package(&app, &alice_jwt, "npm", "acme-widget", "acme-packages").await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );

    let (status, _) = add_org_member(&app, &alice_jwt, "acme-org", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_team(
        &app,
        &alice_jwt,
        "acme-org",
        "Release Engineering",
        "release-engineering",
        Some("Publishes package releases."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, grant_body) = grant_team_package_access(
        &app,
        &alice_jwt,
        "acme-org",
        "release-engineering",
        "npm",
        "acme-widget",
        &["publish"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected team package access response: {grant_body}"
    );

    let (status, member_detail_before) =
        get_package_detail(&app, Some(&bob_jwt), "npm", "acme-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        member_detail_before["team_access"][0]["team_slug"],
        "release-engineering"
    );

    let (status, org_update_body) = update_org_profile(
        &app,
        &alice_jwt,
        "acme-org",
        json!({ "member_directory_is_private": true }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected org update response: {org_update_body}"
    );
    assert_eq!(org_update_body["member_directory_is_private"], true);

    let (status, member_detail_after) =
        get_package_detail(&app, Some(&bob_jwt), "npm", "acme-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(member_detail_after["team_access"], Value::Null);

    let (status, owner_detail_after) =
        get_package_detail(&app, Some(&alice_jwt), "npm", "acme-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        owner_detail_after["team_access"][0]["team_slug"],
        "release-engineering"
    );
    assert_eq!(
        owner_detail_after["team_access"][0]["permissions"],
        json!(["publish"])
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_package_detail_team_access_updates_after_grant_revoke(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &alice_jwt, "Acme Org", "acme-org").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id");

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "Acme Packages",
        "acme-packages",
        Some(org_id),
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, package_body) =
        create_package(&app, &alice_jwt, "npm", "managed-widget", "acme-packages").await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );

    let (status, _) = add_org_member(&app, &alice_jwt, "acme-org", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_team(
        &app,
        &alice_jwt,
        "acme-org",
        "Release Engineering",
        "release-engineering",
        Some("Publishes releases."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, detail_before_grant) =
        get_package_detail(&app, Some(&alice_jwt), "npm", "managed-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail_before_grant["team_access"], json!([]));

    let (status, grant_body) = grant_team_package_access(
        &app,
        &alice_jwt,
        "acme-org",
        "release-engineering",
        "npm",
        "managed-widget",
        &["publish", "write_metadata"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected team package access response: {grant_body}"
    );

    let (status, detail_after_grant) =
        get_package_detail(&app, Some(&alice_jwt), "npm", "managed-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        detail_after_grant["team_access"][0]["team_slug"],
        "release-engineering"
    );
    assert_eq!(
        detail_after_grant["team_access"][0]["permissions"],
        json!(["publish", "write_metadata"])
    );

    let (status, member_detail_after_grant) =
        get_package_detail(&app, Some(&bob_jwt), "npm", "managed-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        member_detail_after_grant["team_access"],
        detail_after_grant["team_access"]
    );

    let (status, revoke_body) = remove_team_package_access(
        &app,
        &alice_jwt,
        "acme-org",
        "release-engineering",
        "npm",
        "managed-widget",
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected remove team package access response: {revoke_body}"
    );

    let (status, detail_after_revoke) =
        get_package_detail(&app, Some(&alice_jwt), "npm", "managed-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail_after_revoke["team_access"], json!([]));
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_team_write_metadata_permission_allows_package_updates_but_not_release_creation(
    pool: PgPool,
) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, source_org_body) = create_org(&app, &alice_jwt, "Docs Org", "docs-org").await;
    assert_eq!(status, StatusCode::CREATED);
    let source_org_id = source_org_body["id"].as_str().expect("source org id");

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "Docs Packages",
        "docs-packages",
        Some(source_org_id),
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, package_body) = create_package_with_options(
        &app,
        &alice_jwt,
        "npm",
        "docs-widget",
        "docs-packages",
        Some("private"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );

    let (status, _) = add_org_member(&app, &alice_jwt, "docs-org", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_team(
        &app,
        &alice_jwt,
        "docs-org",
        "Metadata Editors",
        "metadata-editors",
        Some("Owns package metadata updates without release authority."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) =
        add_team_member_to_team(&app, &alice_jwt, "docs-org", "metadata-editors", "bob").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, grant_body) = grant_team_package_access(
        &app,
        &alice_jwt,
        "docs-org",
        "metadata-editors",
        "npm",
        "docs-widget",
        &["write_metadata"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected team package access response: {grant_body}"
    );

    let (status, update_body) = update_package_metadata(
        &app,
        &bob_jwt,
        "npm",
        "docs-widget",
        json!({
            "description": "Maintained by the metadata-editors team.",
            "homepage": "https://docs.example.test/widgets/docs-widget",
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected package update response: {update_body}"
    );
    assert_eq!(update_body["message"], "Package updated");

    let (status, detail_after) =
        get_package_detail(&app, Some(&bob_jwt), "npm", "docs-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail_after["can_manage_metadata"], true);
    assert_eq!(
        detail_after["description"],
        "Maintained by the metadata-editors team."
    );
    assert_eq!(
        detail_after["homepage"],
        "https://docs.example.test/widgets/docs-widget"
    );
    assert_eq!(detail_after["can_manage_releases"], false);

    let (status, denied_release_body) =
        create_release_for_package(&app, &bob_jwt, "npm", "docs-widget", "2.0.0").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(denied_release_body["error"]
        .as_str()
        .expect("error should be present")
        .contains("publish or mutate releases"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_package_metadata_update_allows_clearing_fields_and_keyword_normalization(
    pool: PgPool,
) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, repository_body) = create_repository(
        &app,
        &alice_jwt,
        "Alice Packages",
        "alice-metadata-packages",
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, package_body) = create_package_with_options(
        &app,
        &alice_jwt,
        "npm",
        "metadata-widget",
        "alice-metadata-packages",
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );

    let (status, update_body) = update_package_metadata(
        &app,
        &alice_jwt,
        "npm",
        "metadata-widget",
        json!({
            "description": "  A metadata editing surface for package settings.  ",
            "homepage": " https://packages.example.test/metadata-widget ",
            "repository_url": " https://github.com/acme/metadata-widget ",
            "license": " MIT ",
            "keywords": [" docs ", "API", "docs", "", "api"],
            "readme": "# Metadata Widget\n\nUpdated package readme.\n",
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected package update response: {update_body}"
    );

    let (status, detail_after_update) =
        get_package_detail(&app, Some(&alice_jwt), "npm", "metadata-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail_after_update["can_manage_metadata"], true);
    assert_eq!(
        detail_after_update["description"],
        "A metadata editing surface for package settings."
    );
    assert_eq!(
        detail_after_update["homepage"],
        "https://packages.example.test/metadata-widget"
    );
    assert_eq!(
        detail_after_update["repository_url"],
        "https://github.com/acme/metadata-widget"
    );
    assert_eq!(detail_after_update["license"], "MIT");
    assert_eq!(detail_after_update["keywords"], json!(["docs", "API"]));
    assert_eq!(
        detail_after_update["readme"],
        "# Metadata Widget\n\nUpdated package readme.\n"
    );

    let (status, clear_body) = update_package_metadata(
        &app,
        &alice_jwt,
        "npm",
        "metadata-widget",
        json!({
            "description": null,
            "homepage": "   ",
            "repository_url": null,
            "license": "\t",
            "keywords": null,
            "readme": "   ",
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected package clear response: {clear_body}"
    );

    let (status, detail_after_clear) =
        get_package_detail(&app, Some(&alice_jwt), "npm", "metadata-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail_after_clear["description"], Value::Null);
    assert_eq!(detail_after_clear["homepage"], Value::Null);
    assert_eq!(detail_after_clear["repository_url"], Value::Null);
    assert_eq!(detail_after_clear["license"], Value::Null);
    assert_eq!(detail_after_clear["keywords"], json!([]));
    assert_eq!(detail_after_clear["readme"], Value::Null);
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_package_detail_surfaces_metadata_management_capability(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "Alice Packages",
        "alice-capability-packages",
        None,
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, package_body) = create_package_with_options(
        &app,
        &alice_jwt,
        "npm",
        "capability-widget",
        "alice-capability-packages",
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );

    let (status, anonymous_package_detail) =
        get_package_detail(&app, None, "npm", "capability-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(anonymous_package_detail["can_manage_metadata"], false);

    let (status, owner_package_detail) =
        get_package_detail(&app, Some(&alice_jwt), "npm", "capability-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(owner_package_detail["can_manage_metadata"], true);
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_org_audit_includes_package_metadata_updates(pool: PgPool) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &alice_jwt, "Docs Org", "docs-org").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id should be returned");

    let (status, repository_body) = create_repository(
        &app,
        &alice_jwt,
        "Docs Packages",
        "docs-audit-packages",
        Some(org_id),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, package_body) = create_package_with_options(
        &app,
        &alice_jwt,
        "npm",
        "audit-widget",
        "docs-audit-packages",
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );
    let package_id = package_body["id"]
        .as_str()
        .expect("package id should be returned")
        .to_owned();

    let (status, update_body) = update_package_metadata(
        &app,
        &alice_jwt,
        "npm",
        "audit-widget",
        json!({
            "description": "Organization-managed package metadata.",
            "homepage": "https://packages.example.test/audit-widget",
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected package update response: {update_body}"
    );

    let (status, audit_body) = list_org_audit(
        &app,
        &alice_jwt,
        "docs-org",
        Some("action=package_update&per_page=10"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let logs = audit_body["logs"]
        .as_array()
        .expect("logs response should be an array");
    assert_eq!(logs.len(), 1, "response: {audit_body}");
    assert_eq!(logs[0]["action"], "package_update");
    assert_eq!(logs[0]["actor_username"], "alice");
    assert_eq!(logs[0]["target_org_id"], org_id);
    assert_eq!(logs[0]["target_package_id"], package_id);
    assert_eq!(logs[0]["metadata"]["ecosystem"], "npm");
    assert_eq!(logs[0]["metadata"]["package_name"], "audit-widget");
    assert_eq!(
        logs[0]["metadata"]["changed_fields"],
        json!(["description", "homepage"])
    );
    assert_eq!(
        logs[0]["metadata"]["changes"]["description"]["before"],
        Value::Null
    );
    assert_eq!(
        logs[0]["metadata"]["changes"]["description"]["after"],
        "Organization-managed package metadata."
    );
    assert_eq!(
        logs[0]["metadata"]["changes"]["homepage"]["before"],
        Value::Null
    );
    assert_eq!(
        logs[0]["metadata"]["changes"]["homepage"]["after"],
        "https://packages.example.test/audit-widget"
    );

    let audit_row = sqlx::query(
        "SELECT metadata \
         FROM audit_logs \
         WHERE action = 'package_update'::audit_action AND target_package_id = $1 \
         ORDER BY occurred_at DESC \
         LIMIT 1",
    )
    .bind(uuid::Uuid::parse_str(&package_id).expect("package id should parse"))
    .fetch_one(&pool)
    .await
    .expect("package update audit row should exist");
    let audit_metadata: Value = audit_row
        .try_get("metadata")
        .expect("audit metadata should be readable");
    assert_eq!(audit_metadata["package_name"], "audit-widget");
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_package_metadata_update_reindexes_search_results(pool: PgPool) {
    if !is_search_backend_available() {
        eprintln!(
            "Skipping search reindex verification because the search backend is unavailable."
        );
        return;
    }

    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "Search Packages",
        "search-packages",
        None,
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, package_body) = create_package_with_options(
        &app,
        &alice_jwt,
        "npm",
        "search-metadata-widget",
        "search-packages",
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );

    let search_token = "metadatareindexalpha";
    let (status, update_body) = update_package_metadata(
        &app,
        &alice_jwt,
        "npm",
        "search-metadata-widget",
        json!({
            "description": format!("{search_token} package metadata reindex verification"),
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected package update response: {update_body}"
    );

    let mut latest_body = Value::Null;
    let mut found = false;
    for _ in 0..30 {
        let (status, search_body) = search_public_packages(&app, search_token, Some("npm")).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "unexpected search response: {search_body}"
        );

        let packages = search_body["packages"]
            .as_array()
            .expect("search packages response should be an array");
        if packages
            .iter()
            .any(|package| package["name"] == "search-metadata-widget")
        {
            latest_body = search_body;
            found = true;
            break;
        }

        latest_body = search_body;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    assert!(
        found,
        "search did not surface updated package metadata: {latest_body}"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_npm_search_respects_authenticated_visibility_and_filtered_offsets(pool: PgPool) {
    if !is_search_backend_available() {
        eprintln!("Skipping npm search visibility verification because the search backend is unavailable.");
        return;
    }

    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &alice_jwt, "Acme Search", "acme-search").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id");

    let (status, _) = add_org_member(&app, &alice_jwt, "acme-search", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    for (name, slug, visibility) in [
        ("Acme Public", "acme-public-search", "public"),
        ("Acme Private", "acme-private-search", "private"),
        ("Acme Internal", "acme-internal-search", "internal_org"),
    ] {
        let (status, body) = create_repository_with_options(
            &app,
            &alice_jwt,
            name,
            slug,
            Some(org_id),
            Some("public"),
            Some(visibility),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "unexpected repository response: {body}"
        );
    }

    for (name, repository_slug, visibility) in [
        (
            "npm-public-search-widget",
            "acme-public-search",
            Some("public"),
        ),
        (
            "npm-private-search-widget",
            "acme-private-search",
            Some("private"),
        ),
        (
            "npm-internal-search-widget",
            "acme-internal-search",
            Some("internal_org"),
        ),
        (
            "npm-unlisted-search-widget",
            "acme-public-search",
            Some("unlisted"),
        ),
        (
            "npm-quarantined-search-widget",
            "acme-public-search",
            Some("quarantined"),
        ),
    ] {
        let (status, body) =
            create_package_with_options(&app, &alice_jwt, "npm", name, repository_slug, visibility)
                .await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "unexpected package response: {body}"
        );
    }

    let search_token = "npmauthsearchbeta";
    for (name, descriptor) in [
        ("npm-public-search-widget", "public"),
        ("npm-private-search-widget", "private"),
        ("npm-internal-search-widget", "internal"),
        ("npm-unlisted-search-widget", "unlisted"),
        ("npm-quarantined-search-widget", "quarantined"),
    ] {
        let (status, body) = update_package_metadata(
            &app,
            &alice_jwt,
            "npm",
            name,
            json!({
                "description": format!("{search_token} npm visibility {descriptor}"),
            }),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "unexpected package update response: {body}"
        );
    }

    let expected_member_names = std::collections::BTreeSet::from([
        "npm-public-search-widget".to_owned(),
        "npm-private-search-widget".to_owned(),
        "npm-internal-search-widget".to_owned(),
    ]);
    let expected_anonymous_names =
        std::collections::BTreeSet::from(["npm-public-search-widget".to_owned()]);

    let mut latest_member_body = Value::Null;
    let mut latest_anonymous_body = Value::Null;
    let mut found = false;
    for _ in 0..30 {
        let (member_status, member_body) =
            search_npm_packages(&app, Some(&bob_jwt), search_token, 10, 0).await;
        assert_eq!(
            member_status,
            StatusCode::OK,
            "unexpected authenticated npm search response: {member_body}"
        );
        let (anonymous_status, anonymous_body) =
            search_npm_packages(&app, None, search_token, 10, 0).await;
        assert_eq!(
            anonymous_status,
            StatusCode::OK,
            "unexpected anonymous npm search response: {anonymous_body}"
        );

        let member_names = member_body["objects"]
            .as_array()
            .expect("npm search objects should be an array")
            .iter()
            .filter_map(|item| item["package"]["name"].as_str().map(str::to_owned))
            .collect::<std::collections::BTreeSet<_>>();
        let anonymous_names = anonymous_body["objects"]
            .as_array()
            .expect("npm search objects should be an array")
            .iter()
            .filter_map(|item| item["package"]["name"].as_str().map(str::to_owned))
            .collect::<std::collections::BTreeSet<_>>();

        latest_member_body = member_body;
        latest_anonymous_body = anonymous_body;

        if member_names == expected_member_names
            && anonymous_names == expected_anonymous_names
            && latest_member_body["total"] == 3
            && latest_anonymous_body["total"] == 1
        {
            found = true;
            break;
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    assert!(
        found,
        "npm search did not converge to expected visibility results. member={latest_member_body} anonymous={latest_anonymous_body}"
    );

    let mut paged_names = std::collections::BTreeSet::new();
    for from in 0..3 {
        let (status, body) = search_npm_packages(&app, Some(&bob_jwt), search_token, 1, from).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "unexpected paged npm search response: {body}"
        );
        assert_eq!(body["total"], 3);
        let objects = body["objects"]
            .as_array()
            .expect("npm paged search objects should be an array");
        assert_eq!(
            objects.len(),
            1,
            "unexpected paged objects response: {body}"
        );
        let name = objects[0]["package"]["name"]
            .as_str()
            .expect("paged npm search object should include a package name");
        paged_names.insert(name.to_owned());
    }
    assert_eq!(paged_names, expected_member_names);

    let (status, body) = search_npm_packages(&app, Some(&bob_jwt), search_token, 1, 3).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected exhausted npm search response: {body}"
    );
    assert_eq!(body["total"], 3);
    assert_eq!(
        body["objects"]
            .as_array()
            .expect("exhausted npm search objects should be an array")
            .len(),
        0
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_cargo_search_respects_authenticated_visibility_and_mixed_repository_combinations(
    pool: PgPool,
) {
    if !is_search_backend_available() {
        eprintln!(
            "Skipping Cargo search visibility verification because the search backend is unavailable."
        );
        return;
    }

    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    register_user(&app, "carol", "carol@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;
    let carol_jwt = login_user(&app, "carol", "super_secret_pw!").await;
    let bob_auth = format!("Bearer {bob_jwt}");
    let carol_auth = format!("Bearer {carol_jwt}");

    let (status, org_body) =
        create_org(&app, &alice_jwt, "Acme Cargo Search", "acme-cargo-search").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id");

    let (status, _) = add_org_member(&app, &alice_jwt, "acme-cargo-search", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    for (name, slug, kind, visibility) in [
        (
            "Cargo Public Search",
            "cargo-public-search",
            "public",
            "public",
        ),
        (
            "Cargo Internal Search",
            "cargo-internal-search",
            "private",
            "internal_org",
        ),
        (
            "Cargo Private Search",
            "cargo-private-search",
            "private",
            "private",
        ),
    ] {
        let (status, body) = create_repository_with_options(
            &app,
            &alice_jwt,
            name,
            slug,
            Some(org_id),
            Some(kind),
            Some(visibility),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "unexpected repository response: {body}"
        );
    }

    let (status, token_body) =
        create_personal_access_token(&app, &alice_jwt, "cargo-search", &["packages:write"]).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected token response: {token_body}"
    );
    let cargo_token = token_body["token"]
        .as_str()
        .expect("cargo token should be returned")
        .to_owned();

    let search_token = "cargosearchvisalpha";
    for (name, repository_slug, visibility, descriptor) in [
        (
            "cargo-public-search-widget",
            "cargo-public-search",
            Some("public"),
            "public",
        ),
        (
            "cargo-private-public-search-widget",
            "cargo-public-search",
            Some("private"),
            "private in public repository",
        ),
        (
            "cargo-internal-search-widget",
            "cargo-internal-search",
            Some("internal_org"),
            "internal_org",
        ),
        (
            "cargo-private-internal-search-widget",
            "cargo-internal-search",
            Some("private"),
            "private in internal repository",
        ),
        (
            "cargo-private-repository-search-widget",
            "cargo-private-search",
            Some("private"),
            "private in private repository",
        ),
        (
            "cargo-unlisted-search-widget",
            "cargo-public-search",
            Some("unlisted"),
            "unlisted",
        ),
        (
            "cargo-quarantined-search-widget",
            "cargo-public-search",
            Some("quarantined"),
            "quarantined",
        ),
    ] {
        let (status, body) = create_package_with_options(
            &app,
            &alice_jwt,
            "cargo",
            name,
            repository_slug,
            visibility,
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "unexpected package response: {body}"
        );

        let crate_bytes = format!("{name}-crate");
        let payload = build_cargo_publish_payload(
            json!({
                "name": name,
                "vers": "0.1.0",
                "deps": [],
                "features": {},
                "authors": ["Alice <alice@test.dev>"],
                "description": format!("{search_token} cargo visibility {descriptor}"),
                "license": "MIT"
            }),
            crate_bytes.as_bytes(),
        );

        let (status, publish_body) = publish_cargo_crate(&app, &cargo_token, payload).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "unexpected cargo publish response: {publish_body}"
        );
    }

    let expected_member_names = std::collections::BTreeSet::from([
        "cargo-public-search-widget".to_owned(),
        "cargo-private-public-search-widget".to_owned(),
        "cargo-internal-search-widget".to_owned(),
        "cargo-private-internal-search-widget".to_owned(),
        "cargo-private-repository-search-widget".to_owned(),
    ]);
    let expected_anonymous_names =
        std::collections::BTreeSet::from(["cargo-public-search-widget".to_owned()]);

    let mut latest_management_member = Value::Null;
    let mut latest_management_anonymous = Value::Null;
    let mut latest_management_outsider = Value::Null;
    let mut latest_cargo_member = Value::Null;
    let mut latest_cargo_anonymous = Value::Null;
    let mut latest_cargo_outsider = Value::Null;
    let mut found = false;

    for _ in 0..30 {
        let (management_member_status, management_member_body) = search_packages_with_options(
            &app,
            Some(&bob_jwt),
            search_token,
            SearchPackagesRequestOptions {
                ecosystem: Some("cargo"),
                ..SearchPackagesRequestOptions::default()
            },
        )
        .await;
        assert_eq!(
            management_member_status,
            StatusCode::OK,
            "unexpected member management search response: {management_member_body}"
        );

        let (management_anonymous_status, management_anonymous_body) =
            search_packages_with_options(
                &app,
                None,
                search_token,
                SearchPackagesRequestOptions {
                    ecosystem: Some("cargo"),
                    ..SearchPackagesRequestOptions::default()
                },
            )
            .await;
        assert_eq!(
            management_anonymous_status,
            StatusCode::OK,
            "unexpected anonymous management search response: {management_anonymous_body}"
        );

        let (management_outsider_status, management_outsider_body) = search_packages_with_options(
            &app,
            Some(&carol_jwt),
            search_token,
            SearchPackagesRequestOptions {
                ecosystem: Some("cargo"),
                ..SearchPackagesRequestOptions::default()
            },
        )
        .await;
        assert_eq!(
            management_outsider_status,
            StatusCode::OK,
            "unexpected outsider management search response: {management_outsider_body}"
        );

        let (cargo_member_status, cargo_member_body) =
            search_cargo_crates(&app, Some(bob_auth.as_str()), search_token, 10).await;
        assert_eq!(
            cargo_member_status,
            StatusCode::OK,
            "unexpected member cargo search response: {cargo_member_body}"
        );

        let (cargo_anonymous_status, cargo_anonymous_body) =
            search_cargo_crates(&app, None, search_token, 10).await;
        assert_eq!(
            cargo_anonymous_status,
            StatusCode::OK,
            "unexpected anonymous cargo search response: {cargo_anonymous_body}"
        );

        let (cargo_outsider_status, cargo_outsider_body) =
            search_cargo_crates(&app, Some(carol_auth.as_str()), search_token, 10).await;
        assert_eq!(
            cargo_outsider_status,
            StatusCode::OK,
            "unexpected outsider cargo search response: {cargo_outsider_body}"
        );

        let management_member_names = management_member_body["packages"]
            .as_array()
            .expect("member management packages should be an array")
            .iter()
            .filter_map(|item| item["name"].as_str().map(str::to_owned))
            .collect::<std::collections::BTreeSet<_>>();
        let management_anonymous_names = management_anonymous_body["packages"]
            .as_array()
            .expect("anonymous management packages should be an array")
            .iter()
            .filter_map(|item| item["name"].as_str().map(str::to_owned))
            .collect::<std::collections::BTreeSet<_>>();
        let management_outsider_names = management_outsider_body["packages"]
            .as_array()
            .expect("outsider management packages should be an array")
            .iter()
            .filter_map(|item| item["name"].as_str().map(str::to_owned))
            .collect::<std::collections::BTreeSet<_>>();

        let cargo_member_names = cargo_member_body["crates"]
            .as_array()
            .expect("member cargo crates should be an array")
            .iter()
            .filter_map(|item| item["name"].as_str().map(str::to_owned))
            .collect::<std::collections::BTreeSet<_>>();
        let cargo_anonymous_names = cargo_anonymous_body["crates"]
            .as_array()
            .expect("anonymous cargo crates should be an array")
            .iter()
            .filter_map(|item| item["name"].as_str().map(str::to_owned))
            .collect::<std::collections::BTreeSet<_>>();
        let cargo_outsider_names = cargo_outsider_body["crates"]
            .as_array()
            .expect("outsider cargo crates should be an array")
            .iter()
            .filter_map(|item| item["name"].as_str().map(str::to_owned))
            .collect::<std::collections::BTreeSet<_>>();

        latest_management_member = management_member_body;
        latest_management_anonymous = management_anonymous_body;
        latest_management_outsider = management_outsider_body;
        latest_cargo_member = cargo_member_body;
        latest_cargo_anonymous = cargo_anonymous_body;
        latest_cargo_outsider = cargo_outsider_body;

        if management_member_names == expected_member_names
            && management_anonymous_names == expected_anonymous_names
            && management_outsider_names == expected_anonymous_names
            && cargo_member_names == expected_member_names
            && cargo_anonymous_names == expected_anonymous_names
            && cargo_outsider_names == expected_anonymous_names
            && latest_management_member["total"] == 5
            && latest_management_anonymous["total"] == 1
            && latest_management_outsider["total"] == 1
            && latest_cargo_member["meta"]["total"] == 5
            && latest_cargo_anonymous["meta"]["total"] == 1
            && latest_cargo_outsider["meta"]["total"] == 1
        {
            found = true;
            break;
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    assert!(
        found,
        "Cargo search did not converge to expected visibility results.\nmanagement member={latest_management_member}\nmanagement anonymous={latest_management_anonymous}\nmanagement outsider={latest_management_outsider}\ncargo member={latest_cargo_member}\ncargo anonymous={latest_cargo_anonymous}\ncargo outsider={latest_cargo_outsider}"
    );

    let (status, body) = search_cargo_crates(&app, Some(bob_auth.as_str()), search_token, 2).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected paged Cargo search response: {body}"
    );
    assert_eq!(body["meta"]["total"], 5);
    assert_eq!(
        body["crates"]
            .as_array()
            .expect("paged cargo crates should be an array")
            .len(),
        2
    );
    for item in body["crates"]
        .as_array()
        .expect("paged cargo crates should be an array")
    {
        assert_eq!(item["max_version"], "0.1.0");
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_search_surfaces_include_private_packages_visible_through_team_grants(pool: PgPool) {
    if !is_search_backend_available() {
        eprintln!(
            "Skipping delegated search visibility verification because the search backend is unavailable."
        );
        return;
    }

    let app = app(pool);
    register_user(
        &app,
        "alice",
        "alice-delegated-search@test.dev",
        "super_secret_pw!",
    )
    .await;
    register_user(
        &app,
        "bob",
        "bob-delegated-search@test.dev",
        "super_secret_pw!",
    )
    .await;
    register_user(
        &app,
        "carol",
        "carol-delegated-search@test.dev",
        "super_secret_pw!",
    )
    .await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;
    let carol_jwt = login_user(&app, "carol", "super_secret_pw!").await;

    let (status, org_body) = create_org(
        &app,
        &alice_jwt,
        "Delegated Search Org",
        "delegated-search-org",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id should be present");

    let (status, _) =
        add_org_member(&app, &alice_jwt, "delegated-search-org", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) =
        add_org_member(&app, &alice_jwt, "delegated-search-org", "carol", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    for (name, slug, visibility) in [
        ("Delegated Public", "delegated-public-search", "public"),
        (
            "Delegated Package Private",
            "delegated-package-private",
            "private",
        ),
        (
            "Delegated Repository Private",
            "delegated-repository-private",
            "private",
        ),
    ] {
        let (status, body) = create_repository_with_options(
            &app,
            &alice_jwt,
            name,
            slug,
            Some(org_id),
            Some("public"),
            Some(visibility),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "unexpected repository response: {body}"
        );
    }

    for (name, repository_slug, visibility, descriptor) in [
        (
            "delegated-public-search-widget",
            "delegated-public-search",
            Some("public"),
            "public",
        ),
        (
            "delegated-package-search-widget",
            "delegated-package-private",
            Some("private"),
            "package-granted",
        ),
        (
            "delegated-repository-search-widget",
            "delegated-repository-private",
            Some("private"),
            "repository-granted",
        ),
    ] {
        let (status, body) =
            create_package_with_options(&app, &alice_jwt, "npm", name, repository_slug, visibility)
                .await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "unexpected package response: {body}"
        );

        let (status, body) = update_package_metadata(
            &app,
            &alice_jwt,
            "npm",
            name,
            json!({
                "description": format!("delegatedsearchomega {descriptor}"),
            }),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "unexpected package update response: {body}"
        );
    }

    let (status, _) = create_team(
        &app,
        &alice_jwt,
        "delegated-search-org",
        "Package Readers",
        "package-readers",
        Some("Can search private packages granted directly."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = create_team(
        &app,
        &alice_jwt,
        "delegated-search-org",
        "Repository Readers",
        "repository-readers",
        Some("Can search private packages granted via repository read access."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = add_team_member_to_team(
        &app,
        &alice_jwt,
        "delegated-search-org",
        "package-readers",
        "bob",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = add_team_member_to_team(
        &app,
        &alice_jwt,
        "delegated-search-org",
        "repository-readers",
        "carol",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, grant_body) = grant_team_package_access(
        &app,
        &alice_jwt,
        "delegated-search-org",
        "package-readers",
        "npm",
        "delegated-package-search-widget",
        &["read_private"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected team package access response: {grant_body}"
    );

    let (status, grant_body) = grant_team_repository_access(
        &app,
        &alice_jwt,
        "delegated-search-org",
        "repository-readers",
        "delegated-repository-private",
        &["read_private"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected team repository access response: {grant_body}"
    );

    let expected_anonymous_names =
        std::collections::BTreeSet::from(["delegated-public-search-widget".to_owned()]);
    let expected_bob_names = std::collections::BTreeSet::from([
        "delegated-public-search-widget".to_owned(),
        "delegated-package-search-widget".to_owned(),
    ]);
    let expected_carol_names = std::collections::BTreeSet::from([
        "delegated-public-search-widget".to_owned(),
        "delegated-repository-search-widget".to_owned(),
    ]);

    let mut latest_management_bob = Value::Null;
    let mut latest_management_carol = Value::Null;
    let mut latest_management_anonymous = Value::Null;
    let mut latest_npm_bob = Value::Null;
    let mut latest_npm_carol = Value::Null;
    let mut latest_npm_anonymous = Value::Null;
    let mut found = false;

    for _ in 0..30 {
        let (management_bob_status, management_bob_body) = search_packages_with_options(
            &app,
            Some(&bob_jwt),
            "delegatedsearchomega",
            SearchPackagesRequestOptions {
                ecosystem: Some("npm"),
                ..SearchPackagesRequestOptions::default()
            },
        )
        .await;
        assert_eq!(
            management_bob_status,
            StatusCode::OK,
            "unexpected bob management search response: {management_bob_body}"
        );

        let (management_carol_status, management_carol_body) = search_packages_with_options(
            &app,
            Some(&carol_jwt),
            "delegatedsearchomega",
            SearchPackagesRequestOptions {
                ecosystem: Some("npm"),
                ..SearchPackagesRequestOptions::default()
            },
        )
        .await;
        assert_eq!(
            management_carol_status,
            StatusCode::OK,
            "unexpected carol management search response: {management_carol_body}"
        );

        let (management_anonymous_status, management_anonymous_body) =
            search_packages_with_options(
                &app,
                None,
                "delegatedsearchomega",
                SearchPackagesRequestOptions {
                    ecosystem: Some("npm"),
                    ..SearchPackagesRequestOptions::default()
                },
            )
            .await;
        assert_eq!(
            management_anonymous_status,
            StatusCode::OK,
            "unexpected anonymous management search response: {management_anonymous_body}"
        );

        let (npm_bob_status, npm_bob_body) =
            search_npm_packages(&app, Some(&bob_jwt), "delegatedsearchomega", 10, 0).await;
        assert_eq!(
            npm_bob_status,
            StatusCode::OK,
            "unexpected bob npm search response: {npm_bob_body}"
        );

        let (npm_carol_status, npm_carol_body) =
            search_npm_packages(&app, Some(&carol_jwt), "delegatedsearchomega", 10, 0).await;
        assert_eq!(
            npm_carol_status,
            StatusCode::OK,
            "unexpected carol npm search response: {npm_carol_body}"
        );

        let (npm_anonymous_status, npm_anonymous_body) =
            search_npm_packages(&app, None, "delegatedsearchomega", 10, 0).await;
        assert_eq!(
            npm_anonymous_status,
            StatusCode::OK,
            "unexpected anonymous npm search response: {npm_anonymous_body}"
        );

        let management_bob_names = management_bob_body["packages"]
            .as_array()
            .expect("bob management packages should be an array")
            .iter()
            .filter_map(|item| item["name"].as_str().map(str::to_owned))
            .collect::<std::collections::BTreeSet<_>>();
        let management_carol_names = management_carol_body["packages"]
            .as_array()
            .expect("carol management packages should be an array")
            .iter()
            .filter_map(|item| item["name"].as_str().map(str::to_owned))
            .collect::<std::collections::BTreeSet<_>>();
        let management_anonymous_names = management_anonymous_body["packages"]
            .as_array()
            .expect("anonymous management packages should be an array")
            .iter()
            .filter_map(|item| item["name"].as_str().map(str::to_owned))
            .collect::<std::collections::BTreeSet<_>>();

        let npm_bob_names = npm_bob_body["objects"]
            .as_array()
            .expect("bob npm search objects should be an array")
            .iter()
            .filter_map(|item| item["package"]["name"].as_str().map(str::to_owned))
            .collect::<std::collections::BTreeSet<_>>();
        let npm_carol_names = npm_carol_body["objects"]
            .as_array()
            .expect("carol npm search objects should be an array")
            .iter()
            .filter_map(|item| item["package"]["name"].as_str().map(str::to_owned))
            .collect::<std::collections::BTreeSet<_>>();
        let npm_anonymous_names = npm_anonymous_body["objects"]
            .as_array()
            .expect("anonymous npm search objects should be an array")
            .iter()
            .filter_map(|item| item["package"]["name"].as_str().map(str::to_owned))
            .collect::<std::collections::BTreeSet<_>>();

        latest_management_bob = management_bob_body;
        latest_management_carol = management_carol_body;
        latest_management_anonymous = management_anonymous_body;
        latest_npm_bob = npm_bob_body;
        latest_npm_carol = npm_carol_body;
        latest_npm_anonymous = npm_anonymous_body;

        if management_bob_names == expected_bob_names
            && management_carol_names == expected_carol_names
            && management_anonymous_names == expected_anonymous_names
            && npm_bob_names == expected_bob_names
            && npm_carol_names == expected_carol_names
            && npm_anonymous_names == expected_anonymous_names
        {
            found = true;
            break;
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    assert!(
        found,
        "delegated search visibility did not converge.\nmanagement bob: {latest_management_bob}\nmanagement carol: {latest_management_carol}\nmanagement anonymous: {latest_management_anonymous}\nnpm bob: {latest_npm_bob}\nnpm carol: {latest_npm_carol}\nnpm anonymous: {latest_npm_anonymous}"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_cargo_search_surfaces_private_crates_visible_through_team_grants(pool: PgPool) {
    if !is_search_backend_available() {
        eprintln!(
            "Skipping delegated Cargo search visibility verification because the search backend is unavailable."
        );
        return;
    }

    let app = app(pool);
    register_user(
        &app,
        "alice",
        "alice-delegated-cargo-search@test.dev",
        "super_secret_pw!",
    )
    .await;
    register_user(
        &app,
        "bob",
        "bob-delegated-cargo-search@test.dev",
        "super_secret_pw!",
    )
    .await;
    register_user(
        &app,
        "carol",
        "carol-delegated-cargo-search@test.dev",
        "super_secret_pw!",
    )
    .await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;
    let carol_jwt = login_user(&app, "carol", "super_secret_pw!").await;
    let bob_auth = format!("Bearer {bob_jwt}");
    let carol_auth = format!("Bearer {carol_jwt}");

    let (status, org_body) = create_org(
        &app,
        &alice_jwt,
        "Delegated Cargo Search Org",
        "delegated-cargo-search-org",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id should be present");

    let (status, _) = add_org_member(
        &app,
        &alice_jwt,
        "delegated-cargo-search-org",
        "bob",
        "viewer",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = add_org_member(
        &app,
        &alice_jwt,
        "delegated-cargo-search-org",
        "carol",
        "viewer",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    for (name, slug, visibility) in [
        (
            "Delegated Cargo Public",
            "delegated-cargo-public-search",
            "public",
        ),
        (
            "Delegated Cargo Package Private",
            "delegated-cargo-package-private",
            "private",
        ),
        (
            "Delegated Cargo Repository Private",
            "delegated-cargo-repository-private",
            "private",
        ),
    ] {
        let (status, body) = create_repository_with_options(
            &app,
            &alice_jwt,
            name,
            slug,
            Some(org_id),
            Some("public"),
            Some(visibility),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "unexpected repository response: {body}"
        );
    }

    let (status, token_body) = create_personal_access_token(
        &app,
        &alice_jwt,
        "delegated-cargo-search",
        &["packages:write"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected token response: {token_body}"
    );
    let cargo_token = token_body["token"]
        .as_str()
        .expect("cargo token should be returned")
        .to_owned();

    let search_token = "delegatedcargosearchomega";
    for (name, repository_slug, visibility, descriptor) in [
        (
            "delegated-cargo-public-search-widget",
            "delegated-cargo-public-search",
            Some("public"),
            "public",
        ),
        (
            "delegated-cargo-package-search-widget",
            "delegated-cargo-package-private",
            Some("private"),
            "package-granted",
        ),
        (
            "delegated-cargo-repository-search-widget",
            "delegated-cargo-repository-private",
            Some("private"),
            "repository-granted",
        ),
    ] {
        let (status, body) = create_package_with_options(
            &app,
            &alice_jwt,
            "cargo",
            name,
            repository_slug,
            visibility,
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "unexpected package response: {body}"
        );

        let crate_bytes = format!("{name}-crate");
        let payload = build_cargo_publish_payload(
            json!({
                "name": name,
                "vers": "0.1.0",
                "deps": [],
                "features": {},
                "authors": ["Alice <alice@test.dev>"],
                "description": format!("{search_token} {descriptor}"),
                "license": "MIT"
            }),
            crate_bytes.as_bytes(),
        );

        let (status, publish_body) = publish_cargo_crate(&app, &cargo_token, payload).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "unexpected cargo publish response: {publish_body}"
        );
    }

    let (status, _) = create_team(
        &app,
        &alice_jwt,
        "delegated-cargo-search-org",
        "Cargo Package Readers",
        "cargo-package-readers",
        Some("Can search private crates granted directly."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = create_team(
        &app,
        &alice_jwt,
        "delegated-cargo-search-org",
        "Cargo Repository Readers",
        "cargo-repository-readers",
        Some("Can search private crates granted via repository access."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = add_team_member_to_team(
        &app,
        &alice_jwt,
        "delegated-cargo-search-org",
        "cargo-package-readers",
        "bob",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = add_team_member_to_team(
        &app,
        &alice_jwt,
        "delegated-cargo-search-org",
        "cargo-repository-readers",
        "carol",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, grant_body) = grant_team_package_access(
        &app,
        &alice_jwt,
        "delegated-cargo-search-org",
        "cargo-package-readers",
        "cargo",
        "delegated-cargo-package-search-widget",
        &["read_private"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected team package access response: {grant_body}"
    );

    let (status, grant_body) = grant_team_repository_access(
        &app,
        &alice_jwt,
        "delegated-cargo-search-org",
        "cargo-repository-readers",
        "delegated-cargo-repository-private",
        &["read_private"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected team repository access response: {grant_body}"
    );

    let expected_anonymous_names =
        std::collections::BTreeSet::from(["delegated-cargo-public-search-widget".to_owned()]);
    let expected_bob_names = std::collections::BTreeSet::from([
        "delegated-cargo-public-search-widget".to_owned(),
        "delegated-cargo-package-search-widget".to_owned(),
    ]);
    let expected_carol_names = std::collections::BTreeSet::from([
        "delegated-cargo-public-search-widget".to_owned(),
        "delegated-cargo-repository-search-widget".to_owned(),
    ]);

    let mut latest_management_bob = Value::Null;
    let mut latest_management_carol = Value::Null;
    let mut latest_management_anonymous = Value::Null;
    let mut latest_cargo_bob = Value::Null;
    let mut latest_cargo_carol = Value::Null;
    let mut latest_cargo_anonymous = Value::Null;
    let mut found = false;

    for _ in 0..30 {
        let (management_bob_status, management_bob_body) = search_packages_with_options(
            &app,
            Some(&bob_jwt),
            search_token,
            SearchPackagesRequestOptions {
                ecosystem: Some("cargo"),
                ..SearchPackagesRequestOptions::default()
            },
        )
        .await;
        assert_eq!(
            management_bob_status,
            StatusCode::OK,
            "unexpected bob management search response: {management_bob_body}"
        );

        let (management_carol_status, management_carol_body) = search_packages_with_options(
            &app,
            Some(&carol_jwt),
            search_token,
            SearchPackagesRequestOptions {
                ecosystem: Some("cargo"),
                ..SearchPackagesRequestOptions::default()
            },
        )
        .await;
        assert_eq!(
            management_carol_status,
            StatusCode::OK,
            "unexpected carol management search response: {management_carol_body}"
        );

        let (management_anonymous_status, management_anonymous_body) =
            search_packages_with_options(
                &app,
                None,
                search_token,
                SearchPackagesRequestOptions {
                    ecosystem: Some("cargo"),
                    ..SearchPackagesRequestOptions::default()
                },
            )
            .await;
        assert_eq!(
            management_anonymous_status,
            StatusCode::OK,
            "unexpected anonymous management search response: {management_anonymous_body}"
        );

        let (cargo_bob_status, cargo_bob_body) =
            search_cargo_crates(&app, Some(bob_auth.as_str()), search_token, 10).await;
        assert_eq!(
            cargo_bob_status,
            StatusCode::OK,
            "unexpected bob cargo search response: {cargo_bob_body}"
        );

        let (cargo_carol_status, cargo_carol_body) =
            search_cargo_crates(&app, Some(carol_auth.as_str()), search_token, 10).await;
        assert_eq!(
            cargo_carol_status,
            StatusCode::OK,
            "unexpected carol cargo search response: {cargo_carol_body}"
        );

        let (cargo_anonymous_status, cargo_anonymous_body) =
            search_cargo_crates(&app, None, search_token, 10).await;
        assert_eq!(
            cargo_anonymous_status,
            StatusCode::OK,
            "unexpected anonymous cargo search response: {cargo_anonymous_body}"
        );

        let management_bob_names = management_bob_body["packages"]
            .as_array()
            .expect("bob management packages should be an array")
            .iter()
            .filter_map(|item| item["name"].as_str().map(str::to_owned))
            .collect::<std::collections::BTreeSet<_>>();
        let management_carol_names = management_carol_body["packages"]
            .as_array()
            .expect("carol management packages should be an array")
            .iter()
            .filter_map(|item| item["name"].as_str().map(str::to_owned))
            .collect::<std::collections::BTreeSet<_>>();
        let management_anonymous_names = management_anonymous_body["packages"]
            .as_array()
            .expect("anonymous management packages should be an array")
            .iter()
            .filter_map(|item| item["name"].as_str().map(str::to_owned))
            .collect::<std::collections::BTreeSet<_>>();

        let cargo_bob_names = cargo_bob_body["crates"]
            .as_array()
            .expect("bob cargo crates should be an array")
            .iter()
            .filter_map(|item| item["name"].as_str().map(str::to_owned))
            .collect::<std::collections::BTreeSet<_>>();
        let cargo_carol_names = cargo_carol_body["crates"]
            .as_array()
            .expect("carol cargo crates should be an array")
            .iter()
            .filter_map(|item| item["name"].as_str().map(str::to_owned))
            .collect::<std::collections::BTreeSet<_>>();
        let cargo_anonymous_names = cargo_anonymous_body["crates"]
            .as_array()
            .expect("anonymous cargo crates should be an array")
            .iter()
            .filter_map(|item| item["name"].as_str().map(str::to_owned))
            .collect::<std::collections::BTreeSet<_>>();

        latest_management_bob = management_bob_body;
        latest_management_carol = management_carol_body;
        latest_management_anonymous = management_anonymous_body;
        latest_cargo_bob = cargo_bob_body;
        latest_cargo_carol = cargo_carol_body;
        latest_cargo_anonymous = cargo_anonymous_body;

        if management_bob_names == expected_bob_names
            && management_carol_names == expected_carol_names
            && management_anonymous_names == expected_anonymous_names
            && cargo_bob_names == expected_bob_names
            && cargo_carol_names == expected_carol_names
            && cargo_anonymous_names == expected_anonymous_names
            && latest_management_bob["total"] == 2
            && latest_management_carol["total"] == 2
            && latest_management_anonymous["total"] == 1
            && latest_cargo_bob["meta"]["total"] == 2
            && latest_cargo_carol["meta"]["total"] == 2
            && latest_cargo_anonymous["meta"]["total"] == 1
        {
            found = true;
            break;
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    assert!(
        found,
        "Delegated Cargo search did not converge.\nmanagement bob={latest_management_bob}\nmanagement carol={latest_management_carol}\nmanagement anonymous={latest_management_anonymous}\ncargo bob={latest_cargo_bob}\ncargo carol={latest_cargo_carol}\ncargo anonymous={latest_cargo_anonymous}"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_management_search_can_scope_results_to_one_org(pool: PgPool) {
    if !is_search_backend_available() {
        eprintln!("Skipping management search org filter verification because the search backend is unavailable.");
        return;
    }

    let app = app(pool);
    register_user(
        &app,
        "alice",
        "alice-org-search@test.dev",
        "super_secret_pw!",
    )
    .await;
    register_user(&app, "bob", "bob-org-search@test.dev", "super_secret_pw!").await;
    register_user(
        &app,
        "carol",
        "carol-org-search@test.dev",
        "super_secret_pw!",
    )
    .await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;
    let carol_jwt = login_user(&app, "carol", "super_secret_pw!").await;

    let (status, acme_body) =
        create_org(&app, &alice_jwt, "Acme Org Search", "acme-org-search").await;
    assert_eq!(status, StatusCode::CREATED);
    let acme_org_id = acme_body["id"].as_str().expect("acme org id");

    let (status, beta_body) =
        create_org(&app, &carol_jwt, "Beta Org Search", "beta-org-search").await;
    assert_eq!(status, StatusCode::CREATED);
    let beta_org_id = beta_body["id"].as_str().expect("beta org id");

    let (status, body) = add_org_member(&app, &alice_jwt, "acme-org-search", "bob", "viewer").await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected org membership response: {body}"
    );

    for (name, slug, owner_org_id, visibility) in [
        (
            "Acme Public Search",
            "acme-public-org-search",
            acme_org_id,
            "public",
        ),
        (
            "Acme Private Search",
            "acme-private-org-search",
            acme_org_id,
            "private",
        ),
        (
            "Beta Public Search",
            "beta-public-org-search",
            beta_org_id,
            "public",
        ),
    ] {
        let (status, body) = create_repository_with_options(
            &app,
            if owner_org_id == acme_org_id {
                &alice_jwt
            } else {
                &carol_jwt
            },
            name,
            slug,
            Some(owner_org_id),
            Some("public"),
            Some(visibility),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "unexpected repository response: {body}"
        );
    }

    for (jwt, package_name, repository_slug, visibility, descriptor) in [
        (
            &alice_jwt,
            "acme-public-org-search-widget",
            "acme-public-org-search",
            Some("public"),
            "acme public",
        ),
        (
            &alice_jwt,
            "acme-private-org-search-widget",
            "acme-private-org-search",
            Some("private"),
            "acme private",
        ),
        (
            &carol_jwt,
            "beta-public-org-search-widget",
            "beta-public-org-search",
            Some("public"),
            "beta public",
        ),
    ] {
        let (status, body) = create_package_with_options(
            &app,
            jwt,
            "npm",
            package_name,
            repository_slug,
            visibility,
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "unexpected package response: {body}"
        );

        let (status, body) = update_package_metadata(
            &app,
            jwt,
            "npm",
            package_name,
            json!({
                "description": format!("orgscopedsearchgamma {descriptor}"),
            }),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "unexpected package update response: {body}"
        );
    }

    let expected_member_names = std::collections::BTreeSet::from([
        "acme-public-org-search-widget".to_owned(),
        "acme-private-org-search-widget".to_owned(),
    ]);
    let expected_anonymous_names =
        std::collections::BTreeSet::from(["acme-public-org-search-widget".to_owned()]);

    let mut latest_member_body = Value::Null;
    let mut latest_anonymous_body = Value::Null;
    let mut found = false;
    for _ in 0..30 {
        let (member_status, member_body) = search_packages_with_options(
            &app,
            Some(&bob_jwt),
            "orgscopedsearchgamma",
            SearchPackagesRequestOptions {
                ecosystem: Some("npm"),
                org: Some("acme-org-search"),
                ..SearchPackagesRequestOptions::default()
            },
        )
        .await;
        assert_eq!(
            member_status,
            StatusCode::OK,
            "unexpected member search response: {member_body}"
        );

        let (anonymous_status, anonymous_body) = search_packages_with_options(
            &app,
            None,
            "orgscopedsearchgamma",
            SearchPackagesRequestOptions {
                ecosystem: Some("npm"),
                org: Some("acme-org-search"),
                ..SearchPackagesRequestOptions::default()
            },
        )
        .await;
        assert_eq!(
            anonymous_status,
            StatusCode::OK,
            "unexpected anonymous search response: {anonymous_body}"
        );

        let member_names = member_body["packages"]
            .as_array()
            .expect("member packages should be an array")
            .iter()
            .filter_map(|item| item["name"].as_str().map(str::to_owned))
            .collect::<std::collections::BTreeSet<_>>();
        let anonymous_names = anonymous_body["packages"]
            .as_array()
            .expect("anonymous packages should be an array")
            .iter()
            .filter_map(|item| item["name"].as_str().map(str::to_owned))
            .collect::<std::collections::BTreeSet<_>>();

        latest_member_body = member_body;
        latest_anonymous_body = anonymous_body;
        if member_names == expected_member_names && anonymous_names == expected_anonymous_names {
            found = true;
            break;
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    assert!(
        found,
        "org-scoped search did not converge.\nmember: {latest_member_body}\nanonymous: {latest_anonymous_body}"
    );

    let (outsider_status, outsider_body) = search_packages_with_options(
        &app,
        Some(&carol_jwt),
        "orgscopedsearchgamma",
        SearchPackagesRequestOptions {
            ecosystem: Some("npm"),
            org: Some("acme-org-search"),
            ..SearchPackagesRequestOptions::default()
        },
    )
    .await;
    assert_eq!(
        outsider_status,
        StatusCode::OK,
        "unexpected outsider search response: {outsider_body}"
    );
    let outsider_names = outsider_body["packages"]
        .as_array()
        .expect("outsider packages should be an array")
        .iter()
        .filter_map(|item| item["name"].as_str().map(str::to_owned))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(outsider_names, expected_anonymous_names);
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_management_search_can_scope_results_to_one_repository(pool: PgPool) {
    if !is_search_backend_available() {
        eprintln!(
            "Skipping management search repository filter verification because the search backend is unavailable."
        );
        return;
    }

    let app = app(pool);
    register_user(
        &app,
        "alice",
        "alice-repository-search@test.dev",
        "super_secret_pw!",
    )
    .await;
    register_user(
        &app,
        "bob",
        "bob-repository-search@test.dev",
        "super_secret_pw!",
    )
    .await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, org_body) = create_org(
        &app,
        &alice_jwt,
        "Acme Repository Search",
        "acme-repository-search",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id");

    let (status, body) =
        add_org_member(&app, &alice_jwt, "acme-repository-search", "bob", "viewer").await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected org membership response: {body}"
    );

    for (name, slug, visibility) in [
        ("Release Packages", "release-packages", "public"),
        ("Private Packages", "private-packages", "private"),
        ("Public Packages", "public-packages", "public"),
    ] {
        let (status, body) = create_repository_with_options(
            &app,
            &alice_jwt,
            name,
            slug,
            Some(org_id),
            Some("public"),
            Some(visibility),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "unexpected repository response: {body}"
        );
    }

    for (package_name, repository_slug, visibility, descriptor) in [
        (
            "release-repository-search-widget",
            "release-packages",
            Some("public"),
            "release public",
        ),
        (
            "private-repository-search-widget",
            "private-packages",
            Some("private"),
            "private repository",
        ),
        (
            "public-repository-search-widget",
            "public-packages",
            Some("public"),
            "public repository",
        ),
    ] {
        let (status, body) = create_package_with_options(
            &app,
            &alice_jwt,
            "npm",
            package_name,
            repository_slug,
            visibility,
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "unexpected package response: {body}"
        );

        let (status, body) = update_package_metadata(
            &app,
            &alice_jwt,
            "npm",
            package_name,
            json!({
                "description": format!("repositoryscopedsearchdelta {descriptor}"),
            }),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "unexpected package update response: {body}"
        );
    }

    let expected_member_names =
        std::collections::BTreeSet::from(["private-repository-search-widget".to_owned()]);
    let expected_anonymous_names = std::collections::BTreeSet::new();

    let mut latest_member_body = Value::Null;
    let mut latest_anonymous_body = Value::Null;
    let mut found = false;
    for _ in 0..30 {
        let (member_status, member_body) = search_packages_with_options(
            &app,
            Some(&bob_jwt),
            "repositoryscopedsearchdelta",
            SearchPackagesRequestOptions {
                ecosystem: Some("npm"),
                org: Some("acme-repository-search"),
                repository: Some("private-packages"),
                ..SearchPackagesRequestOptions::default()
            },
        )
        .await;
        assert_eq!(
            member_status,
            StatusCode::OK,
            "unexpected member repository search response: {member_body}"
        );

        let (anonymous_status, anonymous_body) = search_packages_with_options(
            &app,
            None,
            "repositoryscopedsearchdelta",
            SearchPackagesRequestOptions {
                ecosystem: Some("npm"),
                org: Some("acme-repository-search"),
                repository: Some("private-packages"),
                ..SearchPackagesRequestOptions::default()
            },
        )
        .await;
        assert_eq!(
            anonymous_status,
            StatusCode::OK,
            "unexpected anonymous repository search response: {anonymous_body}"
        );

        let member_names = member_body["packages"]
            .as_array()
            .expect("member packages should be an array")
            .iter()
            .filter_map(|item| item["name"].as_str().map(str::to_owned))
            .collect::<std::collections::BTreeSet<_>>();
        let anonymous_names = anonymous_body["packages"]
            .as_array()
            .expect("anonymous packages should be an array")
            .iter()
            .filter_map(|item| item["name"].as_str().map(str::to_owned))
            .collect::<std::collections::BTreeSet<_>>();

        latest_member_body = member_body;
        latest_anonymous_body = anonymous_body;
        if member_names == expected_member_names && anonymous_names == expected_anonymous_names {
            found = true;
            break;
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    assert!(
        found,
        "repository-scoped search did not converge.\nmember: {latest_member_body}\nanonymous: {latest_anonymous_body}"
    );

    let member_packages = latest_member_body["packages"]
        .as_array()
        .expect("member packages should be an array");
    assert_eq!(member_packages.len(), 1);
    assert_eq!(member_packages[0]["repository_slug"], "private-packages");
    assert_eq!(member_packages[0]["repository_name"], "Private Packages");

    let (release_status, release_body) = search_packages_with_options(
        &app,
        None,
        "repositoryscopedsearchdelta",
        SearchPackagesRequestOptions {
            ecosystem: Some("npm"),
            org: Some("acme-repository-search"),
            repository: Some("release-packages"),
            ..SearchPackagesRequestOptions::default()
        },
    )
    .await;
    assert_eq!(
        release_status,
        StatusCode::OK,
        "unexpected release repository search response: {release_body}"
    );
    let release_names = release_body["packages"]
        .as_array()
        .expect("release packages should be an array")
        .iter()
        .filter_map(|item| item["name"].as_str().map(str::to_owned))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        release_names,
        std::collections::BTreeSet::from(["release-repository-search-widget".to_owned()])
    );
    let release_packages = release_body["packages"]
        .as_array()
        .expect("release packages should be an array");
    assert_eq!(release_packages.len(), 1);
    assert_eq!(release_packages[0]["repository_slug"], "release-packages");
    assert_eq!(release_packages[0]["repository_name"], "Release Packages");
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_release_detail_surfaces_management_capability_and_visibility(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, repository_body) =
        create_repository(&app, &alice_jwt, "Alice Packages", "alice-packages", None).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, package_body) = create_package_with_options(
        &app,
        &alice_jwt,
        "npm",
        "release-ui-widget",
        "alice-packages",
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );

    let (status, anonymous_package_detail) =
        get_package_detail(&app, None, "npm", "release-ui-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(anonymous_package_detail["can_manage_metadata"], false);
    assert_eq!(anonymous_package_detail["can_manage_releases"], false);

    let (status, owner_package_detail) =
        get_package_detail(&app, Some(&alice_jwt), "npm", "release-ui-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(owner_package_detail["can_manage_metadata"], true);
    assert_eq!(owner_package_detail["can_manage_releases"], true);

    let (status, release_body) = create_release_for_package_with_payload(
        &app,
        &alice_jwt,
        "npm",
        "release-ui-widget",
        json!({
            "version": "1.2.3",
            "description": "First managed release",
            "changelog": "- add lifecycle UI coverage",
            "source_ref": "refs/tags/v1.2.3",
            "is_prerelease": true,
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected release response: {release_body}"
    );
    assert_eq!(release_body["status"], "quarantine");
    assert_eq!(release_body["is_prerelease"], true);

    let (status, owner_release_detail) =
        get_release_detail(&app, Some(&alice_jwt), "npm", "release-ui-widget", "1.2.3").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(owner_release_detail["status"], "quarantine");
    assert_eq!(owner_release_detail["can_manage_releases"], true);
    assert_eq!(owner_release_detail["description"], "First managed release");
    assert_eq!(owner_release_detail["source_ref"], "refs/tags/v1.2.3");
    assert_eq!(owner_release_detail["is_prerelease"], true);

    let (status, anonymous_quarantine_release) =
        get_release_detail(&app, None, "npm", "release-ui-widget", "1.2.3").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(anonymous_quarantine_release["error"]
        .as_str()
        .expect("error should be present")
        .contains("not found"));

    let (status, upload_body) = upload_release_artifact(
        &app,
        &alice_jwt,
        "npm",
        "release-ui-widget",
        "1.2.3",
        "release-ui-widget-1.2.3.tgz",
        "tarball",
        "application/octet-stream",
        b"release artifact bytes",
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected upload response: {upload_body}"
    );
    assert_eq!(upload_body["kind"], "tarball");
    assert_eq!(upload_body["filename"], "release-ui-widget-1.2.3.tgz");
    assert_eq!(
        upload_body["sha256"]
            .as_str()
            .expect("sha256 should be returned")
            .len(),
        64
    );

    let (status, artifacts_body) =
        list_release_artifacts(&app, Some(&alice_jwt), "npm", "release-ui-widget", "1.2.3").await;
    assert_eq!(status, StatusCode::OK);
    let artifacts = artifacts_body["artifacts"]
        .as_array()
        .expect("artifacts response should be an array");
    assert_eq!(artifacts.len(), 1, "artifacts response: {artifacts_body}");
    assert_eq!(artifacts[0]["kind"], "tarball");
    assert_eq!(artifacts[0]["filename"], "release-ui-widget-1.2.3.tgz");
    assert!(artifacts[0]["uploaded_at"].as_str().is_some());
    assert_eq!(
        artifacts[0]["sha256"]
            .as_str()
            .expect("artifact sha256 should be returned")
            .len(),
        64
    );

    let (status, publish_body) =
        publish_release_for_package(&app, &alice_jwt, "npm", "release-ui-widget", "1.2.3").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected publish response: {publish_body}"
    );
    assert_eq!(publish_body["status"], "scanning");
    assert_eq!(publish_body["artifact_count"], 1);

    let (status, anonymous_published_release) =
        get_release_detail(&app, None, "npm", "release-ui-widget", "1.2.3").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(anonymous_published_release["error"]
        .as_str()
        .expect("error should be present")
        .contains("not found"));

    let (status, owner_scanning_release) =
        get_release_detail(&app, Some(&alice_jwt), "npm", "release-ui-widget", "1.2.3").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(owner_scanning_release["status"], "scanning");
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_package_detail_includes_ecosystem_identity_metadata(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, repository_body) = create_repository(
        &app,
        &alice_jwt,
        "Alice Ecosystem Packages",
        "alice-ecosystem-packages",
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let cases = [
        (
            "npm",
            "@acme/web-widget",
            "npm",
            json!({
                "kind": "npm",
                "details": {
                    "scope": "@acme",
                    "unscoped_name": "web-widget"
                }
            }),
        ),
        (
            "bun",
            "@acme/bun-widget",
            "npm",
            json!({
                "kind": "npm",
                "details": {
                    "scope": "@acme",
                    "unscoped_name": "bun-widget"
                }
            }),
        ),
        (
            "pypi",
            "demo-widget",
            "pypi",
            json!({
                "kind": "pypi",
                "details": {
                    "project_name": "demo-widget",
                    "normalized_name": "demo-widget"
                }
            }),
        ),
        (
            "cargo",
            "demo_widget",
            "cargo",
            json!({
                "kind": "cargo",
                "details": {
                    "crate_name": "demo_widget",
                    "normalized_name": "demo_widget"
                }
            }),
        ),
        (
            "nuget",
            "Native.NuGet.Widget",
            "nuget",
            json!({
                "kind": "nuget",
                "details": {
                    "package_id": "Native.NuGet.Widget",
                    "normalized_id": "native.nuget.widget"
                }
            }),
        ),
        (
            "rubygems",
            "demo-widget-rb",
            "rubygems",
            json!({
                "kind": "rubygems",
                "details": {
                    "gem_name": "demo-widget-rb",
                    "normalized_name": "demo_widget_rb"
                }
            }),
        ),
        (
            "composer",
            "acme/demo-widget",
            "composer",
            json!({
                "kind": "composer",
                "details": {
                    "vendor": "acme",
                    "package": "demo-widget"
                }
            }),
        ),
        (
            "maven",
            "com.acme:demo-widget",
            "maven",
            json!({
                "kind": "maven",
                "details": {
                    "group_id": "com.acme",
                    "artifact_id": "demo-widget"
                }
            }),
        ),
        (
            "oci",
            "acme/demo-widget-image",
            "oci",
            json!({
                "kind": "oci",
                "details": {
                    "repository": "acme/demo-widget-image",
                    "segments": ["acme", "demo-widget-image"]
                }
            }),
        ),
    ];

    for (ecosystem, name, expected_response_ecosystem, expected_metadata) in cases {
        let (status, package_body) = create_package_with_options(
            &app,
            &alice_jwt,
            ecosystem,
            name,
            "alice-ecosystem-packages",
            Some("public"),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "unexpected package response for {ecosystem}/{name}: {package_body}"
        );

        let (status, package_detail) =
            get_package_detail(&app, Some(&alice_jwt), ecosystem, name).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "unexpected package detail response for {ecosystem}/{name}: {package_detail}"
        );
        assert_eq!(
            package_detail["ecosystem"], expected_response_ecosystem,
            "unexpected package ecosystem for {ecosystem}/{name}: {package_detail}"
        );
        assert_eq!(
            package_detail["ecosystem_metadata"], expected_metadata,
            "unexpected package metadata for {ecosystem}/{name}: {package_detail}"
        );
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_release_detail_includes_ecosystem_native_metadata_blocks(pool: PgPool) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let alice_user_id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE username = $1")
        .bind("alice")
        .fetch_one(&pool)
        .await
        .expect("alice user id should be queryable");

    let (status, repository_body) = create_repository(
        &app,
        &alice_jwt,
        "Alice Release Metadata Packages",
        "alice-release-metadata-packages",
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, cargo_package_body) = create_package_with_options(
        &app,
        &alice_jwt,
        "cargo",
        "demo_widget",
        "alice-release-metadata-packages",
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{cargo_package_body}");

    let (status, cargo_release_body) =
        create_release_for_package(&app, &alice_jwt, "cargo", "demo_widget", "1.0.0").await;
    assert_eq!(status, StatusCode::CREATED, "{cargo_release_body}");
    let cargo_release_id = get_release_id(&pool, "cargo", "demo_widget", "1.0.0").await;
    sqlx::query(
        "INSERT INTO cargo_release_metadata (release_id, deps, features, features2, links, rust_version) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(cargo_release_id)
    .bind(json!([
        {
            "name": "serde",
            "req": "^1.0"
        }
    ]))
    .bind(json!({
        "serde": ["dep:serde"]
    }))
    .bind(json!({
        "serde": ["dep:serde"]
    }))
    .bind("demo_widget_native")
    .bind("1.78")
    .execute(&pool)
    .await
    .expect("cargo metadata should insert");

    let (status, cargo_release) =
        get_release_detail(&app, Some(&alice_jwt), "cargo", "demo_widget", "1.0.0").await;
    assert_eq!(status, StatusCode::OK, "{cargo_release}");
    assert_eq!(cargo_release["ecosystem"], "cargo");
    assert_eq!(
        cargo_release["ecosystem_metadata"],
        json!({
            "kind": "cargo",
            "details": {
                "dependencies": [
                    {
                        "name": "serde",
                        "req": "^1.0"
                    }
                ],
                "features": {
                    "serde": ["dep:serde"]
                },
                "features2": {
                    "serde": ["dep:serde"]
                },
                "links": "demo_widget_native",
                "rust_version": "1.78"
            }
        })
    );

    let (status, nuget_package_body) = create_package_with_options(
        &app,
        &alice_jwt,
        "nuget",
        "Native.NuGet.Widget",
        "alice-release-metadata-packages",
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{nuget_package_body}");

    let (status, nuget_release_body) =
        create_release_for_package(&app, &alice_jwt, "nuget", "Native.NuGet.Widget", "2.0.0").await;
    assert_eq!(status, StatusCode::CREATED, "{nuget_release_body}");
    let nuget_release_id = get_release_id(&pool, "nuget", "Native.NuGet.Widget", "2.0.0").await;
    sqlx::query(
        "INSERT INTO nuget_release_metadata (release_id, authors, title, icon_url, license_url, \
                license_expression, project_url, require_license_acceptance, min_client_version, \
                summary, tags, dependency_groups, package_types, is_listed) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)",
    )
    .bind(nuget_release_id)
    .bind("Alice Example")
    .bind("Native NuGet Widget")
    .bind("https://example.test/icon.png")
    .bind("https://example.test/license")
    .bind("MIT")
    .bind("https://example.test/nuget")
    .bind(true)
    .bind("6.0.0")
    .bind("NuGet metadata coverage")
    .bind(vec!["demo".to_owned(), "nuget".to_owned()])
    .bind(json!([
        {
            "target_framework": "net8.0",
            "dependencies": [
                {
                    "id": "Newtonsoft.Json",
                    "range": "[13.0.3,)"
                }
            ]
        }
    ]))
    .bind(json!([
        {
            "name": "Dependency"
        }
    ]))
    .bind(true)
    .execute(&pool)
    .await
    .expect("nuget metadata should insert");

    let (status, nuget_release) = get_release_detail(
        &app,
        Some(&alice_jwt),
        "nuget",
        "Native.NuGet.Widget",
        "2.0.0",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{nuget_release}");
    assert_eq!(nuget_release["ecosystem"], "nuget");
    assert_eq!(
        nuget_release["ecosystem_metadata"],
        json!({
            "kind": "nuget",
            "details": {
                "authors": "Alice Example",
                "title": "Native NuGet Widget",
                "icon_url": "https://example.test/icon.png",
                "license_url": "https://example.test/license",
                "license_expression": "MIT",
                "project_url": "https://example.test/nuget",
                "require_license_acceptance": true,
                "min_client_version": "6.0.0",
                "summary": "NuGet metadata coverage",
                "tags": ["demo", "nuget"],
                "dependency_groups": [
                    {
                        "target_framework": "net8.0",
                        "dependencies": [
                            {
                                "id": "Newtonsoft.Json",
                                "range": "[13.0.3,)"
                            }
                        ]
                    }
                ],
                "package_types": [
                    {
                        "name": "Dependency"
                    }
                ],
                "is_listed": true
            }
        })
    );

    let (status, rubygems_package_body) = create_package_with_options(
        &app,
        &alice_jwt,
        "rubygems",
        "demo-widget-rb",
        "alice-release-metadata-packages",
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{rubygems_package_body}");

    let (status, rubygems_release_body) =
        create_release_for_package(&app, &alice_jwt, "rubygems", "demo-widget-rb", "3.0.0").await;
    assert_eq!(status, StatusCode::CREATED, "{rubygems_release_body}");
    let rubygems_release_id = get_release_id(&pool, "rubygems", "demo-widget-rb", "3.0.0").await;
    sqlx::query(
        "INSERT INTO rubygems_release_metadata (release_id, platform, summary, authors, licenses, \
                required_ruby_version, required_rubygems_version, runtime_dependencies, \
                development_dependencies) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(rubygems_release_id)
    .bind("ruby")
    .bind("RubyGems metadata coverage")
    .bind(vec!["Alice Example".to_owned()])
    .bind(vec!["MIT".to_owned()])
    .bind(">= 3.2")
    .bind(">= 3.5")
    .bind(json!([
        {
            "name": "rack",
            "requirements": [">= 3.0"]
        }
    ]))
    .bind(json!([
        {
            "name": "rspec",
            "requirements": ["~> 3.0"]
        }
    ]))
    .execute(&pool)
    .await
    .expect("rubygems metadata should insert");

    let (status, rubygems_release) = get_release_detail(
        &app,
        Some(&alice_jwt),
        "rubygems",
        "demo-widget-rb",
        "3.0.0",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rubygems_release}");
    assert_eq!(rubygems_release["ecosystem"], "rubygems");
    assert_eq!(
        rubygems_release["ecosystem_metadata"],
        json!({
            "kind": "rubygems",
            "details": {
                "platform": "ruby",
                "summary": "RubyGems metadata coverage",
                "authors": ["Alice Example"],
                "licenses": ["MIT"],
                "required_ruby_version": ">= 3.2",
                "required_rubygems_version": ">= 3.5",
                "runtime_dependencies": [
                    {
                        "name": "rack",
                        "requirements": [">= 3.0"]
                    }
                ],
                "development_dependencies": [
                    {
                        "name": "rspec",
                        "requirements": ["~> 3.0"]
                    }
                ]
            }
        })
    );

    let (status, maven_package_body) = create_package_with_options(
        &app,
        &alice_jwt,
        "maven",
        "com.acme:demo-widget",
        "alice-release-metadata-packages",
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{maven_package_body}");

    let (status, maven_release_body) =
        create_release_for_package(&app, &alice_jwt, "maven", "com.acme:demo-widget", "4.0.0")
            .await;
    assert_eq!(status, StatusCode::CREATED, "{maven_release_body}");
    let maven_release_id = get_release_id(&pool, "maven", "com.acme:demo-widget", "4.0.0").await;
    let maven_provenance = json!({
        "source": "maven_deploy",
        "group_id": "com.acme",
        "artifact_id": "demo-widget",
        "version": "4.0.0",
        "packaging": "jar",
        "display_name": "Demo Widget",
        "description": "Maven metadata coverage",
        "homepage": "https://example.test/maven",
        "repository_url": "https://example.test/repo",
        "licenses": ["Apache-2.0"]
    });
    sqlx::query("UPDATE releases SET provenance = $1 WHERE id = $2")
        .bind(&maven_provenance)
        .bind(maven_release_id)
        .execute(&pool)
        .await
        .expect("maven provenance should update");

    let (status, maven_release) = get_release_detail(
        &app,
        Some(&alice_jwt),
        "maven",
        "com.acme:demo-widget",
        "4.0.0",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{maven_release}");
    assert_eq!(maven_release["ecosystem"], "maven");
    assert_eq!(
        maven_release["ecosystem_metadata"],
        json!({
            "kind": "maven",
            "details": maven_provenance
        })
    );

    let (status, composer_package_body) = create_package_with_options(
        &app,
        &alice_jwt,
        "composer",
        "acme/demo-widget",
        "alice-release-metadata-packages",
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{composer_package_body}");

    let (status, composer_release_body) =
        create_release_for_package(&app, &alice_jwt, "composer", "acme/demo-widget", "5.0.0").await;
    assert_eq!(status, StatusCode::CREATED, "{composer_release_body}");
    let composer_release_id = get_release_id(&pool, "composer", "acme/demo-widget", "5.0.0").await;
    let composer_manifest = json!({
        "name": "acme/demo-widget",
        "type": "library",
        "license": ["MIT"],
        "keywords": ["composer", "demo"],
        "require": {
            "php": "^8.2",
            "symfony/console": "^7.0"
        },
        "autoload": {
            "psr-4": {
                "Acme\\DemoWidget\\": "src/"
            }
        },
        "support": {
            "source": "https://example.test/repo"
        }
    });
    sqlx::query("UPDATE releases SET provenance = $1 WHERE id = $2")
        .bind(&composer_manifest)
        .bind(composer_release_id)
        .execute(&pool)
        .await
        .expect("composer manifest should update");

    let (status, composer_release) = get_release_detail(
        &app,
        Some(&alice_jwt),
        "composer",
        "acme/demo-widget",
        "5.0.0",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{composer_release}");
    assert_eq!(composer_release["ecosystem"], "composer");
    assert_eq!(
        composer_release["ecosystem_metadata"],
        json!({
            "kind": "composer",
            "details": composer_manifest
        })
    );

    let (status, oci_package_body) = create_package_with_options(
        &app,
        &alice_jwt,
        "oci",
        "acme/demo-widget-image",
        "alice-release-metadata-packages",
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{oci_package_body}");
    let oci_package_id = uuid::Uuid::parse_str(
        oci_package_body["id"]
            .as_str()
            .expect("oci package id should be returned"),
    )
    .expect("oci package id should parse");

    let oci_release_id = uuid::Uuid::new_v4();
    let oci_version = format!("sha256:{}", "a".repeat(64));
    let config_digest = format!("sha256:{}", "b".repeat(64));
    let layer_digest = format!("sha256:{}", "c".repeat(64));
    let subject_digest = format!("sha256:{}", "d".repeat(64));
    let oci_manifest = json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.manifest.v1+json",
        "config": {
            "mediaType": "application/vnd.oci.image.config.v1+json",
            "digest": config_digest,
            "size": 7023
        },
        "layers": [
            {
                "mediaType": "application/vnd.oci.image.layer.v1.tar+gzip",
                "digest": layer_digest,
                "size": 32654
            }
        ],
        "subject": {
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "digest": subject_digest,
            "size": 512
        }
    });
    sqlx::query(
        "INSERT INTO releases (id, package_id, version, status, published_by, provenance) \
         VALUES ($1, $2, $3, 'published', $4, $5)",
    )
    .bind(oci_release_id)
    .bind(oci_package_id)
    .bind(&oci_version)
    .bind(alice_user_id)
    .bind(&oci_manifest)
    .execute(&pool)
    .await
    .expect("oci release should insert");

    for (digest, kind, size) in [
        (&config_digest, "config", 7023_i64),
        (&layer_digest, "layer", 32654_i64),
        (&subject_digest, "subject", 512_i64),
    ] {
        sqlx::query(
            "INSERT INTO oci_manifest_references (release_id, ref_digest, ref_kind, ref_size) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind(oci_release_id)
        .bind(digest)
        .bind(kind)
        .bind(size)
        .execute(&pool)
        .await
        .expect("oci manifest reference should insert");
    }

    let (status, oci_release) = get_release_detail(
        &app,
        Some(&alice_jwt),
        "oci",
        "acme/demo-widget-image",
        &oci_version,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{oci_release}");
    assert_eq!(oci_release["ecosystem"], "oci");
    assert_eq!(oci_release["status"], "published");
    assert_eq!(
        oci_release["ecosystem_metadata"],
        json!({
            "kind": "oci",
            "details": {
                "manifest": oci_manifest,
                "references": [
                    {
                        "digest": config_digest,
                        "kind": "config",
                        "size": 7023
                    },
                    {
                        "digest": layer_digest,
                        "kind": "layer",
                        "size": 32654
                    },
                    {
                        "digest": subject_digest,
                        "kind": "subject",
                        "size": 512
                    }
                ]
            }
        })
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_cargo_publish_populates_sparse_index_and_supports_conditional_fetches(pool: PgPool) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, repository_body) = create_repository(
        &app,
        &alice_jwt,
        "Alice Cargo Packages",
        "alice-cargo-packages",
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, token_body) =
        create_personal_access_token(&app, &alice_jwt, "cargo-publish", &["packages:write"]).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected token response: {token_body}"
    );
    let cargo_token = token_body["token"]
        .as_str()
        .expect("cargo token should be returned")
        .to_owned();

    let crate_bytes = b"fake-cargo-crate-tarball";
    let payload = build_cargo_publish_payload(
        json!({
            "name": "demo_widget",
            "vers": "0.1.0",
            "deps": [{
                "name": "serde",
                "version_req": "^1.0",
                "features": ["derive"],
                "optional": false,
                "default_features": true,
                "target": null,
                "kind": "normal",
                "registry": null,
                "explicit_name_in_toml": null
            }],
            "features": {
                "serde": ["dep:serde"]
            },
            "authors": ["Alice <alice@test.dev>"],
            "description": "Cargo adapter integration coverage",
            "homepage": "https://example.test/demo-widget",
            "readme": "# demo_widget",
            "keywords": ["cargo", "demo"],
            "license": "MIT",
            "repository": "https://example.test/repo",
            "links": "demo_widget_native",
            "rust_version": "1.78"
        }),
        crate_bytes,
    );

    let (status, publish_body) = publish_cargo_crate(&app, &cargo_token, payload).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected cargo publish response: {publish_body}"
    );
    assert_eq!(publish_body["warnings"]["invalid_categories"], json!([]));
    assert_eq!(publish_body["warnings"]["invalid_badges"], json!([]));
    assert_eq!(publish_body["warnings"]["other"], json!([]));

    let config_req = Request::builder()
        .method(Method::GET)
        .uri("/cargo/index/config.json")
        .body(Body::empty())
        .unwrap();
    let config_resp = app.clone().oneshot(config_req).await.unwrap();
    assert_eq!(config_resp.status(), StatusCode::OK);
    let config_body = body_json(config_resp).await;
    assert_eq!(
        config_body,
        json!({
            "dl": "http://localhost:3000/cargo/api/v1/crates/{crate}/{version}/download",
            "api": "http://localhost:3000/cargo",
            "auth-required": false
        })
    );

    let index_req = Request::builder()
        .method(Method::GET)
        .uri("/cargo/index/de/mo/demo_widget")
        .header(header::AUTHORIZATION, cargo_token.as_str())
        .body(Body::empty())
        .unwrap();
    let index_resp = app.clone().oneshot(index_req).await.unwrap();
    assert_eq!(index_resp.status(), StatusCode::OK);
    assert_eq!(
        index_resp
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("text/plain")
    );
    let etag = index_resp
        .headers()
        .get("etag")
        .and_then(|value| value.to_str().ok())
        .expect("etag should be present")
        .to_owned();
    let index_body = body_text(index_resp).await;
    let index_lines: Vec<&str> = index_body.lines().collect();
    assert_eq!(
        index_lines.len(),
        1,
        "unexpected sparse index body: {index_body}"
    );

    let index_entry: Value =
        serde_json::from_str(index_lines[0]).expect("index entry should be valid JSON");
    assert_eq!(index_entry["name"], "demo_widget");
    assert_eq!(index_entry["vers"], "0.1.0");
    assert!(!index_entry["yanked"].as_bool().unwrap_or(true));
    assert_eq!(index_entry["cksum"].as_str().map(str::len), Some(64));
    assert_eq!(index_entry["links"], "demo_widget_native");
    assert_eq!(index_entry["rust_version"], "1.78");
    assert_eq!(index_entry["v"], 2);
    assert_eq!(index_entry["deps"][0]["name"], "serde");
    assert_eq!(index_entry["deps"][0]["req"], "^1.0");
    assert_eq!(index_entry["features"]["serde"], json!(["dep:serde"]));
    assert_eq!(index_entry["features2"]["serde"], json!(["dep:serde"]));

    let conditional_req = Request::builder()
        .method(Method::GET)
        .uri("/cargo/index/de/mo/demo_widget")
        .header(header::AUTHORIZATION, cargo_token.as_str())
        .header("if-none-match", &etag)
        .body(Body::empty())
        .unwrap();
    let conditional_resp = app.clone().oneshot(conditional_req).await.unwrap();
    assert_eq!(conditional_resp.status(), StatusCode::NOT_MODIFIED);
    assert_eq!(
        conditional_resp
            .headers()
            .get("etag")
            .and_then(|value| value.to_str().ok()),
        Some(etag.as_str())
    );
    assert!(body_bytes(conditional_resp).await.is_empty());

    let download_req = Request::builder()
        .method(Method::GET)
        .uri("/cargo/api/v1/crates/demo_widget/0.1.0/download")
        .header(header::AUTHORIZATION, cargo_token.as_str())
        .body(Body::empty())
        .unwrap();
    let download_resp = app.clone().oneshot(download_req).await.unwrap();
    assert_eq!(download_resp.status(), StatusCode::OK);
    assert_eq!(
        download_resp
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("application/gzip")
    );
    assert_eq!(
        download_resp
            .headers()
            .get("etag")
            .and_then(|value| value.to_str().ok())
            .map(str::len),
        Some(66)
    );
    assert_eq!(body_bytes(download_resp).await, crate_bytes);

    let release_status: String = sqlx::query_scalar(
        "SELECT r.status::text \
         FROM releases r \
         JOIN packages p ON p.id = r.package_id \
         WHERE p.ecosystem = 'cargo' AND p.normalized_name = $1 AND r.version = $2",
    )
    .bind("demo_widget")
    .bind("0.1.0")
    .fetch_one(&pool)
    .await
    .expect("cargo release status should be queryable");
    assert_eq!(release_status, "published");
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_cargo_private_sparse_index_and_download_require_authentication(pool: PgPool) {
    let app = app(pool);
    let test_password = "test_password";
    register_user(&app, "alice", "alice@test.dev", test_password).await;
    let alice_jwt = login_user(&app, "alice", test_password).await;

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "Alice Private Cargo Packages",
        "alice-private-cargo-packages",
        None,
        Some("private"),
        Some("private"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, token_body) =
        create_personal_access_token(&app, &alice_jwt, "cargo-private", &["packages:write"]).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected token response: {token_body}"
    );
    let cargo_token = token_body["token"]
        .as_str()
        .expect("cargo token should be returned")
        .to_owned();

    let payload = build_cargo_publish_payload(
        json!({
            "name": "secret_widget",
            "vers": "0.1.0",
            "deps": [],
            "features": {},
            "authors": ["Alice <alice@test.dev>"],
            "description": "Private Cargo adapter coverage",
            "license": "MIT"
        }),
        b"private-cargo-crate",
    );

    let (status, publish_body) = publish_cargo_crate(&app, &cargo_token, payload).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected cargo publish response: {publish_body}"
    );

    let anonymous_index_req = Request::builder()
        .method(Method::GET)
        .uri("/cargo/index/se/cr/secret_widget")
        .body(Body::empty())
        .unwrap();
    let anonymous_index_resp = app.clone().oneshot(anonymous_index_req).await.unwrap();
    assert_eq!(anonymous_index_resp.status(), StatusCode::NOT_FOUND);

    let authenticated_index_req = Request::builder()
        .method(Method::GET)
        .uri("/cargo/index/se/cr/secret_widget")
        .header(header::AUTHORIZATION, &cargo_token)
        .body(Body::empty())
        .unwrap();
    let authenticated_index_resp = app.clone().oneshot(authenticated_index_req).await.unwrap();
    assert_eq!(authenticated_index_resp.status(), StatusCode::OK);
    let authenticated_index_body = body_text(authenticated_index_resp).await;
    let authenticated_index_lines: Vec<&str> = authenticated_index_body.lines().collect();
    assert_eq!(authenticated_index_lines.len(), 1);
    let authenticated_index_entry: Value = serde_json::from_str(authenticated_index_lines[0])
        .expect("private sparse index entry should be valid JSON");
    assert_eq!(authenticated_index_entry["vers"], "0.1.0");

    let anonymous_download_req = Request::builder()
        .method(Method::GET)
        .uri("/cargo/api/v1/crates/secret_widget/0.1.0/download")
        .body(Body::empty())
        .unwrap();
    let anonymous_download_resp = app.clone().oneshot(anonymous_download_req).await.unwrap();
    assert_eq!(anonymous_download_resp.status(), StatusCode::NOT_FOUND);

    let authenticated_download_req = Request::builder()
        .method(Method::GET)
        .uri("/cargo/api/v1/crates/secret_widget/0.1.0/download")
        .header(header::AUTHORIZATION, &cargo_token)
        .body(Body::empty())
        .unwrap();
    let authenticated_download_resp = app
        .clone()
        .oneshot(authenticated_download_req)
        .await
        .unwrap();
    assert_eq!(authenticated_download_resp.status(), StatusCode::OK);
    assert_eq!(
        body_bytes(authenticated_download_resp).await,
        b"private-cargo-crate"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_cargo_private_sparse_index_and_download_allow_delegated_team_access(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    register_user(&app, "carol", "carol@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;
    let carol_jwt = login_user(&app, "carol", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &alice_jwt, "Acme Cargo", "acme-cargo").await;
    assert_eq!(status, StatusCode::CREATED, "{org_body}");
    let org_id = org_body["id"]
        .as_str()
        .expect("organization id should be returned");

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "Acme Private Cargo Packages",
        "acme-private-cargo-packages",
        Some(org_id),
        Some("private"),
        Some("private"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, package_body) = create_package_with_options(
        &app,
        &alice_jwt,
        "cargo",
        "secret_widget",
        "acme-private-cargo-packages",
        Some("private"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );

    let (status, _) = add_org_member(&app, &alice_jwt, "acme-cargo", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = add_org_member(&app, &alice_jwt, "acme-cargo", "carol", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_team(
        &app,
        &alice_jwt,
        "acme-cargo",
        "Crate Security",
        "crate-security",
        Some("Reads specific private cargo crates through package delegation."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = create_team(
        &app,
        &alice_jwt,
        "acme-cargo",
        "Registry Publishers",
        "registry-publishers",
        Some("Reads private cargo crates through repository delegation."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) =
        add_team_member_to_team(&app, &alice_jwt, "acme-cargo", "crate-security", "bob").await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = add_team_member_to_team(
        &app,
        &alice_jwt,
        "acme-cargo",
        "registry-publishers",
        "carol",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, grant_body) = grant_team_package_access(
        &app,
        &alice_jwt,
        "acme-cargo",
        "crate-security",
        "cargo",
        "secret_widget",
        &["security_review"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected team package access response: {grant_body}"
    );

    let (status, grant_body) = grant_team_repository_access(
        &app,
        &alice_jwt,
        "acme-cargo",
        "registry-publishers",
        "acme-private-cargo-packages",
        &["publish"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected team repository access response: {grant_body}"
    );

    let (status, token_body) =
        create_personal_access_token(&app, &alice_jwt, "cargo-private", &["packages:write"]).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected token response: {token_body}"
    );
    let cargo_token = token_body["token"]
        .as_str()
        .expect("cargo token should be returned")
        .to_owned();

    let payload = build_cargo_publish_payload(
        json!({
            "name": "secret_widget",
            "vers": "0.1.0",
            "deps": [],
            "features": {},
            "authors": ["Alice <alice@test.dev>"],
            "description": "Private Cargo delegated-read coverage",
            "license": "MIT"
        }),
        b"private-cargo-crate",
    );

    let (status, publish_body) = publish_cargo_crate(&app, &cargo_token, payload).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected cargo publish response: {publish_body}"
    );

    let anonymous_index_req = Request::builder()
        .method(Method::GET)
        .uri("/cargo/index/se/cr/secret_widget")
        .body(Body::empty())
        .unwrap();
    let anonymous_index_resp = app.clone().oneshot(anonymous_index_req).await.unwrap();
    assert_eq!(anonymous_index_resp.status(), StatusCode::NOT_FOUND);

    let bob_index_req = Request::builder()
        .method(Method::GET)
        .uri("/cargo/index/se/cr/secret_widget")
        .header(header::AUTHORIZATION, bob_jwt.as_str())
        .body(Body::empty())
        .unwrap();
    let bob_index_resp = app.clone().oneshot(bob_index_req).await.unwrap();
    assert_eq!(bob_index_resp.status(), StatusCode::OK);
    let bob_index_body = body_text(bob_index_resp).await;
    let bob_index_entry: Value = serde_json::from_str(
        bob_index_body
            .lines()
            .next()
            .expect("package-delegated sparse index entry should exist"),
    )
    .expect("package-delegated sparse index entry should be valid JSON");
    assert_eq!(bob_index_entry["vers"], "0.1.0");

    let carol_index_req = Request::builder()
        .method(Method::GET)
        .uri("/cargo/index/se/cr/secret_widget")
        .header(header::AUTHORIZATION, carol_jwt.as_str())
        .body(Body::empty())
        .unwrap();
    let carol_index_resp = app.clone().oneshot(carol_index_req).await.unwrap();
    assert_eq!(carol_index_resp.status(), StatusCode::OK);
    let carol_index_body = body_text(carol_index_resp).await;
    let carol_index_entry: Value = serde_json::from_str(
        carol_index_body
            .lines()
            .next()
            .expect("repository-delegated sparse index entry should exist"),
    )
    .expect("repository-delegated sparse index entry should be valid JSON");
    assert_eq!(carol_index_entry["vers"], "0.1.0");

    let anonymous_download_req = Request::builder()
        .method(Method::GET)
        .uri("/cargo/api/v1/crates/secret_widget/0.1.0/download")
        .body(Body::empty())
        .unwrap();
    let anonymous_download_resp = app.clone().oneshot(anonymous_download_req).await.unwrap();
    assert_eq!(anonymous_download_resp.status(), StatusCode::NOT_FOUND);

    let bob_download_req = Request::builder()
        .method(Method::GET)
        .uri("/cargo/api/v1/crates/secret_widget/0.1.0/download")
        .header(header::AUTHORIZATION, bob_jwt.as_str())
        .body(Body::empty())
        .unwrap();
    let bob_download_resp = app.clone().oneshot(bob_download_req).await.unwrap();
    assert_eq!(bob_download_resp.status(), StatusCode::OK);
    assert_eq!(body_bytes(bob_download_resp).await, b"private-cargo-crate");

    let carol_download_req = Request::builder()
        .method(Method::GET)
        .uri("/cargo/api/v1/crates/secret_widget/0.1.0/download")
        .header(header::AUTHORIZATION, carol_jwt.as_str())
        .body(Body::empty())
        .unwrap();
    let carol_download_resp = app.clone().oneshot(carol_download_req).await.unwrap();
    assert_eq!(carol_download_resp.status(), StatusCode::OK);
    assert_eq!(
        body_bytes(carol_download_resp).await,
        b"private-cargo-crate"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_cargo_sparse_index_and_download_follow_internal_org_and_mixed_visibility_rules(
    pool: PgPool,
) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    register_user(&app, "carol", "carol@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;
    let carol_jwt = login_user(&app, "carol", "super_secret_pw!").await;

    let (status, org_body) = create_org(
        &app,
        &alice_jwt,
        "Acme Cargo Visibility",
        "acme-cargo-visibility",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"]
        .as_str()
        .expect("organization id should be returned");

    let (status, _) =
        add_org_member(&app, &alice_jwt, "acme-cargo-visibility", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, token_body) = create_personal_access_token(
        &app,
        &alice_jwt,
        "cargo-visibility-matrix",
        &["packages:write"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected token response: {token_body}"
    );
    let cargo_token = token_body["token"]
        .as_str()
        .expect("cargo token should be returned")
        .to_owned();

    struct VisibilityCase<'a> {
        repository_name: &'a str,
        repository_slug: &'a str,
        repository_kind: &'a str,
        repository_visibility: &'a str,
        package_name: &'a str,
        package_visibility: &'a str,
        crate_bytes: &'a [u8],
        anonymous_can_read: bool,
        outsider_can_read: bool,
    }

    let cases = [
        VisibilityCase {
            repository_name: "Cargo Internal Org Packages",
            repository_slug: "cargo-internal-org-packages",
            repository_kind: "private",
            repository_visibility: "internal_org",
            package_name: "secret_internal_org_widget",
            package_visibility: "internal_org",
            crate_bytes: b"cargo-internal-org-widget",
            anonymous_can_read: false,
            outsider_can_read: false,
        },
        VisibilityCase {
            repository_name: "Cargo Public Repository",
            repository_slug: "cargo-public-packages",
            repository_kind: "public",
            repository_visibility: "public",
            package_name: "secret_private_public_widget",
            package_visibility: "private",
            crate_bytes: b"cargo-private-package-public-repo",
            anonymous_can_read: false,
            outsider_can_read: false,
        },
        VisibilityCase {
            repository_name: "Cargo Internal Private Packages",
            repository_slug: "cargo-internal-private-packages",
            repository_kind: "private",
            repository_visibility: "internal_org",
            package_name: "secret_private_internal_widget",
            package_visibility: "private",
            crate_bytes: b"cargo-private-package-internal-repo",
            anonymous_can_read: false,
            outsider_can_read: false,
        },
    ];

    for case in cases {
        let (status, repository_body) = create_repository_with_options(
            &app,
            &alice_jwt,
            case.repository_name,
            case.repository_slug,
            Some(org_id),
            Some(case.repository_kind),
            Some(case.repository_visibility),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "unexpected repository response for {}: {repository_body}",
            case.package_name
        );

        let (status, package_body) = create_package_with_options(
            &app,
            &alice_jwt,
            "cargo",
            case.package_name,
            case.repository_slug,
            Some(case.package_visibility),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "unexpected package response for {}: {package_body}",
            case.package_name
        );

        let payload = build_cargo_publish_payload(
            json!({
                "name": case.package_name,
                "vers": "0.1.0",
                "deps": [],
                "features": {},
                "authors": ["Alice <alice@test.dev>"],
                "description": format!("Cargo visibility matrix coverage for {}", case.package_name),
                "license": "MIT"
            }),
            case.crate_bytes,
        );

        let (status, publish_body) = publish_cargo_crate(&app, &cargo_token, payload).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "unexpected cargo publish response for {}: {publish_body}",
            case.package_name
        );

        let anonymous_index_resp = get_cargo_sparse_index(&app, None, case.package_name).await;
        if case.anonymous_can_read {
            assert_eq!(
                anonymous_index_resp.status(),
                StatusCode::OK,
                "anonymous sparse index response should be readable for {}",
                case.package_name
            );
            let anonymous_index_body = body_text(anonymous_index_resp).await;
            let anonymous_index_entry: Value = serde_json::from_str(
                anonymous_index_body
                    .lines()
                    .next()
                    .expect("sparse index entry should exist"),
            )
            .expect("sparse index entry should be valid JSON");
            assert_eq!(anonymous_index_entry["name"], case.package_name);
        } else {
            assert_eq!(
                anonymous_index_resp.status(),
                StatusCode::NOT_FOUND,
                "anonymous sparse index response should be hidden for {}",
                case.package_name
            );
        }

        let member_index_resp =
            get_cargo_sparse_index(&app, Some(&bob_jwt), case.package_name).await;
        assert_eq!(
            member_index_resp.status(),
            StatusCode::OK,
            "organization member should read sparse index for {}",
            case.package_name
        );
        assert_eq!(
            member_index_resp
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("text/plain")
        );
        let member_index_body = body_text(member_index_resp).await;
        let member_index_entry: Value = serde_json::from_str(
            member_index_body
                .lines()
                .next()
                .expect("sparse index entry should exist"),
        )
        .expect("sparse index entry should be valid JSON");
        assert_eq!(member_index_entry["name"], case.package_name);
        assert_eq!(member_index_entry["vers"], "0.1.0");

        let outsider_index_resp =
            get_cargo_sparse_index(&app, Some(&carol_jwt), case.package_name).await;
        if case.outsider_can_read {
            assert_eq!(
                outsider_index_resp.status(),
                StatusCode::OK,
                "non-member sparse index response should be readable for {}",
                case.package_name
            );
        } else {
            assert_eq!(
                outsider_index_resp.status(),
                StatusCode::NOT_FOUND,
                "non-member sparse index response should be hidden for {}",
                case.package_name
            );
        }

        let anonymous_download_resp =
            download_cargo_crate(&app, None, case.package_name, "0.1.0").await;
        if case.anonymous_can_read {
            assert_eq!(
                anonymous_download_resp.status(),
                StatusCode::OK,
                "anonymous download response should be readable for {}",
                case.package_name
            );
            assert_eq!(body_bytes(anonymous_download_resp).await, case.crate_bytes);
        } else {
            assert_eq!(
                anonymous_download_resp.status(),
                StatusCode::NOT_FOUND,
                "anonymous download response should be hidden for {}",
                case.package_name
            );
            let anonymous_download_body = body_json(anonymous_download_resp).await;
            assert_eq!(
                cargo_error_detail(&anonymous_download_body),
                "Crate not found"
            );
        }

        let member_download_resp =
            download_cargo_crate(&app, Some(&bob_jwt), case.package_name, "0.1.0").await;
        assert_eq!(
            member_download_resp.status(),
            StatusCode::OK,
            "organization member should download crate bytes for {}",
            case.package_name
        );
        assert_eq!(
            member_download_resp
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("application/gzip")
        );
        assert_eq!(body_bytes(member_download_resp).await, case.crate_bytes);

        let outsider_download_resp =
            download_cargo_crate(&app, Some(&carol_jwt), case.package_name, "0.1.0").await;
        if case.outsider_can_read {
            assert_eq!(
                outsider_download_resp.status(),
                StatusCode::OK,
                "non-member download response should be readable for {}",
                case.package_name
            );
            assert_eq!(body_bytes(outsider_download_resp).await, case.crate_bytes);
        } else {
            assert_eq!(
                outsider_download_resp.status(),
                StatusCode::NOT_FOUND,
                "non-member download response should be hidden for {}",
                case.package_name
            );
            let outsider_download_body = body_json(outsider_download_resp).await;
            assert_eq!(
                cargo_error_detail(&outsider_download_body),
                "Crate not found"
            );
        }
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_cargo_yank_and_unyank_update_sparse_index_and_audit_log(pool: PgPool) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, repository_body) = create_repository(
        &app,
        &alice_jwt,
        "Alice Cargo Packages",
        "alice-cargo-packages",
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, token_body) =
        create_personal_access_token(&app, &alice_jwt, "cargo-yank", &["packages:write"]).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected token response: {token_body}"
    );
    let cargo_token = token_body["token"]
        .as_str()
        .expect("cargo token should be returned")
        .to_owned();

    let crate_bytes = b"cargo-yankable-crate";
    let payload = build_cargo_publish_payload(
        json!({
            "name": "toggle_widget",
            "vers": "0.1.0",
            "deps": [],
            "features": {},
            "authors": ["Alice <alice@test.dev>"],
            "description": "Cargo yank and unyank coverage",
            "license": "MIT"
        }),
        crate_bytes,
    );

    let (status, publish_body) = publish_cargo_crate(&app, &cargo_token, payload).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected cargo publish response: {publish_body}"
    );

    let release_id = get_release_id(&pool, "cargo", "toggle_widget", "0.1.0").await;

    let (status, yank_body) =
        yank_cargo_crate_version(&app, &cargo_token, "toggle_widget", "0.1.0").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected yank response: {yank_body}"
    );
    assert_eq!(yank_body, json!({ "ok": true }));

    let release_state_after_yank: (String, bool) =
        sqlx::query_as("SELECT status::text, is_yanked FROM releases WHERE id = $1")
            .bind(release_id)
            .fetch_one(&pool)
            .await
            .expect("cargo release should be queryable after yank");
    assert_eq!(release_state_after_yank, ("yanked".to_owned(), true));

    let yanked_index_req = Request::builder()
        .method(Method::GET)
        .uri("/cargo/index/to/gg/toggle_widget")
        .header(header::AUTHORIZATION, cargo_token.as_str())
        .body(Body::empty())
        .unwrap();
    let yanked_index_resp = app.clone().oneshot(yanked_index_req).await.unwrap();
    assert_eq!(yanked_index_resp.status(), StatusCode::OK);
    let yanked_index_body = body_text(yanked_index_resp).await;
    let yanked_index_entry: Value = serde_json::from_str(
        yanked_index_body
            .lines()
            .next()
            .expect("yanked sparse index entry should exist"),
    )
    .expect("yanked sparse index entry should be valid JSON");
    assert_eq!(yanked_index_entry["yanked"], true);

    let yanked_download_req = Request::builder()
        .method(Method::GET)
        .uri("/cargo/api/v1/crates/toggle_widget/0.1.0/download")
        .header(header::AUTHORIZATION, cargo_token.as_str())
        .body(Body::empty())
        .unwrap();
    let yanked_download_resp = app.clone().oneshot(yanked_download_req).await.unwrap();
    assert_eq!(yanked_download_resp.status(), StatusCode::OK);
    assert_eq!(body_bytes(yanked_download_resp).await, crate_bytes);

    let (status, unyank_body) =
        unyank_cargo_crate_version(&app, &cargo_token, "toggle_widget", "0.1.0").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected unyank response: {unyank_body}"
    );
    assert_eq!(unyank_body, json!({ "ok": true }));

    let release_state_after_unyank: (String, bool) =
        sqlx::query_as("SELECT status::text, is_yanked FROM releases WHERE id = $1")
            .bind(release_id)
            .fetch_one(&pool)
            .await
            .expect("cargo release should be queryable after unyank");
    assert_eq!(release_state_after_unyank, ("published".to_owned(), false));

    let restored_index_req = Request::builder()
        .method(Method::GET)
        .uri("/cargo/index/to/gg/toggle_widget")
        .header(header::AUTHORIZATION, cargo_token.as_str())
        .body(Body::empty())
        .unwrap();
    let restored_index_resp = app.clone().oneshot(restored_index_req).await.unwrap();
    assert_eq!(restored_index_resp.status(), StatusCode::OK);
    let restored_index_body = body_text(restored_index_resp).await;
    let restored_index_entry: Value = serde_json::from_str(
        restored_index_body
            .lines()
            .next()
            .expect("restored sparse index entry should exist"),
    )
    .expect("restored sparse index entry should be valid JSON");
    assert_eq!(restored_index_entry["yanked"], false);

    let audit_actions: Vec<String> = sqlx::query_scalar(
        "SELECT action::text \
         FROM audit_logs \
         WHERE target_release_id = $1 AND action IN ('release_yank', 'release_unyank') \
         ORDER BY occurred_at ASC",
    )
    .bind(release_id)
    .fetch_all(&pool)
    .await
    .expect("cargo yank audit actions should be queryable");
    assert_eq!(
        audit_actions,
        vec!["release_yank".to_owned(), "release_unyank".to_owned()]
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_cargo_owner_endpoints_list_org_admins_and_acknowledge_mutations(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &alice_jwt, "Acme Cargo", "acme-cargo").await;
    assert_eq!(status, StatusCode::CREATED, "{org_body}");
    let org_id = org_body["id"]
        .as_str()
        .expect("organization id should be returned")
        .to_owned();

    let (status, add_member_body) =
        add_org_member(&app, &alice_jwt, "acme-cargo", "bob", "admin").await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected member add response: {add_member_body}"
    );

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "Acme Cargo Packages",
        "acme-cargo-packages",
        Some(&org_id),
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, package_body) = create_package_with_options(
        &app,
        &alice_jwt,
        "cargo",
        "org_widget",
        "acme-cargo-packages",
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );

    let (status, token_body) =
        create_personal_access_token(&app, &alice_jwt, "cargo-owners", &["packages:write"]).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected token response: {token_body}"
    );
    let cargo_token = token_body["token"]
        .as_str()
        .expect("cargo token should be returned")
        .to_owned();

    let (status, owners_body) = list_cargo_crate_owners(&app, &cargo_token, "org_widget").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected cargo owners response: {owners_body}"
    );
    let owners = owners_body["users"]
        .as_array()
        .expect("cargo owners response should include a users array");
    let owner_logins = owners
        .iter()
        .filter_map(|entry| entry["login"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(owner_logins, vec!["alice", "bob"]);

    let (status, add_owners_body) =
        add_cargo_crate_owners(&app, &cargo_token, "org_widget", &["carol", "dave"]).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected cargo add owners response: {add_owners_body}"
    );
    assert_eq!(add_owners_body["ok"], true);
    assert!(add_owners_body["msg"]
        .as_str()
        .expect("cargo add owners response should include a message")
        .contains("Requested users: [\"carol\", \"dave\"]"));

    let (status, remove_owners_body) =
        remove_cargo_crate_owners(&app, &cargo_token, "org_widget", &["alice"]).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected cargo remove owners response: {remove_owners_body}"
    );
    assert_eq!(remove_owners_body["ok"], true);
    assert!(remove_owners_body["msg"]
        .as_str()
        .expect("cargo remove owners response should include a message")
        .contains("Requested removal: [\"alice\"]"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_cargo_yank_and_unyank_negative_auth_scope_and_ownership_paths(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, repository_body) = create_repository(
        &app,
        &alice_jwt,
        "Alice Cargo Packages",
        "alice-cargo-packages",
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, write_token_body) =
        create_personal_access_token(&app, &alice_jwt, "cargo-write", &["packages:write"]).await;
    assert_eq!(status, StatusCode::CREATED, "{write_token_body}");
    let alice_cargo_token = write_token_body["token"]
        .as_str()
        .expect("write-scoped cargo token should be returned")
        .to_owned();

    let (status, read_only_token_body) =
        create_personal_access_token(&app, &alice_jwt, "cargo-read-only", &["tokens:read"]).await;
    assert_eq!(status, StatusCode::CREATED, "{read_only_token_body}");
    let alice_read_only_token = read_only_token_body["token"]
        .as_str()
        .expect("read-only cargo token should be returned")
        .to_owned();

    let (status, bob_token_body) =
        create_personal_access_token(&app, &bob_jwt, "bob-cargo-write", &["packages:write"]).await;
    assert_eq!(status, StatusCode::CREATED, "{bob_token_body}");
    let bob_cargo_token = bob_token_body["token"]
        .as_str()
        .expect("bob cargo token should be returned")
        .to_owned();

    let payload = build_cargo_publish_payload(
        json!({
            "name": "locked_widget",
            "vers": "0.1.0",
            "deps": [],
            "features": {},
            "authors": ["Alice <alice@test.dev>"],
            "description": "Cargo negative-path yank coverage",
            "license": "MIT"
        }),
        b"locked-cargo-crate",
    );
    let (status, publish_body) = publish_cargo_crate(&app, &alice_cargo_token, payload).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected cargo publish response: {publish_body}"
    );

    let missing_auth_yank_req = Request::builder()
        .method(Method::DELETE)
        .uri("/cargo/api/v1/crates/locked_widget/0.1.0/yank")
        .body(Body::empty())
        .unwrap();
    let missing_auth_yank_resp = app.clone().oneshot(missing_auth_yank_req).await.unwrap();
    assert_eq!(missing_auth_yank_resp.status(), StatusCode::UNAUTHORIZED);
    let missing_auth_yank_body = body_json(missing_auth_yank_resp).await;
    assert_eq!(
        cargo_error_detail(&missing_auth_yank_body),
        "Authentication required. Run `cargo login --registry <name>` to authenticate."
    );

    let (status, insufficient_scope_yank_body) =
        yank_cargo_crate_version(&app, &alice_read_only_token, "locked_widget", "0.1.0").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        cargo_error_detail(&insufficient_scope_yank_body),
        "Token does not have the packages:write scope"
    );

    let (status, missing_crate_yank_body) =
        yank_cargo_crate_version(&app, &alice_cargo_token, "missing_widget", "0.1.0").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        cargo_error_detail(&missing_crate_yank_body),
        "Crate not found"
    );

    let (status, missing_version_unyank_body) =
        unyank_cargo_crate_version(&app, &alice_cargo_token, "locked_widget", "9.9.9").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        cargo_error_detail(&missing_version_unyank_body),
        "Version not found"
    );

    let (status, unauthorized_unyank_body) =
        unyank_cargo_crate_version(&app, &bob_cargo_token, "locked_widget", "0.1.0").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        cargo_error_detail(&unauthorized_unyank_body),
        "You do not have permission to modify this crate"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_cargo_owner_negative_auth_scope_and_ownership_paths(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, repository_body) = create_repository(
        &app,
        &alice_jwt,
        "Alice Cargo Packages",
        "alice-cargo-packages",
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, package_body) = create_package_with_options(
        &app,
        &alice_jwt,
        "cargo",
        "owner_locked_widget",
        "alice-cargo-packages",
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );

    let (status, write_token_body) =
        create_personal_access_token(&app, &alice_jwt, "cargo-owners-write", &["packages:write"])
            .await;
    assert_eq!(status, StatusCode::CREATED, "{write_token_body}");
    let alice_cargo_token = write_token_body["token"]
        .as_str()
        .expect("write-scoped cargo token should be returned")
        .to_owned();

    let (status, read_only_token_body) =
        create_personal_access_token(&app, &alice_jwt, "cargo-owners-read-only", &["tokens:read"])
            .await;
    assert_eq!(status, StatusCode::CREATED, "{read_only_token_body}");
    let alice_read_only_token = read_only_token_body["token"]
        .as_str()
        .expect("read-only cargo token should be returned")
        .to_owned();

    let (status, bob_token_body) =
        create_personal_access_token(&app, &bob_jwt, "bob-cargo-owners", &["packages:write"]).await;
    assert_eq!(status, StatusCode::CREATED, "{bob_token_body}");
    let bob_cargo_token = bob_token_body["token"]
        .as_str()
        .expect("bob cargo token should be returned")
        .to_owned();

    let missing_auth_list_req = Request::builder()
        .method(Method::GET)
        .uri("/cargo/api/v1/crates/owner_locked_widget/owners")
        .body(Body::empty())
        .unwrap();
    let missing_auth_list_resp = app.clone().oneshot(missing_auth_list_req).await.unwrap();
    assert_eq!(missing_auth_list_resp.status(), StatusCode::UNAUTHORIZED);
    let missing_auth_list_body = body_json(missing_auth_list_resp).await;
    assert_eq!(
        cargo_error_detail(&missing_auth_list_body),
        "Authentication required. Run `cargo login --registry <name>` to authenticate."
    );

    let (status, missing_crate_list_body) =
        list_cargo_crate_owners(&app, &alice_cargo_token, "missing_widget").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        cargo_error_detail(&missing_crate_list_body),
        "Crate not found"
    );

    let (status, insufficient_scope_add_body) = add_cargo_crate_owners(
        &app,
        &alice_read_only_token,
        "owner_locked_widget",
        &["carol"],
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        cargo_error_detail(&insufficient_scope_add_body),
        "Token does not have the packages:write scope"
    );

    let (status, insufficient_scope_remove_body) = remove_cargo_crate_owners(
        &app,
        &alice_read_only_token,
        "owner_locked_widget",
        &["carol"],
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        cargo_error_detail(&insufficient_scope_remove_body),
        "Token does not have the packages:write scope"
    );

    let (status, unauthorized_add_body) =
        add_cargo_crate_owners(&app, &bob_cargo_token, "owner_locked_widget", &["carol"]).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        cargo_error_detail(&unauthorized_add_body),
        "You do not have permission to modify this crate"
    );

    let (status, unauthorized_remove_body) =
        remove_cargo_crate_owners(&app, &bob_cargo_token, "owner_locked_widget", &["alice"]).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        cargo_error_detail(&unauthorized_remove_body),
        "You do not have permission to modify this crate"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_npm_packument_and_dist_tag_listing_ignore_quarantine_channel_refs(pool: PgPool) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, repository_body) = create_repository(
        &app,
        &alice_jwt,
        "Alice npm Packages",
        "alice-npm-packages",
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, package_body) = create_package_with_options(
        &app,
        &alice_jwt,
        "npm",
        "quarantine-tag-widget",
        "alice-npm-packages",
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );
    let package_id = uuid::Uuid::parse_str(
        package_body["id"]
            .as_str()
            .expect("package id should be returned"),
    )
    .expect("package id should parse");

    let alice_user_id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE username = $1")
        .bind("alice")
        .fetch_one(&pool)
        .await
        .expect("alice user id should be queryable");

    let (status, release_body) =
        create_release_for_package(&app, &alice_jwt, "npm", "quarantine-tag-widget", "1.0.0").await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected release response: {release_body}"
    );
    assert_eq!(release_body["status"], "quarantine");

    let release_id = get_release_id(&pool, "npm", "quarantine-tag-widget", "1.0.0").await;
    insert_channel_ref(&pool, package_id, "latest", release_id, alice_user_id).await;

    let (status, packument) = get_npm_packument(&app, None, "quarantine-tag-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(packument["versions"], json!({}));
    assert_eq!(packument["dist-tags"], json!({}));

    let (status, dist_tags) = list_npm_dist_tags(&app, None, "quarantine-tag-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(dist_tags, json!({}));
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_npm_dist_tag_mutation_requires_a_readable_release(pool: PgPool) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, repository_body) = create_repository(
        &app,
        &alice_jwt,
        "Alice npm Packages",
        "alice-npm-packages",
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, package_body) = create_package_with_options(
        &app,
        &alice_jwt,
        "npm",
        "taggable-widget",
        "alice-npm-packages",
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );

    let (status, release_body) =
        create_release_for_package(&app, &alice_jwt, "npm", "taggable-widget", "1.0.0").await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected release response: {release_body}"
    );
    assert_eq!(release_body["status"], "quarantine");
    let release_id = get_release_id(&pool, "npm", "taggable-widget", "1.0.0").await;

    let (status, denied_tag_body) =
        set_npm_dist_tag(&app, &alice_jwt, "taggable-widget", "beta", "1.0.0").await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(denied_tag_body["error"]
        .as_str()
        .expect("error should be present")
        .contains("cannot receive dist-tags"));

    let (status, empty_dist_tags) = list_npm_dist_tags(&app, None, "taggable-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(empty_dist_tags, json!({}));

    sqlx::query("UPDATE releases SET status = 'published', updated_at = NOW() WHERE id = $1")
        .bind(release_id)
        .execute(&pool)
        .await
        .expect("release status should update");

    let (status, set_tag_body) =
        set_npm_dist_tag(&app, &alice_jwt, "taggable-widget", "beta", "1.0.0").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected dist-tag response: {set_tag_body}"
    );
    assert_eq!(set_tag_body["ok"], true);

    let (status, dist_tags) = list_npm_dist_tags(&app, None, "taggable-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(dist_tags["beta"], "1.0.0");

    let (status, packument) = get_npm_packument(&app, None, "taggable-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(packument["dist-tags"]["beta"], "1.0.0");
    assert_eq!(packument["versions"]["1.0.0"]["version"], "1.0.0");
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_native_npm_publish_auto_creates_org_owned_package_from_repository_publish_grant(
    pool: PgPool,
) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &alice_jwt, "Source Org", "source-org").await;
    assert_eq!(status, StatusCode::CREATED);
    let source_org_id = org_body["id"].as_str().expect("source org id");
    let source_org_uuid = uuid::Uuid::parse_str(source_org_id).expect("source org id should parse");

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "Source Packages",
        "source-packages",
        Some(source_org_id),
        Some("private"),
        Some("private"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, _) = add_org_member(&app, &alice_jwt, "source-org", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_team(
        &app,
        &alice_jwt,
        "source-org",
        "Release Engineering",
        "release-engineering",
        Some("Publishes npm packages through repository delegation."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) =
        add_team_member_to_team(&app, &alice_jwt, "source-org", "release-engineering", "bob").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, grant_body) = grant_team_repository_access(
        &app,
        &alice_jwt,
        "source-org",
        "release-engineering",
        "source-packages",
        &["publish"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected team repository access response: {grant_body}"
    );

    let (status, publish_body) = publish_npm_package(
        &app,
        &bob_jwt,
        "native-org-widget",
        "1.0.0",
        b"native-org-widget-tarball",
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected npm publish response: {publish_body}"
    );
    assert_eq!(publish_body["ok"], true);

    let package_owner = sqlx::query(
        "SELECT owner_user_id, owner_org_id, visibility::text AS visibility \
         FROM packages \
         WHERE ecosystem = 'npm' AND name = $1",
    )
    .bind("native-org-widget")
    .fetch_one(&pool)
    .await
    .expect("native npm package should exist");
    let package_owner_user_id = package_owner
        .try_get::<Option<uuid::Uuid>, _>("owner_user_id")
        .expect("package owner user id should be readable");
    let package_owner_org_id = package_owner
        .try_get::<Option<uuid::Uuid>, _>("owner_org_id")
        .expect("package owner org id should be readable");
    let package_visibility = package_owner
        .try_get::<String, _>("visibility")
        .expect("package visibility should be readable");
    assert_eq!(package_owner_user_id, None);
    assert_eq!(package_owner_org_id, Some(source_org_uuid));
    assert_eq!(package_visibility, "private");

    let (status, package_detail) =
        get_package_detail(&app, Some(&bob_jwt), "npm", "native-org-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(package_detail["owner_org_slug"], "source-org");
    assert_eq!(package_detail["can_manage_releases"], true);
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_native_npm_repository_publish_permission_allows_existing_package_publish_and_dist_tag_updates(
    pool: PgPool,
) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &alice_jwt, "Source Org", "source-org").await;
    assert_eq!(status, StatusCode::CREATED);
    let source_org_id = org_body["id"].as_str().expect("source org id");

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "Source Packages",
        "source-packages",
        Some(source_org_id),
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, package_body) = create_package_with_options(
        &app,
        &alice_jwt,
        "npm",
        "native-release-widget",
        "source-packages",
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );

    let (status, _) = add_org_member(&app, &alice_jwt, "source-org", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_team(
        &app,
        &alice_jwt,
        "source-org",
        "Release Engineering",
        "release-engineering",
        Some("Publishes existing npm packages and manages dist-tags."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) =
        add_team_member_to_team(&app, &alice_jwt, "source-org", "release-engineering", "bob").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, grant_body) = grant_team_repository_access(
        &app,
        &alice_jwt,
        "source-org",
        "release-engineering",
        "source-packages",
        &["publish"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected team repository access response: {grant_body}"
    );

    let (status, publish_body) = publish_npm_package(
        &app,
        &bob_jwt,
        "native-release-widget",
        "1.0.0",
        b"native-release-widget-tarball",
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected npm publish response: {publish_body}"
    );
    assert_eq!(publish_body["ok"], true);

    let release_id = get_release_id(&pool, "npm", "native-release-widget", "1.0.0").await;
    let release_status: String =
        sqlx::query_scalar("SELECT status::text FROM releases WHERE id = $1")
            .bind(release_id)
            .fetch_one(&pool)
            .await
            .expect("native npm release status should be queryable");
    assert_eq!(release_status, "published");

    let (status, detail_after_publish) =
        get_package_detail(&app, Some(&bob_jwt), "npm", "native-release-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail_after_publish["can_manage_releases"], true);

    let (status, set_tag_body) =
        set_npm_dist_tag(&app, &bob_jwt, "native-release-widget", "beta", "1.0.0").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected dist-tag response: {set_tag_body}"
    );
    assert_eq!(set_tag_body["ok"], true);

    let (status, dist_tags) =
        list_npm_dist_tags(&app, Some(&bob_jwt), "native-release-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(dist_tags["beta"], "1.0.0");
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_native_npm_private_reads_allow_team_package_and_repository_grants(pool: PgPool) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    register_user(&app, "carol", "carol@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;
    let carol_jwt = login_user(&app, "carol", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &alice_jwt, "Acme Org", "acme-org").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id should be present");

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "Private npm Packages",
        "private-npm-packages",
        Some(org_id),
        Some("private"),
        Some("private"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, package_body) = create_package_with_options(
        &app,
        &alice_jwt,
        "npm",
        "secret-team-widget",
        "private-npm-packages",
        Some("private"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );

    let (status, publish_body) = publish_npm_package(
        &app,
        &alice_jwt,
        "secret-team-widget",
        "1.0.0",
        b"secret-team-widget-tarball",
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected npm publish response: {publish_body}"
    );

    let (status, _) = add_org_member(&app, &alice_jwt, "acme-org", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = add_org_member(&app, &alice_jwt, "acme-org", "carol", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_team(
        &app,
        &alice_jwt,
        "acme-org",
        "Package Security",
        "package-security",
        Some("Reads specific private npm packages through package delegation."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = create_team(
        &app,
        &alice_jwt,
        "acme-org",
        "Repository Publishers",
        "repository-publishers",
        Some("Reads private npm packages through repository delegation."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) =
        add_team_member_to_team(&app, &alice_jwt, "acme-org", "package-security", "bob").await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = add_team_member_to_team(
        &app,
        &alice_jwt,
        "acme-org",
        "repository-publishers",
        "carol",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, grant_body) = grant_team_package_access(
        &app,
        &alice_jwt,
        "acme-org",
        "package-security",
        "npm",
        "secret-team-widget",
        &["security_review"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected package access response: {grant_body}"
    );

    let (status, grant_body) = grant_team_repository_access(
        &app,
        &alice_jwt,
        "acme-org",
        "repository-publishers",
        "private-npm-packages",
        &["publish"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected repository access response: {grant_body}"
    );

    let tarball_filename = "secret-team-widget-1.0.0.tgz";

    let (status, anonymous_packument) = get_npm_packument(&app, None, "secret-team-widget").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(anonymous_packument["error"], "Package not found");

    let (status, anonymous_headers, anonymous_tarball) =
        download_npm_tarball(&app, None, "secret-team-widget", tarball_filename).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        anonymous_headers
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("application/json")
    );
    assert_eq!(anonymous_tarball, b"{\"error\":\"Package not found\"}");

    let (status, bob_packument) =
        get_npm_packument(&app, Some(&bob_jwt), "secret-team-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bob_packument["name"], "secret-team-widget");
    assert_eq!(bob_packument["versions"]["1.0.0"]["version"], "1.0.0");

    let (status, bob_dist_tags) =
        list_npm_dist_tags(&app, Some(&bob_jwt), "secret-team-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bob_dist_tags["latest"], "1.0.0");

    let (status, bob_headers, bob_tarball) =
        download_npm_tarball(&app, Some(&bob_jwt), "secret-team-widget", tarball_filename).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        bob_headers
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("application/gzip")
    );
    assert_eq!(bob_tarball, b"secret-team-widget-tarball");

    let (status, carol_packument) =
        get_npm_packument(&app, Some(&carol_jwt), "secret-team-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(carol_packument["dist-tags"]["latest"], "1.0.0");

    let (status, carol_headers, carol_tarball) = download_npm_tarball(
        &app,
        Some(&carol_jwt),
        "secret-team-widget",
        tarball_filename,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        carol_headers
            .get(header::CONTENT_DISPOSITION)
            .and_then(|value| value.to_str().ok()),
        Some("attachment; filename=\"secret-team-widget-1.0.0.tgz\"")
    );
    assert_eq!(carol_tarball, b"secret-team-widget-tarball");
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_native_cargo_publish_auto_creates_org_owned_package_from_repository_publish_grant(
    pool: PgPool,
) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &alice_jwt, "Source Org", "source-org").await;
    assert_eq!(status, StatusCode::CREATED);
    let source_org_id = org_body["id"].as_str().expect("source org id");
    let source_org_uuid = uuid::Uuid::parse_str(source_org_id).expect("source org id should parse");

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "Source Crates",
        "source-crates",
        Some(source_org_id),
        Some("private"),
        Some("private"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, _) = add_org_member(&app, &alice_jwt, "source-org", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_team(
        &app,
        &alice_jwt,
        "source-org",
        "Crate Release Engineering",
        "crate-release-engineering",
        Some("Publishes cargo crates through repository delegation."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = add_team_member_to_team(
        &app,
        &alice_jwt,
        "source-org",
        "crate-release-engineering",
        "bob",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, grant_body) = grant_team_repository_access(
        &app,
        &alice_jwt,
        "source-org",
        "crate-release-engineering",
        "source-crates",
        &["publish"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected team repository access response: {grant_body}"
    );

    let (status, token_body) =
        create_personal_access_token(&app, &bob_jwt, "bob-cargo-publish", &["packages:write"])
            .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected token response: {token_body}"
    );
    let cargo_token = token_body["token"]
        .as_str()
        .expect("cargo token should be returned")
        .to_owned();

    let crate_bytes = b"native-org-crate-tarball";
    let payload = build_cargo_publish_payload(
        json!({
            "name": "native_org_crate",
            "vers": "1.0.0",
            "deps": [],
            "features": {},
            "authors": ["Bob <bob@test.dev>"],
            "description": "Repository-delegated cargo publish",
            "license": "MIT"
        }),
        crate_bytes,
    );

    let (status, publish_body) = publish_cargo_crate(&app, &cargo_token, payload).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected cargo publish response: {publish_body}"
    );

    let package_owner = sqlx::query(
        "SELECT owner_user_id, owner_org_id, visibility::text AS visibility \
         FROM packages \
         WHERE ecosystem = 'cargo' AND normalized_name = $1",
    )
    .bind("native_org_crate")
    .fetch_one(&pool)
    .await
    .expect("native cargo package should exist");
    let package_owner_user_id = package_owner
        .try_get::<Option<uuid::Uuid>, _>("owner_user_id")
        .expect("package owner user id should be readable");
    let package_owner_org_id = package_owner
        .try_get::<Option<uuid::Uuid>, _>("owner_org_id")
        .expect("package owner org id should be readable");
    let package_visibility = package_owner
        .try_get::<String, _>("visibility")
        .expect("package visibility should be readable");
    assert_eq!(package_owner_user_id, None);
    assert_eq!(package_owner_org_id, Some(source_org_uuid));
    assert_eq!(package_visibility, "private");

    let (status, package_detail) =
        get_package_detail(&app, Some(&bob_jwt), "cargo", "native_org_crate").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(package_detail["owner_org_slug"], "source-org");
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_native_cargo_repository_publish_permission_allows_existing_package_publish(
    pool: PgPool,
) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &alice_jwt, "Source Org", "source-org").await;
    assert_eq!(status, StatusCode::CREATED);
    let source_org_id = org_body["id"].as_str().expect("source org id");

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "Source Crates",
        "source-crates",
        Some(source_org_id),
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, package_body) = create_package_with_options(
        &app,
        &alice_jwt,
        "cargo",
        "native_release_crate",
        "source-crates",
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );

    let (status, _) = add_org_member(&app, &alice_jwt, "source-org", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_team(
        &app,
        &alice_jwt,
        "source-org",
        "Crate Release Engineering",
        "crate-release-engineering",
        Some("Publishes existing cargo crates via repository delegation."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = add_team_member_to_team(
        &app,
        &alice_jwt,
        "source-org",
        "crate-release-engineering",
        "bob",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, grant_body) = grant_team_repository_access(
        &app,
        &alice_jwt,
        "source-org",
        "crate-release-engineering",
        "source-crates",
        &["publish"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected team repository access response: {grant_body}"
    );

    let (status, token_body) =
        create_personal_access_token(&app, &bob_jwt, "bob-cargo-publish", &["packages:write"])
            .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected token response: {token_body}"
    );
    let cargo_token = token_body["token"]
        .as_str()
        .expect("cargo token should be returned")
        .to_owned();

    let crate_bytes = b"native-release-crate-tarball";
    let payload = build_cargo_publish_payload(
        json!({
            "name": "native_release_crate",
            "vers": "1.0.0",
            "deps": [],
            "features": {},
            "authors": ["Bob <bob@test.dev>"],
            "description": "Repository-delegated cargo publish for existing package",
            "license": "MIT"
        }),
        crate_bytes,
    );

    let (status, publish_body) = publish_cargo_crate(&app, &cargo_token, payload).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected cargo publish response: {publish_body}"
    );

    let release_status: String = sqlx::query_scalar(
        "SELECT r.status::text \
         FROM releases r \
         JOIN packages p ON p.id = r.package_id \
         WHERE p.ecosystem = 'cargo' AND p.normalized_name = $1 AND r.version = $2",
    )
    .bind("native_release_crate")
    .bind("1.0.0")
    .fetch_one(&pool)
    .await
    .expect("cargo release status should be queryable");
    assert_eq!(release_status, "published");

    let (status, detail_after_publish) =
        get_package_detail(&app, Some(&bob_jwt), "cargo", "native_release_crate").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail_after_publish["can_manage_releases"], true);
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_native_pypi_upload_auto_creates_org_owned_package_from_repository_publish_grant(
    pool: PgPool,
) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &alice_jwt, "Source Org", "source-org").await;
    assert_eq!(status, StatusCode::CREATED);
    let source_org_id = org_body["id"].as_str().expect("source org id");
    let source_org_uuid = uuid::Uuid::parse_str(source_org_id).expect("source org id should parse");

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "Source Python Packages",
        "source-py-packages",
        Some(source_org_id),
        Some("private"),
        Some("private"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, _) = add_org_member(&app, &alice_jwt, "source-org", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_team(
        &app,
        &alice_jwt,
        "source-org",
        "PyPI Release Engineering",
        "pypi-release-engineering",
        Some("Publishes PyPI distributions through repository delegation."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = add_team_member_to_team(
        &app,
        &alice_jwt,
        "source-org",
        "pypi-release-engineering",
        "bob",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, grant_body) = grant_team_repository_access(
        &app,
        &alice_jwt,
        "source-org",
        "pypi-release-engineering",
        "source-py-packages",
        &["publish"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected team repository access response: {grant_body}"
    );

    let (status, token_body) =
        create_personal_access_token(&app, &bob_jwt, "bob-pypi-publish", &["packages:write"]).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected token response: {token_body}"
    );
    let pypi_token = token_body["token"]
        .as_str()
        .expect("pypi token should be returned")
        .to_owned();

    let artifact_bytes = b"fake-sdist-bytes-for-delegated-org-package";
    let (status, upload_body) = upload_pypi_distribution(
        &app,
        &pypi_token,
        None,
        "native-org-widget",
        "1.0.0",
        artifact_bytes,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected pypi upload response: {upload_body}"
    );

    let package_owner = sqlx::query(
        "SELECT owner_user_id, owner_org_id, visibility::text AS visibility \
         FROM packages \
         WHERE ecosystem = 'pypi' AND normalized_name = $1",
    )
    .bind("native-org-widget")
    .fetch_one(&pool)
    .await
    .expect("native pypi package should exist");
    let package_owner_user_id = package_owner
        .try_get::<Option<uuid::Uuid>, _>("owner_user_id")
        .expect("package owner user id should be readable");
    let package_owner_org_id = package_owner
        .try_get::<Option<uuid::Uuid>, _>("owner_org_id")
        .expect("package owner org id should be readable");
    let package_visibility = package_owner
        .try_get::<String, _>("visibility")
        .expect("package visibility should be readable");
    assert_eq!(package_owner_user_id, None);
    assert_eq!(package_owner_org_id, Some(source_org_uuid));
    assert_eq!(package_visibility, "private");

    let (status, package_detail) =
        get_package_detail(&app, Some(&bob_jwt), "pypi", "native-org-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(package_detail["owner_org_slug"], "source-org");
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_native_pypi_repository_publish_permission_allows_existing_package_upload(
    pool: PgPool,
) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &alice_jwt, "Source Org", "source-org").await;
    assert_eq!(status, StatusCode::CREATED);
    let source_org_id = org_body["id"].as_str().expect("source org id");

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "Source Python Packages",
        "source-py-packages",
        Some(source_org_id),
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, package_body) = create_package_with_options(
        &app,
        &alice_jwt,
        "pypi",
        "native-release-widget",
        "source-py-packages",
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );

    let (status, _) = add_org_member(&app, &alice_jwt, "source-org", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_team(
        &app,
        &alice_jwt,
        "source-org",
        "PyPI Release Engineering",
        "pypi-release-engineering",
        Some("Publishes existing PyPI distributions through repository delegation."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = add_team_member_to_team(
        &app,
        &alice_jwt,
        "source-org",
        "pypi-release-engineering",
        "bob",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, grant_body) = grant_team_repository_access(
        &app,
        &alice_jwt,
        "source-org",
        "pypi-release-engineering",
        "source-py-packages",
        &["publish"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected team repository access response: {grant_body}"
    );

    let (status, token_body) =
        create_personal_access_token(&app, &bob_jwt, "bob-pypi-publish", &["packages:write"]).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected token response: {token_body}"
    );
    let pypi_token = token_body["token"]
        .as_str()
        .expect("pypi token should be returned")
        .to_owned();

    let artifact_bytes = b"fake-sdist-bytes-for-delegated-existing-package";
    let (status, upload_body) = upload_pypi_distribution(
        &app,
        &pypi_token,
        None,
        "native-release-widget",
        "1.0.0",
        artifact_bytes,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected pypi upload response: {upload_body}"
    );

    let release_status: String = sqlx::query_scalar(
        "SELECT r.status::text \
         FROM releases r \
         JOIN packages p ON p.id = r.package_id \
         WHERE p.ecosystem = 'pypi' AND p.normalized_name = $1 AND r.version = $2",
    )
    .bind("native-release-widget")
    .bind("1.0.0")
    .fetch_one(&pool)
    .await
    .expect("pypi release status should be queryable");
    assert_eq!(release_status, "published");

    let (status, detail_after_publish) =
        get_package_detail(&app, Some(&bob_jwt), "pypi", "native-release-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail_after_publish["can_manage_releases"], true);
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_native_pypi_simple_api_projects_requires_python_and_dependency_metadata(
    pool: PgPool,
) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "Alice PyPI Packages",
        "alice-pypi-packages",
        None,
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, token_body) =
        create_personal_access_token(&app, &alice_jwt, "alice-pypi-publish", &["packages:write"])
            .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected token response: {token_body}"
    );
    let pypi_token = token_body["token"]
        .as_str()
        .expect("pypi token should be returned")
        .to_owned();

    let artifact_bytes = b"fake-sdist-bytes-for-pypi-metadata";
    let (status, upload_body) = upload_pypi_distribution_with_fields(
        &app,
        &pypi_token,
        Some("alice-pypi-packages"),
        "native-metadata-widget",
        "1.0.0",
        artifact_bytes,
        &[
            ("requires_python", ">=3.10"),
            ("requires_dist", "requests>=2.31"),
            ("requires_dist", "urllib3>=2"),
            ("requires_external", "libssl"),
            ("provides_extra", "s3"),
        ],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected pypi upload response: {upload_body}"
    );

    let release_id = get_release_id(&pool, "pypi", "native-metadata-widget", "1.0.0").await;
    let metadata_row = sqlx::query(
        "SELECT requires_python, requires_dist, requires_external, provides_extra
         FROM pypi_release_metadata
         WHERE release_id = $1",
    )
    .bind(release_id)
    .fetch_one(&pool)
    .await
    .expect("pypi release metadata should be stored");
    let requires_python = metadata_row
        .try_get::<Option<String>, _>("requires_python")
        .expect("requires_python should be readable");
    let requires_dist = metadata_row
        .try_get::<Option<Vec<String>>, _>("requires_dist")
        .expect("requires_dist should be readable")
        .expect("requires_dist should be present");
    let requires_external = metadata_row
        .try_get::<Option<Vec<String>>, _>("requires_external")
        .expect("requires_external should be readable")
        .expect("requires_external should be present");
    let provides_extra = metadata_row
        .try_get::<Option<Vec<String>>, _>("provides_extra")
        .expect("provides_extra should be readable")
        .expect("provides_extra should be present");
    assert_eq!(requires_python.as_deref(), Some(">=3.10"));
    assert_eq!(
        requires_dist,
        vec!["requests>=2.31".to_owned(), "urllib3>=2".to_owned()]
    );
    assert_eq!(requires_external, vec!["libssl".to_owned()]);
    assert_eq!(provides_extra, vec!["s3".to_owned()]);

    let (status, simple_json) =
        get_pypi_simple_project_json(&app, None, "native-metadata-widget").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected simple api response: {simple_json}"
    );
    assert_eq!(simple_json["name"], "native-metadata-widget");
    assert_eq!(simple_json["files"][0]["requires-python"], ">=3.10");
    assert_eq!(
        simple_json["files"][0]["requires-dist"],
        json!(["requests>=2.31", "urllib3>=2"])
    );
    assert_eq!(
        simple_json["files"][0]["requires-external"],
        json!(["libssl"])
    );
    assert_eq!(simple_json["files"][0]["provides-extra"], json!(["s3"]));

    let (status, simple_html) =
        get_pypi_simple_project_html(&app, None, "native-metadata-widget").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected simple html response: {simple_html}"
    );
    assert!(simple_html.contains("data-requires-python=\"&gt;=3.10\""));
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_native_pypi_retry_preserves_existing_requires_python_metadata(pool: PgPool) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "Alice PyPI Packages",
        "alice-pypi-packages",
        None,
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, token_body) =
        create_personal_access_token(&app, &alice_jwt, "alice-pypi-publish", &["packages:write"])
            .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected token response: {token_body}"
    );
    let pypi_token = token_body["token"]
        .as_str()
        .expect("pypi token should be returned")
        .to_owned();

    let artifact_bytes = b"fake-sdist-bytes-for-pypi-idempotent-retry";
    let (status, first_upload_body) = upload_pypi_distribution_with_fields(
        &app,
        &pypi_token,
        Some("alice-pypi-packages"),
        "native-retry-widget",
        "1.0.0",
        artifact_bytes,
        &[("requires_python", ">=3.11")],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected first pypi upload response: {first_upload_body}"
    );

    let (status, retry_body) = upload_pypi_distribution(
        &app,
        &pypi_token,
        Some("alice-pypi-packages"),
        "native-retry-widget",
        "1.0.0",
        artifact_bytes,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected retry pypi upload response: {retry_body}"
    );

    let (status, simple_json) =
        get_pypi_simple_project_json(&app, None, "native-retry-widget").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected simple api response: {simple_json}"
    );
    assert_eq!(simple_json["files"][0]["requires-python"], ">=3.11");
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_native_pypi_simple_api_omits_missing_resolver_metadata(pool: PgPool) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "Alice PyPI Packages",
        "alice-pypi-packages",
        None,
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, token_body) =
        create_personal_access_token(&app, &alice_jwt, "alice-pypi-publish", &["packages:write"])
            .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected token response: {token_body}"
    );
    let pypi_token = token_body["token"]
        .as_str()
        .expect("pypi token should be returned")
        .to_owned();

    let artifact_bytes = b"fake-sdist-bytes-without-pypi-resolver-metadata";
    let (status, upload_body) = upload_pypi_distribution(
        &app,
        &pypi_token,
        Some("alice-pypi-packages"),
        "native-null-metadata-widget",
        "1.0.0",
        artifact_bytes,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected pypi upload response: {upload_body}"
    );

    let release_id = get_release_id(&pool, "pypi", "native-null-metadata-widget", "1.0.0").await;
    let metadata_row = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM pypi_release_metadata WHERE release_id = $1",
    )
    .bind(release_id)
    .fetch_one(&pool)
    .await
    .expect("pypi metadata count should be queryable");
    assert_eq!(
        metadata_row, 0,
        "empty uploads should not create metadata rows"
    );

    let (status, simple_json) =
        get_pypi_simple_project_json(&app, None, "native-null-metadata-widget").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected simple api response: {simple_json}"
    );
    assert!(simple_json["files"][0].get("requires-python").is_none());
    assert!(simple_json["files"][0].get("requires-dist").is_none());
    assert!(simple_json["files"][0].get("requires-external").is_none());
    assert!(simple_json["files"][0].get("provides-extra").is_none());

    let (status, simple_html) =
        get_pypi_simple_project_html(&app, None, "native-null-metadata-widget").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected simple html response: {simple_html}"
    );
    assert!(!simple_html.contains("data-requires-python"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_native_composer_publish_auto_creates_org_owned_package_from_repository_publish_grant(
    pool: PgPool,
) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &alice_jwt, "Source Org", "source-org").await;
    assert_eq!(status, StatusCode::CREATED);
    let source_org_id = org_body["id"].as_str().expect("source org id");
    let source_org_uuid = uuid::Uuid::parse_str(source_org_id).expect("source org id should parse");

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "Source Composer Packages",
        "source-composer-packages",
        Some(source_org_id),
        Some("private"),
        Some("private"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, _) = add_org_member(&app, &alice_jwt, "source-org", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_team(
        &app,
        &alice_jwt,
        "source-org",
        "Composer Release Engineering",
        "composer-release-engineering",
        Some("Publishes Composer packages through repository delegation."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = add_team_member_to_team(
        &app,
        &alice_jwt,
        "source-org",
        "composer-release-engineering",
        "bob",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, grant_body) = grant_team_repository_access(
        &app,
        &alice_jwt,
        "source-org",
        "composer-release-engineering",
        "source-composer-packages",
        &["publish"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected team repository access response: {grant_body}"
    );

    let (status, token_body) =
        create_personal_access_token(&app, &bob_jwt, "bob-composer-publish", &["packages:write"])
            .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected token response: {token_body}"
    );
    let composer_token = token_body["token"]
        .as_str()
        .expect("composer token should be returned")
        .to_owned();

    let package_name = "acme/native-org-widget";
    let artifact_bytes = b"fake-composer-zip-for-delegated-org-package";
    let (status, publish_body) =
        publish_composer_package(&app, &composer_token, package_name, "1.0.0", artifact_bytes)
            .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected composer publish response: {publish_body}"
    );
    assert_eq!(publish_body["ok"], true);
    assert_eq!(publish_body["name"], package_name);

    let package_owner = sqlx::query(
        "SELECT owner_user_id, owner_org_id, visibility::text AS visibility \
         FROM packages \
         WHERE ecosystem = 'composer' AND name = $1",
    )
    .bind(package_name)
    .fetch_one(&pool)
    .await
    .expect("native composer package should exist");
    let package_owner_user_id = package_owner
        .try_get::<Option<uuid::Uuid>, _>("owner_user_id")
        .expect("package owner user id should be readable");
    let package_owner_org_id = package_owner
        .try_get::<Option<uuid::Uuid>, _>("owner_org_id")
        .expect("package owner org id should be readable");
    let package_visibility = package_owner
        .try_get::<String, _>("visibility")
        .expect("package visibility should be readable");
    assert_eq!(package_owner_user_id, None);
    assert_eq!(package_owner_org_id, Some(source_org_uuid));
    assert_eq!(package_visibility, "private");

    let release_status: String = sqlx::query_scalar(
        "SELECT r.status::text \
         FROM releases r \
         JOIN packages p ON p.id = r.package_id \
         WHERE p.ecosystem = 'composer' AND p.name = $1 AND r.version = $2",
    )
    .bind(package_name)
    .bind("1.0.0")
    .fetch_one(&pool)
    .await
    .expect("composer release status should be queryable");
    assert_eq!(release_status, "published");

    let (status, metadata_body) =
        get_composer_package_metadata(&app, Some(&composer_token), "acme", "native-org-widget")
            .await;
    assert_eq!(status, StatusCode::OK);
    let versions = metadata_body["packages"][package_name]
        .as_array()
        .expect("composer metadata versions should be an array");
    assert_eq!(versions.len(), 1, "response: {metadata_body}");
    assert_eq!(versions[0]["version"], "1.0.0");
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_native_composer_repository_publish_permission_allows_existing_package_publish_and_yank_keeps_exact_downloads(
    pool: PgPool,
) {
    use sha2::{Digest, Sha256};

    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &alice_jwt, "Source Org", "source-org").await;
    assert_eq!(status, StatusCode::CREATED);
    let source_org_id = org_body["id"].as_str().expect("source org id");

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "Source Composer Packages",
        "source-composer-packages",
        Some(source_org_id),
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let package_name = "acme/native-release-widget";
    let (status, package_body) = create_package_with_options(
        &app,
        &alice_jwt,
        "composer",
        package_name,
        "source-composer-packages",
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );

    let (status, _) = add_org_member(&app, &alice_jwt, "source-org", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_team(
        &app,
        &alice_jwt,
        "source-org",
        "Composer Release Engineering",
        "composer-release-engineering",
        Some("Publishes existing Composer packages and yanks releases."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = add_team_member_to_team(
        &app,
        &alice_jwt,
        "source-org",
        "composer-release-engineering",
        "bob",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, grant_body) = grant_team_repository_access(
        &app,
        &alice_jwt,
        "source-org",
        "composer-release-engineering",
        "source-composer-packages",
        &["publish"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected team repository access response: {grant_body}"
    );

    let (status, token_body) =
        create_personal_access_token(&app, &bob_jwt, "bob-composer-publish", &["packages:write"])
            .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected token response: {token_body}"
    );
    let composer_token = token_body["token"]
        .as_str()
        .expect("composer token should be returned")
        .to_owned();

    let artifact_bytes = b"fake-composer-zip-for-existing-package";
    let artifact_sha256 = hex::encode(Sha256::digest(artifact_bytes));
    let (status, publish_body) =
        publish_composer_package(&app, &composer_token, package_name, "1.0.0", artifact_bytes)
            .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected composer publish response: {publish_body}"
    );
    assert_eq!(publish_body["ok"], true);

    let release_status: String = sqlx::query_scalar(
        "SELECT r.status::text \
         FROM releases r \
         JOIN packages p ON p.id = r.package_id \
         WHERE p.ecosystem = 'composer' AND p.name = $1 AND r.version = $2",
    )
    .bind(package_name)
    .bind("1.0.0")
    .fetch_one(&pool)
    .await
    .expect("composer release status should be queryable");
    assert_eq!(release_status, "published");

    let (status, metadata_body) =
        get_composer_package_metadata(&app, None, "acme", "native-release-widget").await;
    assert_eq!(status, StatusCode::OK);
    let versions = metadata_body["packages"][package_name]
        .as_array()
        .expect("composer metadata versions should be an array");
    assert_eq!(versions.len(), 1, "response: {metadata_body}");
    assert_eq!(versions[0]["version"], "1.0.0");

    let dist_url = versions[0]["dist"]["url"]
        .as_str()
        .expect("composer dist url should be present")
        .to_owned();
    let download_path = Url::parse(&dist_url)
        .expect("composer dist url should parse")
        .path()
        .to_owned();

    let download_req = Request::builder()
        .method(Method::GET)
        .uri(download_path.clone())
        .body(Body::empty())
        .unwrap();
    let download_resp = app.clone().oneshot(download_req).await.unwrap();
    assert_eq!(download_resp.status(), StatusCode::OK);
    let download_headers = download_resp.headers().clone();
    let download_body = body_bytes(download_resp).await;
    assert_eq!(download_body, artifact_bytes);
    assert_eq!(
        download_headers
            .get("x-checksum-sha256")
            .expect("download checksum header should exist"),
        artifact_sha256.as_str()
    );

    let (status, yank_body) = yank_composer_package_version(
        &app,
        &composer_token,
        "acme",
        "native-release-widget",
        "1.0.0",
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected composer yank response: {yank_body}"
    );
    assert_eq!(yank_body["ok"], true);

    let release_status_after_yank: String = sqlx::query_scalar(
        "SELECT r.status::text \
         FROM releases r \
         JOIN packages p ON p.id = r.package_id \
         WHERE p.ecosystem = 'composer' AND p.name = $1 AND r.version = $2",
    )
    .bind(package_name)
    .bind("1.0.0")
    .fetch_one(&pool)
    .await
    .expect("composer release status after yank should be queryable");
    assert_eq!(release_status_after_yank, "yanked");

    let (status, metadata_after_yank) =
        get_composer_package_metadata(&app, None, "acme", "native-release-widget").await;
    assert_eq!(status, StatusCode::OK);
    let versions_after_yank = metadata_after_yank["packages"][package_name]
        .as_array()
        .expect("composer metadata versions should be an array after yank");
    assert!(
        versions_after_yank.is_empty(),
        "yanked releases should be hidden from metadata: {metadata_after_yank}"
    );
    let download_after_yank_req = Request::builder()
        .method(Method::GET)
        .uri(download_path)
        .body(Body::empty())
        .unwrap();
    let download_after_yank_resp = app.clone().oneshot(download_after_yank_req).await.unwrap();
    assert_eq!(download_after_yank_resp.status(), StatusCode::OK);
    let download_after_yank_body = body_bytes(download_after_yank_resp).await;
    assert_eq!(download_after_yank_body, artifact_bytes);
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_native_maven_deploy_auto_creates_org_owned_package_from_repository_publish_grant_and_pom_finalizes_release(
    pool: PgPool,
) {
    use sha2::{Digest, Sha256};

    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &alice_jwt, "Source Org", "source-org").await;
    assert_eq!(status, StatusCode::CREATED);
    let source_org_id = org_body["id"].as_str().expect("source org id");
    let source_org_uuid = uuid::Uuid::parse_str(source_org_id).expect("source org id should parse");

    let (status, namespace_body) =
        create_namespace_claim(&app, &alice_jwt, "maven", "com.acme", Some(source_org_id)).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected namespace claim response: {namespace_body}"
    );

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "Source Maven Packages",
        "source-maven-packages",
        Some(source_org_id),
        Some("private"),
        Some("private"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, _) = add_org_member(&app, &alice_jwt, "source-org", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_team(
        &app,
        &alice_jwt,
        "source-org",
        "Maven Release Engineering",
        "maven-release-engineering",
        Some("Publishes Maven packages through repository delegation."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = add_team_member_to_team(
        &app,
        &alice_jwt,
        "source-org",
        "maven-release-engineering",
        "bob",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, grant_body) = grant_team_repository_access(
        &app,
        &alice_jwt,
        "source-org",
        "maven-release-engineering",
        "source-maven-packages",
        &["publish"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected team repository access response: {grant_body}"
    );

    let (status, token_body) =
        create_personal_access_token(&app, &bob_jwt, "bob-maven-publish", &["packages:write"])
            .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected token response: {token_body}"
    );
    let maven_token = token_body["token"]
        .as_str()
        .expect("maven token should be returned")
        .to_owned();

    let jar_bytes = b"fake-maven-jar-for-delegated-org-package";
    let jar_sha256 = hex::encode(Sha256::digest(jar_bytes));
    let (status, jar_body) = upload_maven_artifact(
        &app,
        &maven_token,
        "com/acme",
        "native-org-widget",
        "1.0.0",
        "native-org-widget-1.0.0.jar",
        "application/java-archive",
        jar_bytes,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected maven jar upload response: {jar_body}"
    );

    let release_status_before_pom: String = sqlx::query_scalar(
        "SELECT r.status::text \
         FROM releases r \
         JOIN packages p ON p.id = r.package_id \
         WHERE p.ecosystem = 'maven' AND p.name = $1 AND r.version = $2",
    )
    .bind("com.acme:native-org-widget")
    .bind("1.0.0")
    .fetch_one(&pool)
    .await
    .expect("maven release status before pom should be queryable");
    assert_eq!(release_status_before_pom, "quarantine");

    let pom_bytes = build_maven_pom("com.acme", "native-org-widget", "1.0.0");
    let (status, pom_body) = upload_maven_artifact(
        &app,
        &maven_token,
        "com/acme",
        "native-org-widget",
        "1.0.0",
        "native-org-widget-1.0.0.pom",
        "application/xml",
        &pom_bytes,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected maven pom upload response: {pom_body}"
    );

    let package_owner = sqlx::query(
        "SELECT owner_user_id, owner_org_id, visibility AS visibility \
         FROM packages \
         WHERE ecosystem = 'maven' AND name = $1",
    )
    .bind("com.acme:native-org-widget")
    .fetch_one(&pool)
    .await
    .expect("native maven package should exist");
    let package_owner_user_id = package_owner
        .try_get::<Option<uuid::Uuid>, _>("owner_user_id")
        .expect("package owner user id should be readable");
    let package_owner_org_id = package_owner
        .try_get::<Option<uuid::Uuid>, _>("owner_org_id")
        .expect("package owner org id should be readable");
    let package_visibility = package_owner
        .try_get::<String, _>("visibility")
        .expect("package visibility should be readable");
    assert_eq!(package_owner_user_id, None);
    assert_eq!(package_owner_org_id, Some(source_org_uuid));
    assert_eq!(package_visibility, "private");

    let release_status_after_pom: String = sqlx::query_scalar(
        "SELECT r.status::text \
         FROM releases r \
         JOIN packages p ON p.id = r.package_id \
         WHERE p.ecosystem = 'maven' AND p.name = $1 AND r.version = $2",
    )
    .bind("com.acme:native-org-widget")
    .bind("1.0.0")
    .fetch_one(&pool)
    .await
    .expect("maven release status after pom should be queryable");
    assert_eq!(release_status_after_pom, "published");

    let metadata_req = Request::builder()
        .method(Method::GET)
        .uri("/maven/com/acme/native-org-widget/maven-metadata.xml")
        .header(header::AUTHORIZATION, format!("Bearer {maven_token}"))
        .body(Body::empty())
        .unwrap();
    let metadata_resp = app.clone().oneshot(metadata_req).await.unwrap();
    assert_eq!(metadata_resp.status(), StatusCode::OK);
    let metadata_text = body_text(metadata_resp).await;
    assert!(metadata_text.contains("<version>1.0.0</version>"));

    let checksum_req = Request::builder()
        .method(Method::GET)
        .uri("/maven/com/acme/native-org-widget/1.0.0/native-org-widget-1.0.0.jar.sha256")
        .header(header::AUTHORIZATION, format!("Bearer {maven_token}"))
        .body(Body::empty())
        .unwrap();
    let checksum_resp = app.clone().oneshot(checksum_req).await.unwrap();
    assert_eq!(checksum_resp.status(), StatusCode::OK);
    let checksum_text = body_text(checksum_resp).await;
    assert_eq!(checksum_text.trim(), jar_sha256);

    let download_req = Request::builder()
        .method(Method::GET)
        .uri("/maven/com/acme/native-org-widget/1.0.0/native-org-widget-1.0.0.jar")
        .header(header::AUTHORIZATION, format!("Bearer {maven_token}"))
        .body(Body::empty())
        .unwrap();
    let download_resp = app.clone().oneshot(download_req).await.unwrap();
    assert_eq!(download_resp.status(), StatusCode::OK);
    let download_body = body_bytes(download_resp).await;
    assert_eq!(download_body, jar_bytes);
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_native_maven_repository_publish_permission_allows_existing_package_publish_and_follow_up_artifacts_within_grace_window(
    pool: PgPool,
) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &alice_jwt, "Source Org", "source-org").await;
    assert_eq!(status, StatusCode::CREATED);
    let source_org_id = org_body["id"].as_str().expect("source org id");

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "Source Maven Packages",
        "source-maven-packages",
        Some(source_org_id),
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, package_body) = create_package_with_options(
        &app,
        &alice_jwt,
        "maven",
        "com.acme:native-release-widget",
        "source-maven-packages",
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );

    let (status, _) = add_org_member(&app, &alice_jwt, "source-org", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_team(
        &app,
        &alice_jwt,
        "source-org",
        "Maven Release Engineering",
        "maven-release-engineering",
        Some("Publishes existing Maven packages and attaches follow-up artifacts."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = add_team_member_to_team(
        &app,
        &alice_jwt,
        "source-org",
        "maven-release-engineering",
        "bob",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, grant_body) = grant_team_repository_access(
        &app,
        &alice_jwt,
        "source-org",
        "maven-release-engineering",
        "source-maven-packages",
        &["publish"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected team repository access response: {grant_body}"
    );

    let (status, token_body) =
        create_personal_access_token(&app, &bob_jwt, "bob-maven-publish", &["packages:write"])
            .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected token response: {token_body}"
    );
    let maven_token = token_body["token"]
        .as_str()
        .expect("maven token should be returned")
        .to_owned();

    let pom_bytes = build_maven_pom("com.acme", "native-release-widget", "1.0.0");
    let (status, pom_body) = upload_maven_artifact(
        &app,
        &maven_token,
        "com/acme",
        "native-release-widget",
        "1.0.0",
        "native-release-widget-1.0.0.pom",
        "application/xml",
        &pom_bytes,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected maven pom upload response: {pom_body}"
    );

    let release_status: String = sqlx::query_scalar(
        "SELECT r.status::text \
         FROM releases r \
         JOIN packages p ON p.id = r.package_id \
         WHERE p.ecosystem = 'maven' AND p.name = $1 AND r.version = $2",
    )
    .bind("com.acme:native-release-widget")
    .bind("1.0.0")
    .fetch_one(&pool)
    .await
    .expect("maven release status should be queryable");
    assert_eq!(release_status, "published");

    let (status, checksum_body) = upload_maven_artifact(
        &app,
        &maven_token,
        "com/acme",
        "native-release-widget",
        "1.0.0",
        "native-release-widget-1.0.0-sources.jar.sha256",
        "text/plain; charset=utf-8",
        b"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\n",
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "unexpected checksum-before-target response: {checksum_body}"
    );

    let sources_bytes = b"fake-maven-sources-jar";
    let (status, sources_body) = upload_maven_artifact(
        &app,
        &maven_token,
        "com/acme",
        "native-release-widget",
        "1.0.0",
        "native-release-widget-1.0.0-sources.jar",
        "application/java-archive",
        sources_bytes,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected maven sources upload response: {sources_body}"
    );

    let (status, signature_body) = upload_maven_artifact(
        &app,
        &maven_token,
        "com/acme",
        "native-release-widget",
        "1.0.0",
        "native-release-widget-1.0.0-sources.jar.asc",
        "application/pgp-signature",
        b"fake-maven-signature",
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected maven signature upload response: {signature_body}"
    );

    let metadata_req = Request::builder()
        .method(Method::GET)
        .uri("/maven/com/acme/native-release-widget/maven-metadata.xml")
        .body(Body::empty())
        .unwrap();
    let metadata_resp = app.clone().oneshot(metadata_req).await.unwrap();
    assert_eq!(metadata_resp.status(), StatusCode::OK);
    let metadata_text = body_text(metadata_resp).await;
    assert!(metadata_text.contains("<version>1.0.0</version>"));

    let download_req = Request::builder()
        .method(Method::GET)
        .uri("/maven/com/acme/native-release-widget/1.0.0/native-release-widget-1.0.0-sources.jar")
        .body(Body::empty())
        .unwrap();
    let download_resp = app.clone().oneshot(download_req).await.unwrap();
    assert_eq!(download_resp.status(), StatusCode::OK);
    let download_body = body_bytes(download_resp).await;
    assert_eq!(download_body, sources_bytes);
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_native_maven_deploy_rejects_snapshots(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, repository_body) = create_repository(
        &app,
        &alice_jwt,
        "Personal Maven Packages",
        "personal-maven-packages",
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, token_body) =
        create_personal_access_token(&app, &alice_jwt, "alice-maven-publish", &["packages:write"])
            .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected token response: {token_body}"
    );
    let maven_token = token_body["token"]
        .as_str()
        .expect("maven token should be returned")
        .to_owned();

    let pom_bytes = build_maven_pom("com.acme", "snapshot-widget", "1.0.0-SNAPSHOT");
    let (status, snapshot_body) = upload_maven_artifact(
        &app,
        &maven_token,
        "com/acme",
        "snapshot-widget",
        "1.0.0-SNAPSHOT",
        "snapshot-widget-1.0.0-SNAPSHOT.pom",
        "application/xml",
        &pom_bytes,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "unexpected snapshot rejection response: {snapshot_body}"
    );
    assert!(snapshot_body
        .as_str()
        .expect("snapshot rejection should return a plain-text message")
        .contains("Snapshots are not supported"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_native_oci_push_auto_creates_org_owned_package_from_repository_publish_grant_and_private_pull_requires_auth(
    pool: PgPool,
) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &alice_jwt, "Source Org", "source-org").await;
    assert_eq!(status, StatusCode::CREATED);
    let source_org_id = org_body["id"].as_str().expect("source org id");
    let source_org_uuid = uuid::Uuid::parse_str(source_org_id).expect("source org id should parse");

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "Source OCI Packages",
        "source-oci-packages",
        Some(source_org_id),
        Some("private"),
        Some("private"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, _) = add_org_member(&app, &alice_jwt, "source-org", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_team(
        &app,
        &alice_jwt,
        "source-org",
        "OCI Release Engineering",
        "oci-release-engineering",
        Some("Publishes OCI artifacts through repository delegation."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = add_team_member_to_team(
        &app,
        &alice_jwt,
        "source-org",
        "oci-release-engineering",
        "bob",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, grant_body) = grant_team_repository_access(
        &app,
        &alice_jwt,
        "source-org",
        "oci-release-engineering",
        "source-oci-packages",
        &["publish"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected team repository access response: {grant_body}"
    );

    let (status, token_body) =
        create_personal_access_token(&app, &bob_jwt, "bob-oci-publish", &["packages:write"]).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected token response: {token_body}"
    );
    let oci_token = token_body["token"]
        .as_str()
        .expect("oci token should be returned")
        .to_owned();

    let package_name = "acme/native-org-widget";
    let config_bytes =
        br#"{"architecture":"amd64","os":"linux","rootfs":{"type":"layers","diff_ids":[]}}"#;
    let layer_bytes = b"fake-oci-layer-for-delegated-org-package";

    let (config_digest, config_resp) =
        upload_oci_blob_monolithic(&app, &oci_token, package_name, config_bytes).await;
    assert_eq!(config_resp.status(), StatusCode::CREATED);

    let (layer_digest, layer_resp) =
        upload_oci_blob_monolithic(&app, &oci_token, package_name, layer_bytes).await;
    assert_eq!(layer_resp.status(), StatusCode::CREATED);

    let manifest_bytes = build_oci_image_manifest(
        &config_digest,
        config_bytes.len(),
        &layer_digest,
        layer_bytes.len(),
    );
    let manifest_digest = oci_digest(&manifest_bytes);
    let manifest_resp = send_oci_request(
        &app,
        Method::PUT,
        format!("/oci/v2/{package_name}/manifests/latest"),
        Some(&oci_token),
        Some("application/vnd.oci.image.manifest.v1+json"),
        manifest_bytes.clone(),
    )
    .await;
    assert_eq!(manifest_resp.status(), StatusCode::CREATED);
    assert_eq!(
        manifest_resp
            .headers()
            .get("docker-content-digest")
            .expect("manifest response should include digest header"),
        manifest_digest.as_str()
    );

    let package_owner = sqlx::query(
        "SELECT owner_user_id, owner_org_id, visibility::text AS visibility \
         FROM packages \
         WHERE ecosystem = 'oci' AND name = $1",
    )
    .bind(package_name)
    .fetch_one(&pool)
    .await
    .expect("native oci package should exist");
    let package_owner_user_id = package_owner
        .try_get::<Option<uuid::Uuid>, _>("owner_user_id")
        .expect("package owner user id should be readable");
    let package_owner_org_id = package_owner
        .try_get::<Option<uuid::Uuid>, _>("owner_org_id")
        .expect("package owner org id should be readable");
    let package_visibility = package_owner
        .try_get::<String, _>("visibility")
        .expect("package visibility should be readable");
    assert_eq!(package_owner_user_id, None);
    assert_eq!(package_owner_org_id, Some(source_org_uuid));
    assert_eq!(package_visibility, "private");

    let release_status: String = sqlx::query_scalar(
        "SELECT r.status::text \
         FROM releases r \
         JOIN packages p ON p.id = r.package_id \
         WHERE p.ecosystem = 'oci' AND p.name = $1 AND r.version = $2",
    )
    .bind(package_name)
    .bind(&manifest_digest)
    .fetch_one(&pool)
    .await
    .expect("oci release status should be queryable");
    assert_eq!(release_status, "published");

    let probe_resp =
        send_oci_request(&app, Method::GET, "/oci/v2/".to_owned(), None, None, vec![]).await;
    assert_eq!(probe_resp.status(), StatusCode::UNAUTHORIZED);
    assert!(probe_resp
        .headers()
        .get(header::WWW_AUTHENTICATE)
        .expect("probe challenge should exist")
        .to_str()
        .expect("probe challenge should be utf-8")
        .contains("registry:catalog:*"));

    let tags_challenge_resp = send_oci_request(
        &app,
        Method::GET,
        format!("/oci/v2/{package_name}/tags/list"),
        None,
        None,
        vec![],
    )
    .await;
    assert_eq!(tags_challenge_resp.status(), StatusCode::UNAUTHORIZED);
    assert!(tags_challenge_resp
        .headers()
        .get(header::WWW_AUTHENTICATE)
        .expect("tags challenge should exist")
        .to_str()
        .expect("tags challenge should be utf-8")
        .contains(&format!("repository:{package_name}:pull")));

    let tags_resp = send_oci_request(
        &app,
        Method::GET,
        format!("/oci/v2/{package_name}/tags/list"),
        Some(&oci_token),
        None,
        vec![],
    )
    .await;
    assert_eq!(tags_resp.status(), StatusCode::OK);
    let tags_body = body_json(tags_resp).await;
    assert_eq!(tags_body["name"], package_name);
    assert_eq!(tags_body["tags"], json!(["latest"]));

    let catalog_resp = send_oci_request(
        &app,
        Method::GET,
        "/oci/v2/_catalog".to_owned(),
        Some(&oci_token),
        None,
        vec![],
    )
    .await;
    assert_eq!(catalog_resp.status(), StatusCode::OK);
    let catalog_body = body_json(catalog_resp).await;
    assert_eq!(catalog_body["repositories"], json!([package_name]));

    let manifest_get_resp = send_oci_request(
        &app,
        Method::GET,
        format!("/oci/v2/{package_name}/manifests/latest"),
        Some(&oci_token),
        None,
        vec![],
    )
    .await;
    assert_eq!(manifest_get_resp.status(), StatusCode::OK);
    assert_eq!(
        manifest_get_resp
            .headers()
            .get("docker-content-digest")
            .expect("manifest get should include digest header"),
        manifest_digest.as_str()
    );
    let manifest_download = body_bytes(manifest_get_resp).await;
    assert_eq!(manifest_download, manifest_bytes);

    let manifest_head_challenge_resp = send_oci_request(
        &app,
        Method::HEAD,
        format!("/oci/v2/{package_name}/manifests/latest"),
        None,
        None,
        vec![],
    )
    .await;
    assert_eq!(
        manifest_head_challenge_resp.status(),
        StatusCode::UNAUTHORIZED
    );
    assert!(manifest_head_challenge_resp
        .headers()
        .get(header::WWW_AUTHENTICATE)
        .expect("manifest head challenge should exist")
        .to_str()
        .expect("manifest head challenge should be utf-8")
        .contains(&format!("repository:{package_name}:pull")));
    assert!(body_bytes(manifest_head_challenge_resp).await.is_empty());

    let manifest_head_resp = send_oci_request(
        &app,
        Method::HEAD,
        format!("/oci/v2/{package_name}/manifests/latest"),
        Some(&oci_token),
        None,
        vec![],
    )
    .await;
    assert_eq!(manifest_head_resp.status(), StatusCode::OK);
    assert_eq!(
        manifest_head_resp
            .headers()
            .get("docker-content-digest")
            .expect("manifest head should include digest header"),
        manifest_digest.as_str()
    );
    assert_eq!(
        manifest_head_resp
            .headers()
            .get(header::CONTENT_TYPE)
            .expect("manifest head should include content type"),
        "application/vnd.oci.image.manifest.v1+json"
    );
    assert_eq!(
        manifest_head_resp
            .headers()
            .get(header::CONTENT_LENGTH)
            .expect("manifest head should include content length"),
        manifest_bytes.len().to_string().as_str()
    );
    assert!(body_bytes(manifest_head_resp).await.is_empty());

    let manifest_head_by_digest_resp = send_oci_request(
        &app,
        Method::HEAD,
        format!("/oci/v2/{package_name}/manifests/{manifest_digest}"),
        Some(&oci_token),
        None,
        vec![],
    )
    .await;
    assert_eq!(manifest_head_by_digest_resp.status(), StatusCode::OK);
    assert_eq!(
        manifest_head_by_digest_resp
            .headers()
            .get("docker-content-digest")
            .expect("manifest head by digest should include digest header"),
        manifest_digest.as_str()
    );
    assert!(body_bytes(manifest_head_by_digest_resp).await.is_empty());

    let blob_get_resp = send_oci_request(
        &app,
        Method::GET,
        format!("/oci/v2/{package_name}/blobs/{layer_digest}"),
        Some(&oci_token),
        None,
        vec![],
    )
    .await;
    assert_eq!(blob_get_resp.status(), StatusCode::OK);
    let blob_download = body_bytes(blob_get_resp).await;
    assert_eq!(blob_download, layer_bytes);

    let blob_head_resp = send_oci_request(
        &app,
        Method::HEAD,
        format!("/oci/v2/{package_name}/blobs/{layer_digest}"),
        Some(&oci_token),
        None,
        vec![],
    )
    .await;
    assert_eq!(blob_head_resp.status(), StatusCode::OK);
    assert_eq!(
        blob_head_resp
            .headers()
            .get("docker-content-digest")
            .expect("blob head should include digest header"),
        layer_digest.as_str()
    );
    assert_eq!(
        blob_head_resp
            .headers()
            .get(header::CONTENT_LENGTH)
            .expect("blob head should include content length"),
        layer_bytes.len().to_string().as_str()
    );
    assert!(body_bytes(blob_head_resp).await.is_empty());
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_native_oci_public_repository_supports_chunked_upload_anonymous_pull_and_manifest_blob_cleanup(
    pool: PgPool,
) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "Public OCI Packages",
        "public-oci-packages",
        None,
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let package_name = "acme/public-widget";
    let (status, package_body) = create_package_with_options(
        &app,
        &alice_jwt,
        "oci",
        package_name,
        "public-oci-packages",
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );

    let (status, token_body) =
        create_personal_access_token(&app, &alice_jwt, "alice-oci-publish", &["packages:write"])
            .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected token response: {token_body}"
    );
    let oci_token = token_body["token"]
        .as_str()
        .expect("oci token should be returned")
        .to_owned();

    let config_bytes =
        br#"{"architecture":"amd64","os":"linux","rootfs":{"type":"layers","diff_ids":[]}}"#;
    let (config_digest, config_resp) =
        upload_oci_blob_monolithic(&app, &oci_token, package_name, config_bytes).await;
    assert_eq!(config_resp.status(), StatusCode::CREATED);

    let layer_bytes = b"fake-oci-layer-from-chunked-upload";
    let layer_digest = oci_digest(layer_bytes);
    let start_resp = send_oci_request(
        &app,
        Method::POST,
        format!("/oci/v2/{package_name}/blobs/uploads/"),
        Some(&oci_token),
        None,
        vec![],
    )
    .await;
    assert_eq!(start_resp.status(), StatusCode::ACCEPTED);
    let upload_location = start_resp
        .headers()
        .get(header::LOCATION)
        .expect("upload start should return a location header")
        .to_str()
        .expect("upload location should be utf-8")
        .to_owned();
    let upload_path = Url::parse(&upload_location)
        .expect("upload location should parse as a URL")
        .path()
        .to_owned();

    let split_at = 10;
    let patch_resp = send_oci_request(
        &app,
        Method::PATCH,
        upload_path.clone(),
        Some(&oci_token),
        Some("application/octet-stream"),
        layer_bytes[..split_at].to_vec(),
    )
    .await;
    assert_eq!(patch_resp.status(), StatusCode::ACCEPTED);
    assert_eq!(
        patch_resp
            .headers()
            .get("range")
            .expect("upload patch should expose a range header"),
        format!("0-{}", split_at - 1).as_str()
    );

    let finalize_resp = send_oci_request(
        &app,
        Method::PUT,
        format!("{upload_path}?digest={layer_digest}"),
        Some(&oci_token),
        Some("application/octet-stream"),
        layer_bytes[split_at..].to_vec(),
    )
    .await;
    assert_eq!(finalize_resp.status(), StatusCode::CREATED);
    assert_eq!(
        finalize_resp
            .headers()
            .get("docker-content-digest")
            .expect("blob finalize should include digest header"),
        layer_digest.as_str()
    );

    let manifest_bytes = build_oci_image_manifest(
        &config_digest,
        config_bytes.len(),
        &layer_digest,
        layer_bytes.len(),
    );
    let manifest_digest = oci_digest(&manifest_bytes);
    let manifest_put_resp = send_oci_request(
        &app,
        Method::PUT,
        format!("/oci/v2/{package_name}/manifests/latest"),
        Some(&oci_token),
        Some("application/vnd.oci.image.manifest.v1+json"),
        manifest_bytes.clone(),
    )
    .await;
    assert_eq!(manifest_put_resp.status(), StatusCode::CREATED);

    let anonymous_catalog_resp = send_oci_request(
        &app,
        Method::GET,
        "/oci/v2/_catalog".to_owned(),
        None,
        None,
        vec![],
    )
    .await;
    assert_eq!(anonymous_catalog_resp.status(), StatusCode::OK);
    let anonymous_catalog_body = body_json(anonymous_catalog_resp).await;
    assert_eq!(
        anonymous_catalog_body["repositories"],
        json!([package_name])
    );

    let anonymous_tags_resp = send_oci_request(
        &app,
        Method::GET,
        format!("/oci/v2/{package_name}/tags/list"),
        None,
        None,
        vec![],
    )
    .await;
    assert_eq!(anonymous_tags_resp.status(), StatusCode::OK);
    let anonymous_tags_body = body_json(anonymous_tags_resp).await;
    assert_eq!(anonymous_tags_body["tags"], json!(["latest"]));

    let anonymous_manifest_resp = send_oci_request(
        &app,
        Method::GET,
        format!("/oci/v2/{package_name}/manifests/latest"),
        None,
        None,
        vec![],
    )
    .await;
    assert_eq!(anonymous_manifest_resp.status(), StatusCode::OK);
    assert_eq!(body_bytes(anonymous_manifest_resp).await, manifest_bytes);

    let anonymous_manifest_head_resp = send_oci_request(
        &app,
        Method::HEAD,
        format!("/oci/v2/{package_name}/manifests/latest"),
        None,
        None,
        vec![],
    )
    .await;
    assert_eq!(anonymous_manifest_head_resp.status(), StatusCode::OK);
    assert_eq!(
        anonymous_manifest_head_resp
            .headers()
            .get("docker-content-digest")
            .expect("anonymous manifest head should include digest header"),
        manifest_digest.as_str()
    );
    assert!(body_bytes(anonymous_manifest_head_resp).await.is_empty());

    let anonymous_blob_resp = send_oci_request(
        &app,
        Method::GET,
        format!("/oci/v2/{package_name}/blobs/{layer_digest}"),
        None,
        None,
        vec![],
    )
    .await;
    assert_eq!(anonymous_blob_resp.status(), StatusCode::OK);
    assert_eq!(body_bytes(anonymous_blob_resp).await, layer_bytes);

    let anonymous_blob_head_resp = send_oci_request(
        &app,
        Method::HEAD,
        format!("/oci/v2/{package_name}/blobs/{layer_digest}"),
        None,
        None,
        vec![],
    )
    .await;
    assert_eq!(anonymous_blob_head_resp.status(), StatusCode::OK);
    assert_eq!(
        anonymous_blob_head_resp
            .headers()
            .get("docker-content-digest")
            .expect("anonymous blob head should include digest header"),
        layer_digest.as_str()
    );
    assert!(body_bytes(anonymous_blob_head_resp).await.is_empty());

    let delete_blob_while_referenced_resp = send_oci_request(
        &app,
        Method::DELETE,
        format!("/oci/v2/{package_name}/blobs/{layer_digest}"),
        Some(&oci_token),
        None,
        vec![],
    )
    .await;
    assert_eq!(
        delete_blob_while_referenced_resp.status(),
        StatusCode::CONFLICT
    );
    let delete_blob_while_referenced_body = body_json(delete_blob_while_referenced_resp).await;
    assert_eq!(
        delete_blob_while_referenced_body["errors"][0]["code"],
        "DENIED"
    );

    let delete_tag_resp = send_oci_request(
        &app,
        Method::DELETE,
        format!("/oci/v2/{package_name}/manifests/latest"),
        Some(&oci_token),
        None,
        vec![],
    )
    .await;
    assert_eq!(delete_tag_resp.status(), StatusCode::ACCEPTED);

    let tags_after_tag_delete_resp = send_oci_request(
        &app,
        Method::GET,
        format!("/oci/v2/{package_name}/tags/list"),
        None,
        None,
        vec![],
    )
    .await;
    assert_eq!(tags_after_tag_delete_resp.status(), StatusCode::OK);
    let tags_after_tag_delete_body = body_json(tags_after_tag_delete_resp).await;
    assert_eq!(tags_after_tag_delete_body["tags"], json!([]));

    let delete_manifest_resp = send_oci_request(
        &app,
        Method::DELETE,
        format!("/oci/v2/{package_name}/manifests/{manifest_digest}"),
        Some(&oci_token),
        None,
        vec![],
    )
    .await;
    assert_eq!(delete_manifest_resp.status(), StatusCode::ACCEPTED);

    let release_status_after_delete: String = sqlx::query_scalar(
        "SELECT r.status::text \
         FROM releases r \
         JOIN packages p ON p.id = r.package_id \
         WHERE p.ecosystem = 'oci' AND p.name = $1 AND r.version = $2",
    )
    .bind(package_name)
    .bind(&manifest_digest)
    .fetch_one(&pool)
    .await
    .expect("oci release status after delete should be queryable");
    assert_eq!(release_status_after_delete, "deleted");

    let delete_blob_after_manifest_delete_resp = send_oci_request(
        &app,
        Method::DELETE,
        format!("/oci/v2/{package_name}/blobs/{layer_digest}"),
        Some(&oci_token),
        None,
        vec![],
    )
    .await;
    assert_eq!(
        delete_blob_after_manifest_delete_resp.status(),
        StatusCode::ACCEPTED
    );

    let blob_after_delete_resp = send_oci_request(
        &app,
        Method::GET,
        format!("/oci/v2/{package_name}/blobs/{layer_digest}"),
        None,
        None,
        vec![],
    )
    .await;
    assert_eq!(blob_after_delete_resp.status(), StatusCode::NOT_FOUND);

    let blob_head_after_delete_resp = send_oci_request(
        &app,
        Method::HEAD,
        format!("/oci/v2/{package_name}/blobs/{layer_digest}"),
        None,
        None,
        vec![],
    )
    .await;
    assert_eq!(blob_head_after_delete_resp.status(), StatusCode::NOT_FOUND);
    assert!(body_bytes(blob_head_after_delete_resp).await.is_empty());

    let manifest_after_delete_resp = send_oci_request(
        &app,
        Method::GET,
        format!("/oci/v2/{package_name}/manifests/{manifest_digest}"),
        None,
        None,
        vec![],
    )
    .await;
    assert_eq!(manifest_after_delete_resp.status(), StatusCode::NOT_FOUND);

    let manifest_head_after_delete_resp = send_oci_request(
        &app,
        Method::HEAD,
        format!("/oci/v2/{package_name}/manifests/{manifest_digest}"),
        None,
        None,
        vec![],
    )
    .await;
    assert_eq!(
        manifest_head_after_delete_resp.status(),
        StatusCode::NOT_FOUND
    );
    assert!(body_bytes(manifest_head_after_delete_resp).await.is_empty());
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_native_oci_referrers_support_subject_headers_filters_and_pagination(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "Public OCI Referrer Packages",
        "public-oci-referrer-packages",
        None,
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let package_name = "acme/public-referrer-widget";
    let (status, package_body) = create_package_with_options(
        &app,
        &alice_jwt,
        "oci",
        package_name,
        "public-oci-referrer-packages",
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );

    let (status, token_body) =
        create_personal_access_token(&app, &alice_jwt, "alice-oci-referrers", &["packages:write"])
            .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected token response: {token_body}"
    );
    let oci_token = token_body["token"]
        .as_str()
        .expect("oci token should be returned")
        .to_owned();

    let subject_config_bytes =
        br#"{"architecture":"amd64","os":"linux","rootfs":{"type":"layers","diff_ids":[]}}"#;
    let subject_layer_bytes = b"subject-image-layer";
    let (subject_config_digest, subject_config_resp) =
        upload_oci_blob_monolithic(&app, &oci_token, package_name, subject_config_bytes).await;
    assert_eq!(subject_config_resp.status(), StatusCode::CREATED);
    let (subject_layer_digest, subject_layer_resp) =
        upload_oci_blob_monolithic(&app, &oci_token, package_name, subject_layer_bytes).await;
    assert_eq!(subject_layer_resp.status(), StatusCode::CREATED);

    let subject_manifest_bytes = build_oci_image_manifest(
        &subject_config_digest,
        subject_config_bytes.len(),
        &subject_layer_digest,
        subject_layer_bytes.len(),
    );
    let subject_manifest_digest = oci_digest(&subject_manifest_bytes);
    let subject_put_resp = send_oci_request(
        &app,
        Method::PUT,
        format!("/oci/v2/{package_name}/manifests/latest"),
        Some(&oci_token),
        Some("application/vnd.oci.image.manifest.v1+json"),
        subject_manifest_bytes.clone(),
    )
    .await;
    assert_eq!(subject_put_resp.status(), StatusCode::CREATED);

    let sbom_blob_bytes = br#"{"spdxVersion":"SPDX-2.3"}"#;
    let (sbom_blob_digest, sbom_blob_resp) =
        upload_oci_blob_monolithic(&app, &oci_token, package_name, sbom_blob_bytes).await;
    assert_eq!(sbom_blob_resp.status(), StatusCode::CREATED);
    let sbom_annotations = json!({
        "org.opencontainers.artifact.created": "2026-04-20T12:00:00Z",
        "org.example.sbom.format": "json",
    });
    let sbom_manifest_bytes = build_oci_artifact_manifest(
        "application/vnd.example.sbom.v1+json",
        &subject_manifest_digest,
        subject_manifest_bytes.len(),
        &sbom_blob_digest,
        sbom_blob_bytes.len(),
        Some(sbom_annotations.clone()),
    );
    let sbom_manifest_digest = oci_digest(&sbom_manifest_bytes);
    let sbom_put_resp = send_oci_request(
        &app,
        Method::PUT,
        format!("/oci/v2/{package_name}/manifests/sbom"),
        Some(&oci_token),
        Some("application/vnd.oci.artifact.manifest.v1+json"),
        sbom_manifest_bytes,
    )
    .await;
    assert_eq!(sbom_put_resp.status(), StatusCode::CREATED);
    assert_eq!(
        sbom_put_resp
            .headers()
            .get("OCI-Subject")
            .expect("subject-bearing manifest pushes should acknowledge the subject"),
        subject_manifest_digest.as_str()
    );

    let signature_config_media_type = "application/vnd.dev.cosign.simplesigning.v1+json";
    let signature_config_bytes =
        br#"{"critical":{"identity":{"docker-reference":"acme/public-referrer-widget"}}}"#;
    let signature_layer_bytes = b"signature-manifest-layer";
    let (signature_config_digest, signature_config_resp) =
        upload_oci_blob_monolithic(&app, &oci_token, package_name, signature_config_bytes).await;
    assert_eq!(signature_config_resp.status(), StatusCode::CREATED);
    let (signature_layer_digest, signature_layer_resp) =
        upload_oci_blob_monolithic(&app, &oci_token, package_name, signature_layer_bytes).await;
    assert_eq!(signature_layer_resp.status(), StatusCode::CREATED);
    let signature_annotations = json!({
        "org.opencontainers.artifact.created": "2026-04-20T12:05:00Z",
        "org.example.signature.kind": "simplesigning",
    });
    let signature_manifest_bytes = build_oci_image_manifest_with_options(
        &signature_config_digest,
        signature_config_bytes.len(),
        signature_config_media_type,
        &signature_layer_digest,
        signature_layer_bytes.len(),
        Some(&subject_manifest_digest),
        Some(subject_manifest_bytes.len()),
        Some(signature_annotations.clone()),
    );
    let signature_manifest_digest = oci_digest(&signature_manifest_bytes);
    let signature_put_resp = send_oci_request(
        &app,
        Method::PUT,
        format!("/oci/v2/{package_name}/manifests/signature"),
        Some(&oci_token),
        Some("application/vnd.oci.image.manifest.v1+json"),
        signature_manifest_bytes,
    )
    .await;
    assert_eq!(signature_put_resp.status(), StatusCode::CREATED);
    assert_eq!(
        signature_put_resp
            .headers()
            .get("OCI-Subject")
            .expect("subject-bearing manifest pushes should acknowledge the subject"),
        subject_manifest_digest.as_str()
    );

    let second_signature_config_bytes =
        br#"{"critical":{"identity":{"docker-reference":"acme/public-referrer-widget:v2"}}}"#;
    let second_signature_layer_bytes = b"signature-manifest-layer-v2";
    let (second_signature_config_digest, second_signature_config_resp) =
        upload_oci_blob_monolithic(
            &app,
            &oci_token,
            package_name,
            second_signature_config_bytes,
        )
        .await;
    assert_eq!(second_signature_config_resp.status(), StatusCode::CREATED);
    let (second_signature_layer_digest, second_signature_layer_resp) =
        upload_oci_blob_monolithic(&app, &oci_token, package_name, second_signature_layer_bytes)
            .await;
    assert_eq!(second_signature_layer_resp.status(), StatusCode::CREATED);
    let second_signature_annotations = json!({
        "org.opencontainers.artifact.created": "2026-04-20T12:10:00Z",
        "org.example.signature.kind": "simplesigning-v2",
    });
    let second_signature_manifest_bytes = build_oci_image_manifest_with_options(
        &second_signature_config_digest,
        second_signature_config_bytes.len(),
        signature_config_media_type,
        &second_signature_layer_digest,
        second_signature_layer_bytes.len(),
        Some(&subject_manifest_digest),
        Some(subject_manifest_bytes.len()),
        Some(second_signature_annotations.clone()),
    );
    let second_signature_manifest_digest = oci_digest(&second_signature_manifest_bytes);
    let second_signature_put_resp = send_oci_request(
        &app,
        Method::PUT,
        format!("/oci/v2/{package_name}/manifests/signature-v2"),
        Some(&oci_token),
        Some("application/vnd.oci.image.manifest.v1+json"),
        second_signature_manifest_bytes,
    )
    .await;
    assert_eq!(second_signature_put_resp.status(), StatusCode::CREATED);
    assert_eq!(
        second_signature_put_resp
            .headers()
            .get("OCI-Subject")
            .expect("subject-bearing manifest pushes should acknowledge the subject"),
        subject_manifest_digest.as_str()
    );

    let referrers_resp =
        get_oci_referrers(&app, None, package_name, &subject_manifest_digest, None).await;
    assert_eq!(referrers_resp.status(), StatusCode::OK);
    assert_eq!(
        referrers_resp
            .headers()
            .get(header::CONTENT_TYPE)
            .expect("referrers response should expose the OCI image index media type"),
        "application/vnd.oci.image.index.v1+json"
    );
    let referrers_body = body_json(referrers_resp).await;
    assert_eq!(referrers_body["schemaVersion"], 2);
    assert_eq!(
        referrers_body["mediaType"],
        "application/vnd.oci.image.index.v1+json"
    );
    let referrers = referrers_body["manifests"]
        .as_array()
        .expect("referrers response should contain a manifests array");
    assert_eq!(referrers.len(), 3, "response: {referrers_body}");

    let sbom_descriptor = referrers
        .iter()
        .find(|descriptor| descriptor["digest"] == sbom_manifest_digest)
        .expect("sbom referrer should be present");
    assert_eq!(
        sbom_descriptor["mediaType"],
        "application/vnd.oci.artifact.manifest.v1+json"
    );
    assert_eq!(
        sbom_descriptor["artifactType"],
        "application/vnd.example.sbom.v1+json"
    );
    assert_eq!(
        sbom_descriptor["annotations"]["org.example.sbom.format"],
        "json"
    );

    let signature_descriptor = referrers
        .iter()
        .find(|descriptor| descriptor["digest"] == signature_manifest_digest)
        .expect("signature referrer should be present");
    assert_eq!(
        signature_descriptor["mediaType"],
        "application/vnd.oci.image.manifest.v1+json"
    );
    assert_eq!(
        signature_descriptor["artifactType"],
        signature_config_media_type
    );
    assert_eq!(
        signature_descriptor["annotations"]["org.example.signature.kind"],
        "simplesigning"
    );

    let second_signature_descriptor = referrers
        .iter()
        .find(|descriptor| descriptor["digest"] == second_signature_manifest_digest)
        .expect("second signature referrer should be present");
    assert_eq!(
        second_signature_descriptor["mediaType"],
        "application/vnd.oci.image.manifest.v1+json"
    );
    assert_eq!(
        second_signature_descriptor["artifactType"],
        signature_config_media_type
    );
    assert_eq!(
        second_signature_descriptor["annotations"]["org.example.signature.kind"],
        "simplesigning-v2"
    );

    let first_page_resp = get_oci_referrers(
        &app,
        None,
        package_name,
        &subject_manifest_digest,
        Some("n=2"),
    )
    .await;
    assert_eq!(first_page_resp.status(), StatusCode::OK);
    let next_link = first_page_resp
        .headers()
        .get(header::LINK)
        .expect("paginated referrers responses should include a next link")
        .to_str()
        .expect("link header should be utf-8")
        .to_owned();
    assert!(next_link.contains("rel=\"next\""));
    let first_page_body = body_json(first_page_resp).await;
    let first_page_manifests = first_page_body["manifests"]
        .as_array()
        .expect("first page should contain a manifests array");
    assert_eq!(first_page_manifests.len(), 2);
    let first_page_last_digest = first_page_manifests[1]["digest"]
        .as_str()
        .expect("first page last digest should be present")
        .to_owned();
    let encoded_first_page_last_digest: String =
        url::form_urlencoded::byte_serialize(first_page_last_digest.as_bytes()).collect();
    assert!(next_link.contains(&format!("last={encoded_first_page_last_digest}")));
    let next_uri = extract_oci_next_link_uri(&next_link);
    let second_page_resp = send_oci_request(&app, Method::GET, next_uri, None, None, vec![]).await;
    assert_eq!(second_page_resp.status(), StatusCode::OK);
    assert!(
        second_page_resp.headers().get(header::LINK).is_none(),
        "the terminal referrers page should not expose another next link"
    );
    let second_page_body = body_json(second_page_resp).await;
    let second_page_manifests = second_page_body["manifests"]
        .as_array()
        .expect("second page should contain a manifests array");
    assert_eq!(second_page_manifests.len(), 1);
    let second_page_digest = second_page_manifests[0]["digest"]
        .as_str()
        .expect("second page digest should be present")
        .to_owned();
    let paged_digests = first_page_manifests
        .iter()
        .map(|manifest| {
            manifest["digest"]
                .as_str()
                .expect("first page digest should be present")
                .to_owned()
        })
        .chain(std::iter::once(second_page_digest))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        paged_digests,
        std::collections::BTreeSet::from([
            sbom_manifest_digest.clone(),
            signature_manifest_digest.clone(),
            second_signature_manifest_digest.clone(),
        ])
    );

    let encoded_filter: String =
        url::form_urlencoded::byte_serialize(signature_config_media_type.as_bytes()).collect();
    let filtered_first_page_resp = get_oci_referrers(
        &app,
        None,
        package_name,
        &subject_manifest_digest,
        Some(&format!("artifactType={encoded_filter}&n=1")),
    )
    .await;
    assert_eq!(filtered_first_page_resp.status(), StatusCode::OK);
    assert_eq!(
        filtered_first_page_resp
            .headers()
            .get("OCI-Filters-Applied")
            .expect("artifactType filters should be acknowledged"),
        "artifactType"
    );
    let filtered_first_page_link = filtered_first_page_resp
        .headers()
        .get(header::LINK)
        .expect("filtered pagination should expose a next link")
        .to_str()
        .expect("filtered link header should be utf-8")
        .to_owned();
    assert!(filtered_first_page_link.contains(&format!("artifactType={encoded_filter}")));
    let filtered_first_page_body = body_json(filtered_first_page_resp).await;
    let filtered_first_page_manifests = filtered_first_page_body["manifests"]
        .as_array()
        .expect("filtered response should contain a manifests array");
    assert_eq!(filtered_first_page_manifests.len(), 1);
    let filtered_first_page_digest = filtered_first_page_manifests[0]["digest"]
        .as_str()
        .expect("filtered first page digest should be present")
        .to_owned();
    let encoded_filtered_last: String =
        url::form_urlencoded::byte_serialize(filtered_first_page_digest.as_bytes()).collect();
    assert!(filtered_first_page_link.contains(&format!("last={encoded_filtered_last}")));

    let filtered_second_page_resp = send_oci_request(
        &app,
        Method::GET,
        extract_oci_next_link_uri(&filtered_first_page_link),
        None,
        None,
        vec![],
    )
    .await;
    assert_eq!(filtered_second_page_resp.status(), StatusCode::OK);
    assert_eq!(
        filtered_second_page_resp
            .headers()
            .get("OCI-Filters-Applied")
            .expect("filtered continuation pages should acknowledge filters"),
        "artifactType"
    );
    assert!(
        filtered_second_page_resp
            .headers()
            .get(header::LINK)
            .is_none(),
        "the terminal filtered referrers page should not expose another next link"
    );
    let filtered_second_page_body = body_json(filtered_second_page_resp).await;
    let filtered_second_page_manifests = filtered_second_page_body["manifests"]
        .as_array()
        .expect("filtered continuation response should contain a manifests array");
    assert_eq!(filtered_second_page_manifests.len(), 1);
    let filtered_paged_digests = filtered_first_page_manifests
        .iter()
        .map(|manifest| {
            manifest["digest"]
                .as_str()
                .expect("filtered digest should be present")
                .to_owned()
        })
        .chain(filtered_second_page_manifests.iter().map(|manifest| {
            manifest["digest"]
                .as_str()
                .expect("filtered continuation digest should be present")
                .to_owned()
        }))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        filtered_paged_digests,
        std::collections::BTreeSet::from([
            signature_manifest_digest.clone(),
            second_signature_manifest_digest.clone(),
        ])
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_native_oci_referrers_require_auth_for_private_repositories_and_return_empty_indexes(
    pool: PgPool,
) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "Private OCI Referrer Packages",
        "private-oci-referrer-packages",
        None,
        Some("private"),
        Some("private"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let package_name = "acme/private-referrer-widget";
    let (status, package_body) = create_package_with_options(
        &app,
        &alice_jwt,
        "oci",
        package_name,
        "private-oci-referrer-packages",
        Some("private"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );

    let (status, token_body) = create_personal_access_token(
        &app,
        &alice_jwt,
        "alice-private-oci-referrers",
        &["packages:write"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected token response: {token_body}"
    );
    let oci_token = token_body["token"]
        .as_str()
        .expect("oci token should be returned")
        .to_owned();

    let config_bytes =
        br#"{"architecture":"amd64","os":"linux","rootfs":{"type":"layers","diff_ids":[]}}"#;
    let layer_bytes = b"private-referrer-layer";
    let (config_digest, config_resp) =
        upload_oci_blob_monolithic(&app, &oci_token, package_name, config_bytes).await;
    assert_eq!(config_resp.status(), StatusCode::CREATED);
    let (layer_digest, layer_resp) =
        upload_oci_blob_monolithic(&app, &oci_token, package_name, layer_bytes).await;
    assert_eq!(layer_resp.status(), StatusCode::CREATED);

    let manifest_bytes = build_oci_image_manifest(
        &config_digest,
        config_bytes.len(),
        &layer_digest,
        layer_bytes.len(),
    );
    let manifest_digest = oci_digest(&manifest_bytes);
    let manifest_put_resp = send_oci_request(
        &app,
        Method::PUT,
        format!("/oci/v2/{package_name}/manifests/latest"),
        Some(&oci_token),
        Some("application/vnd.oci.image.manifest.v1+json"),
        manifest_bytes,
    )
    .await;
    assert_eq!(manifest_put_resp.status(), StatusCode::CREATED);

    let encoded_filter: String =
        url::form_urlencoded::byte_serialize("application/vnd.example.empty.v1+json".as_bytes())
            .collect();
    let anonymous_referrers_resp = get_oci_referrers(
        &app,
        None,
        package_name,
        &manifest_digest,
        Some(&format!("n=1&artifactType={encoded_filter}")),
    )
    .await;
    assert_eq!(anonymous_referrers_resp.status(), StatusCode::UNAUTHORIZED);
    assert!(anonymous_referrers_resp
        .headers()
        .get(header::WWW_AUTHENTICATE)
        .expect("private referrers should challenge unauthenticated clients")
        .to_str()
        .expect("challenge header should be utf-8")
        .contains(&format!("repository:{package_name}:pull")));

    let unauthorized_referrers_resp = get_oci_referrers(
        &app,
        Some(&bob_jwt),
        package_name,
        &manifest_digest,
        Some("n=1"),
    )
    .await;
    assert_eq!(unauthorized_referrers_resp.status(), StatusCode::NOT_FOUND);
    let unauthorized_referrers_body = body_json(unauthorized_referrers_resp).await;
    assert_eq!(
        unauthorized_referrers_body["errors"][0]["code"],
        "NAME_UNKNOWN"
    );

    let authenticated_empty_resp = get_oci_referrers(
        &app,
        Some(&oci_token),
        package_name,
        &manifest_digest,
        Some(&format!("n=1&artifactType={encoded_filter}")),
    )
    .await;
    assert_eq!(authenticated_empty_resp.status(), StatusCode::OK);
    assert_eq!(
        authenticated_empty_resp
            .headers()
            .get("OCI-Filters-Applied")
            .expect("empty filtered referrers responses should acknowledge filters"),
        "artifactType"
    );
    assert!(
        authenticated_empty_resp
            .headers()
            .get(header::LINK)
            .is_none(),
        "empty filtered referrers responses should not expose a next link"
    );
    let authenticated_empty_body = body_json(authenticated_empty_resp).await;
    assert_eq!(authenticated_empty_body["schemaVersion"], 2);
    assert_eq!(authenticated_empty_body["manifests"], json!([]));

    let invalid_digest_resp = get_oci_referrers(
        &app,
        Some(&oci_token),
        package_name,
        "not-a-valid-digest",
        None,
    )
    .await;
    assert_eq!(invalid_digest_resp.status(), StatusCode::BAD_REQUEST);
    let invalid_digest_body = body_json(invalid_digest_resp).await;
    assert_eq!(invalid_digest_body["errors"][0]["code"], "DIGEST_INVALID");
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_native_oci_blob_cleanup_jobs_are_enqueued_for_uploads_and_manifest_delete(
    pool: PgPool,
) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "OCI Cleanup Scheduling",
        "oci-cleanup-scheduling",
        None,
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let package_name = "acme/cleanup-scheduling-widget";
    let (status, package_body) = create_package_with_options(
        &app,
        &alice_jwt,
        "oci",
        package_name,
        "oci-cleanup-scheduling",
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );

    let (status, token_body) = create_personal_access_token(
        &app,
        &alice_jwt,
        "alice-oci-cleanup-scheduling",
        &["packages:write"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected token response: {token_body}"
    );
    let oci_token = token_body["token"]
        .as_str()
        .expect("oci token should be returned")
        .to_owned();

    let config_bytes =
        br#"{"architecture":"amd64","os":"linux","rootfs":{"type":"layers","diff_ids":[]}}"#;
    let layer_bytes = b"oci-cleanup-scheduling-layer";
    let (config_digest, config_resp) =
        upload_oci_blob_monolithic(&app, &oci_token, package_name, config_bytes).await;
    assert_eq!(config_resp.status(), StatusCode::CREATED);
    let (layer_digest, layer_resp) =
        upload_oci_blob_monolithic(&app, &oci_token, package_name, layer_bytes).await;
    assert_eq!(layer_resp.status(), StatusCode::CREATED);

    let cleanup_jobs_after_upload = fetch_oci_cleanup_jobs(&pool).await;
    assert_eq!(
        cleanup_jobs_after_upload.len(),
        2,
        "jobs: {cleanup_jobs_after_upload:?}"
    );
    assert!(cleanup_jobs_after_upload
        .iter()
        .all(|(_, status, _)| status == "pending"));
    assert!(cleanup_jobs_after_upload.iter().all(|(payload, _, _)| {
        payload["grace_period_hours"] == json!(168) && payload["batch_size"] == json!(100)
    }));
    assert!(cleanup_jobs_after_upload
        .iter()
        .all(|(_, _, scheduled_at)| *scheduled_at > Utc::now()));

    let manifest_bytes = build_oci_image_manifest(
        &config_digest,
        config_bytes.len(),
        &layer_digest,
        layer_bytes.len(),
    );
    let manifest_digest = oci_digest(&manifest_bytes);
    let manifest_resp = send_oci_request(
        &app,
        Method::PUT,
        format!("/oci/v2/{package_name}/manifests/latest"),
        Some(&oci_token),
        Some("application/vnd.oci.image.manifest.v1+json"),
        manifest_bytes,
    )
    .await;
    assert_eq!(manifest_resp.status(), StatusCode::CREATED);

    let delete_tag_resp = send_oci_request(
        &app,
        Method::DELETE,
        format!("/oci/v2/{package_name}/manifests/latest"),
        Some(&oci_token),
        None,
        vec![],
    )
    .await;
    assert_eq!(delete_tag_resp.status(), StatusCode::ACCEPTED);

    let delete_manifest_resp = send_oci_request(
        &app,
        Method::DELETE,
        format!("/oci/v2/{package_name}/manifests/{manifest_digest}"),
        Some(&oci_token),
        None,
        vec![],
    )
    .await;
    assert_eq!(delete_manifest_resp.status(), StatusCode::ACCEPTED);

    let cleanup_jobs_after_delete = fetch_oci_cleanup_jobs(&pool).await;
    assert_eq!(
        cleanup_jobs_after_delete.len(),
        3,
        "jobs: {cleanup_jobs_after_delete:?}"
    );
    assert!(cleanup_jobs_after_delete
        .iter()
        .any(|(_, _, scheduled_at)| *scheduled_at <= Utc::now()));
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_native_oci_blob_cleanup_handler_preserves_referenced_blobs(pool: PgPool) {
    let (state, app) = app_with_state(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "OCI Cleanup Preserve",
        "oci-cleanup-preserve",
        None,
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let package_name = "acme/cleanup-preserve-widget";
    let (status, package_body) = create_package_with_options(
        &app,
        &alice_jwt,
        "oci",
        package_name,
        "oci-cleanup-preserve",
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );

    let (status, token_body) = create_personal_access_token(
        &app,
        &alice_jwt,
        "alice-oci-cleanup-preserve",
        &["packages:write"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected token response: {token_body}"
    );
    let oci_token = token_body["token"]
        .as_str()
        .expect("oci token should be returned")
        .to_owned();

    let config_bytes =
        br#"{"architecture":"amd64","os":"linux","rootfs":{"type":"layers","diff_ids":[]}}"#;
    let layer_bytes = b"oci-cleanup-preserve-layer";
    let (config_digest, config_resp) =
        upload_oci_blob_monolithic(&app, &oci_token, package_name, config_bytes).await;
    assert_eq!(config_resp.status(), StatusCode::CREATED);
    let (layer_digest, layer_resp) =
        upload_oci_blob_monolithic(&app, &oci_token, package_name, layer_bytes).await;
    assert_eq!(layer_resp.status(), StatusCode::CREATED);

    let manifest_resp = send_oci_request(
        &app,
        Method::PUT,
        format!("/oci/v2/{package_name}/manifests/latest"),
        Some(&oci_token),
        Some("application/vnd.oci.image.manifest.v1+json"),
        build_oci_image_manifest(
            &config_digest,
            config_bytes.len(),
            &layer_digest,
            layer_bytes.len(),
        ),
    )
    .await;
    assert_eq!(manifest_resp.status(), StatusCode::CREATED);

    let cleanup_handler = CleanupOciBlobsHandler {
        db: pool.clone(),
        artifact_store: state.artifact_store.clone(),
    };
    cleanup_handler
        .handle(json!({ "grace_period_hours": 0, "batch_size": 100 }))
        .await
        .expect("cleanup handler should succeed");

    assert!(state
        .artifact_store
        .get_object(&oci_blob_storage_key(&layer_digest))
        .await
        .expect("blob lookup should succeed")
        .is_some());
    assert_eq!(count_oci_blob_inventory(&pool, &layer_digest).await, 1);

    let blob_resp = send_oci_request(
        &app,
        Method::GET,
        format!("/oci/v2/{package_name}/blobs/{layer_digest}"),
        None,
        None,
        vec![],
    )
    .await;
    assert_eq!(blob_resp.status(), StatusCode::OK);
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_native_oci_blob_cleanup_handler_removes_orphaned_blobs_after_manifest_delete(
    pool: PgPool,
) {
    let (state, app) = app_with_state(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "OCI Cleanup Remove",
        "oci-cleanup-remove",
        None,
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let package_name = "acme/cleanup-remove-widget";
    let (status, package_body) = create_package_with_options(
        &app,
        &alice_jwt,
        "oci",
        package_name,
        "oci-cleanup-remove",
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );

    let (status, token_body) = create_personal_access_token(
        &app,
        &alice_jwt,
        "alice-oci-cleanup-remove",
        &["packages:write"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected token response: {token_body}"
    );
    let oci_token = token_body["token"]
        .as_str()
        .expect("oci token should be returned")
        .to_owned();

    let config_bytes =
        br#"{"architecture":"amd64","os":"linux","rootfs":{"type":"layers","diff_ids":[]}}"#;
    let layer_bytes = b"oci-cleanup-remove-layer";
    let (config_digest, config_resp) =
        upload_oci_blob_monolithic(&app, &oci_token, package_name, config_bytes).await;
    assert_eq!(config_resp.status(), StatusCode::CREATED);
    let (layer_digest, layer_resp) =
        upload_oci_blob_monolithic(&app, &oci_token, package_name, layer_bytes).await;
    assert_eq!(layer_resp.status(), StatusCode::CREATED);

    let manifest_bytes = build_oci_image_manifest(
        &config_digest,
        config_bytes.len(),
        &layer_digest,
        layer_bytes.len(),
    );
    let manifest_digest = oci_digest(&manifest_bytes);
    let manifest_resp = send_oci_request(
        &app,
        Method::PUT,
        format!("/oci/v2/{package_name}/manifests/latest"),
        Some(&oci_token),
        Some("application/vnd.oci.image.manifest.v1+json"),
        manifest_bytes,
    )
    .await;
    assert_eq!(manifest_resp.status(), StatusCode::CREATED);

    let delete_tag_resp = send_oci_request(
        &app,
        Method::DELETE,
        format!("/oci/v2/{package_name}/manifests/latest"),
        Some(&oci_token),
        None,
        vec![],
    )
    .await;
    assert_eq!(delete_tag_resp.status(), StatusCode::ACCEPTED);

    let delete_manifest_resp = send_oci_request(
        &app,
        Method::DELETE,
        format!("/oci/v2/{package_name}/manifests/{manifest_digest}"),
        Some(&oci_token),
        None,
        vec![],
    )
    .await;
    assert_eq!(delete_manifest_resp.status(), StatusCode::ACCEPTED);

    let cleanup_handler = CleanupOciBlobsHandler {
        db: pool.clone(),
        artifact_store: state.artifact_store.clone(),
    };
    cleanup_handler
        .handle(json!({ "grace_period_hours": 0, "batch_size": 100 }))
        .await
        .expect("cleanup handler should succeed");

    assert!(state
        .artifact_store
        .get_object(&oci_blob_storage_key(&layer_digest))
        .await
        .expect("blob lookup should succeed")
        .is_none());
    assert_eq!(count_oci_blob_inventory(&pool, &layer_digest).await, 0);

    let blob_resp = send_oci_request(
        &app,
        Method::GET,
        format!("/oci/v2/{package_name}/blobs/{layer_digest}"),
        None,
        None,
        vec![],
    )
    .await;
    assert_eq!(blob_resp.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_native_oci_blob_cleanup_handler_respects_grace_period_and_is_idempotent(
    pool: PgPool,
) {
    let (state, app) = app_with_state(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "OCI Cleanup Grace",
        "oci-cleanup-grace",
        None,
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let package_name = "acme/cleanup-grace-widget";
    let (status, package_body) = create_package_with_options(
        &app,
        &alice_jwt,
        "oci",
        package_name,
        "oci-cleanup-grace",
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );

    let (status, token_body) = create_personal_access_token(
        &app,
        &alice_jwt,
        "alice-oci-cleanup-grace",
        &["packages:write"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected token response: {token_body}"
    );
    let oci_token = token_body["token"]
        .as_str()
        .expect("oci token should be returned")
        .to_owned();

    let layer_bytes = b"oci-cleanup-grace-layer";
    let (layer_digest, layer_resp) =
        upload_oci_blob_monolithic(&app, &oci_token, package_name, layer_bytes).await;
    assert_eq!(layer_resp.status(), StatusCode::CREATED);

    let cleanup_handler = CleanupOciBlobsHandler {
        db: pool.clone(),
        artifact_store: state.artifact_store.clone(),
    };
    cleanup_handler
        .handle(json!({ "grace_period_hours": 24, "batch_size": 100 }))
        .await
        .expect("grace-period cleanup should succeed");

    assert!(state
        .artifact_store
        .get_object(&oci_blob_storage_key(&layer_digest))
        .await
        .expect("blob lookup should succeed")
        .is_some());
    assert_eq!(count_oci_blob_inventory(&pool, &layer_digest).await, 1);

    cleanup_handler
        .handle(json!({ "grace_period_hours": 0, "batch_size": 100 }))
        .await
        .expect("cleanup handler should delete orphaned blob");
    cleanup_handler
        .handle(json!({ "grace_period_hours": 0, "batch_size": 100 }))
        .await
        .expect("cleanup handler should be idempotent");

    assert!(state
        .artifact_store
        .get_object(&oci_blob_storage_key(&layer_digest))
        .await
        .expect("blob lookup should succeed")
        .is_none());
    assert_eq!(count_oci_blob_inventory(&pool, &layer_digest).await, 0);
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_native_oci_catalog_pagination_respects_visibility_and_ordering(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    for (repository_name, repository_slug, visibility, package_name) in [
        (
            "Alpha OCI Packages",
            "alpha-oci-packages",
            "public",
            "acme/alpha-widget",
        ),
        (
            "Bravo OCI Packages",
            "bravo-oci-packages",
            "private",
            "acme/bravo-widget",
        ),
        (
            "Zulu OCI Packages",
            "zulu-oci-packages",
            "public",
            "acme/zulu-widget",
        ),
    ] {
        let (status, repository_body) = create_repository_with_options(
            &app,
            &alice_jwt,
            repository_name,
            repository_slug,
            None,
            Some(visibility),
            Some(visibility),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "unexpected repository response: {repository_body}"
        );

        let (status, package_body) = create_package_with_options(
            &app,
            &alice_jwt,
            "oci",
            package_name,
            repository_slug,
            Some(visibility),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "unexpected package response: {package_body}"
        );
    }

    let (status, token_body) =
        create_personal_access_token(&app, &alice_jwt, "alice-oci-catalog", &["packages:write"])
            .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected token response: {token_body}"
    );
    let oci_token = token_body["token"]
        .as_str()
        .expect("oci token should be returned")
        .to_owned();

    for (tag, package_name) in [
        ("1.0.0", "acme/alpha-widget"),
        ("1.0.0", "acme/bravo-widget"),
        ("1.0.0", "acme/zulu-widget"),
    ] {
        let config_bytes = format!(
            "{{\"architecture\":\"amd64\",\"os\":\"linux\",\"tag\":\"{tag}\",\"package\":\"{package_name}\"}}"
        );
        let layer_bytes = format!("layer-{package_name}-{tag}");
        publish_oci_image_manifest_tag(
            &app,
            &oci_token,
            package_name,
            tag,
            config_bytes.as_bytes(),
            layer_bytes.as_bytes(),
        )
        .await;
    }

    let anonymous_first_page_resp = get_oci_catalog(&app, None, Some("n=1")).await;
    assert_eq!(anonymous_first_page_resp.status(), StatusCode::OK);
    let anonymous_first_page_link = anonymous_first_page_resp
        .headers()
        .get(header::LINK)
        .expect("paginated anonymous catalog responses should include a next link")
        .to_str()
        .expect("catalog next link should be utf-8")
        .to_owned();
    assert!(anonymous_first_page_link.contains("rel=\"next\""));
    assert!(anonymous_first_page_link.contains("last=acme%2Falpha-widget"));
    let anonymous_first_page_body = body_json(anonymous_first_page_resp).await;
    assert_eq!(
        anonymous_first_page_body["repositories"],
        json!(["acme/alpha-widget"])
    );

    let anonymous_second_page_resp = send_oci_request(
        &app,
        Method::GET,
        extract_oci_next_link_uri(&anonymous_first_page_link),
        None,
        None,
        vec![],
    )
    .await;
    assert_eq!(anonymous_second_page_resp.status(), StatusCode::OK);
    assert!(
        anonymous_second_page_resp
            .headers()
            .get(header::LINK)
            .is_none(),
        "the final anonymous catalog page should not expose another next link"
    );
    let anonymous_second_page_body = body_json(anonymous_second_page_resp).await;
    assert_eq!(
        anonymous_second_page_body["repositories"],
        json!(["acme/zulu-widget"])
    );

    let anonymous_after_private_last_resp =
        get_oci_catalog(&app, None, Some("last=acme%2Fbravo-widget&n=10")).await;
    assert_eq!(anonymous_after_private_last_resp.status(), StatusCode::OK);
    let anonymous_after_private_last_body = body_json(anonymous_after_private_last_resp).await;
    assert_eq!(
        anonymous_after_private_last_body["repositories"],
        json!(["acme/zulu-widget"])
    );

    let authenticated_first_page_resp = get_oci_catalog(&app, Some(&alice_jwt), Some("n=2")).await;
    assert_eq!(authenticated_first_page_resp.status(), StatusCode::OK);
    let authenticated_first_page_link = authenticated_first_page_resp
        .headers()
        .get(header::LINK)
        .expect("paginated authenticated catalog responses should include a next link")
        .to_str()
        .expect("authenticated catalog next link should be utf-8")
        .to_owned();
    assert!(authenticated_first_page_link.contains("last=acme%2Fbravo-widget"));
    let authenticated_first_page_body = body_json(authenticated_first_page_resp).await;
    assert_eq!(
        authenticated_first_page_body["repositories"],
        json!(["acme/alpha-widget", "acme/bravo-widget"])
    );

    let authenticated_second_page_resp = send_oci_request(
        &app,
        Method::GET,
        extract_oci_next_link_uri(&authenticated_first_page_link),
        Some(&alice_jwt),
        None,
        vec![],
    )
    .await;
    assert_eq!(authenticated_second_page_resp.status(), StatusCode::OK);
    assert!(
        authenticated_second_page_resp
            .headers()
            .get(header::LINK)
            .is_none(),
        "the final authenticated catalog page should not expose another next link"
    );
    let authenticated_second_page_body = body_json(authenticated_second_page_resp).await;
    assert_eq!(
        authenticated_second_page_body["repositories"],
        json!(["acme/zulu-widget"])
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_native_oci_tags_list_pagination_respects_n_last_and_ordering(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "Paged OCI Tags",
        "paged-oci-tags",
        None,
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let package_name = "acme/paged-tags-widget";
    let (status, package_body) = create_package_with_options(
        &app,
        &alice_jwt,
        "oci",
        package_name,
        "paged-oci-tags",
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );

    let (status, token_body) =
        create_personal_access_token(&app, &alice_jwt, "alice-oci-tags", &["packages:write"]).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected token response: {token_body}"
    );
    let oci_token = token_body["token"]
        .as_str()
        .expect("oci token should be returned")
        .to_owned();

    for tag in ["0.9.0", "1.0.0", "1.0.1"] {
        let config_bytes =
            format!("{{\"architecture\":\"amd64\",\"os\":\"linux\",\"tag\":\"{tag}\"}}");
        let layer_bytes = format!("layer-{tag}");
        publish_oci_image_manifest_tag(
            &app,
            &oci_token,
            package_name,
            tag,
            config_bytes.as_bytes(),
            layer_bytes.as_bytes(),
        )
        .await;
    }

    let clamped_first_page_resp = get_oci_tags_list(&app, None, package_name, Some("n=0")).await;
    assert_eq!(clamped_first_page_resp.status(), StatusCode::OK);
    let clamped_first_page_link = clamped_first_page_resp
        .headers()
        .get(header::LINK)
        .expect("clamped paginated tag responses should include a next link")
        .to_str()
        .expect("tag next link should be utf-8")
        .to_owned();
    assert!(clamped_first_page_link.contains("n=1"));
    assert!(clamped_first_page_link.contains("last=0.9.0"));
    let clamped_first_page_body = body_json(clamped_first_page_resp).await;
    assert_eq!(clamped_first_page_body["name"], package_name);
    assert_eq!(clamped_first_page_body["tags"], json!(["0.9.0"]));

    let paged_first_page_resp = get_oci_tags_list(&app, None, package_name, Some("n=2")).await;
    assert_eq!(paged_first_page_resp.status(), StatusCode::OK);
    let paged_first_page_link = paged_first_page_resp
        .headers()
        .get(header::LINK)
        .expect("multi-page tag responses should include a next link")
        .to_str()
        .expect("paged tag next link should be utf-8")
        .to_owned();
    assert!(paged_first_page_link.contains("last=1.0.0"));
    let paged_first_page_body = body_json(paged_first_page_resp).await;
    assert_eq!(paged_first_page_body["tags"], json!(["0.9.0", "1.0.0"]));

    let paged_second_page_resp = send_oci_request(
        &app,
        Method::GET,
        extract_oci_next_link_uri(&paged_first_page_link),
        None,
        None,
        vec![],
    )
    .await;
    assert_eq!(paged_second_page_resp.status(), StatusCode::OK);
    assert!(
        paged_second_page_resp.headers().get(header::LINK).is_none(),
        "the final tag page should not expose another next link"
    );
    let paged_second_page_body = body_json(paged_second_page_resp).await;
    assert_eq!(paged_second_page_body["tags"], json!(["1.0.1"]));

    let manual_last_resp =
        get_oci_tags_list(&app, None, package_name, Some("last=0.9.0&n=10")).await;
    assert_eq!(manual_last_resp.status(), StatusCode::OK);
    let manual_last_body = body_json(manual_last_resp).await;
    assert_eq!(manual_last_body["tags"], json!(["1.0.0", "1.0.1"]));
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_native_rubygems_push_and_yank_keep_exact_downloads(pool: PgPool) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, repository_body) = create_repository(
        &app,
        &alice_jwt,
        "Personal RubyGems Packages",
        "personal-rubygems-packages",
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    sqlx::query(
        "UPDATE repositories \
         SET visibility = 'public', updated_at = NOW() \
         WHERE owner_user_id = (SELECT id FROM users WHERE username = $1 LIMIT 1)",
    )
    .bind("alice")
    .execute(&pool)
    .await
    .expect("alice personal repositories should be made public for RubyGems read checks");

    let (status, token_body) = create_personal_access_token(
        &app,
        &alice_jwt,
        "alice-rubygems-publish",
        &["packages:write"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected token response: {token_body}"
    );
    let rubygems_token = token_body["token"]
        .as_str()
        .expect("rubygems token should be returned")
        .to_owned();

    let gem_name = "native-rubygems-widget";
    let gem_bytes = build_rubygems_package(gem_name, "1.0.0");
    let gem_sha256 = {
        use sha2::Digest;
        hex::encode(sha2::Sha256::digest(&gem_bytes))
    };

    let (status, push_body) = push_rubygems_package(&app, &rubygems_token, &gem_bytes).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected RubyGems push response: {push_body}"
    );
    assert!(push_body.contains("Successfully registered gem"));

    let release_status: String = sqlx::query_scalar(
        "SELECT r.status::text \
         FROM releases r \
         JOIN packages p ON p.id = r.package_id \
         WHERE p.ecosystem = 'rubygems' AND p.name = $1 AND r.version = $2",
    )
    .bind(gem_name)
    .bind("1.0.0")
    .fetch_one(&pool)
    .await
    .expect("rubygems release status should be queryable");
    assert_eq!(release_status, "published");

    let (status, metadata_body) =
        get_rubygems_metadata(&app, Some(&rubygems_token), gem_name).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(metadata_body["name"], gem_name);
    assert_eq!(metadata_body["version"], "1.0.0");

    let (status, versions_body) =
        get_rubygems_versions(&app, Some(&rubygems_token), gem_name).await;
    assert_eq!(status, StatusCode::OK);
    let versions = versions_body
        .as_array()
        .expect("versions response should be an array");
    assert_eq!(versions.len(), 1, "response: {versions_body}");
    assert_eq!(versions[0]["number"], "1.0.0");

    let download_req = Request::builder()
        .method(Method::GET)
        .uri(format!("/rubygems/gems/{gem_name}-1.0.0.gem"))
        .header("x-gem-api-key", &rubygems_token)
        .body(Body::empty())
        .unwrap();
    let download_resp = app.clone().oneshot(download_req).await.unwrap();
    assert_eq!(download_resp.status(), StatusCode::OK);
    let download_headers = download_resp.headers().clone();
    let download_body = body_bytes(download_resp).await;
    assert_eq!(download_body, gem_bytes);
    assert_eq!(
        download_headers
            .get("x-checksum-sha256")
            .expect("gem download should expose sha256 header"),
        gem_sha256.as_str()
    );

    let (status, yank_body) = yank_rubygems_version(&app, &rubygems_token, gem_name, "1.0.0").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected RubyGems yank response: {yank_body}"
    );
    assert!(yank_body.contains("Successfully yanked gem"));

    let release_status_after_yank: String = sqlx::query_scalar(
        "SELECT r.status::text \
         FROM releases r \
         JOIN packages p ON p.id = r.package_id \
         WHERE p.ecosystem = 'rubygems' AND p.name = $1 AND r.version = $2",
    )
    .bind(gem_name)
    .bind("1.0.0")
    .fetch_one(&pool)
    .await
    .expect("rubygems release status after yank should be queryable");
    assert_eq!(release_status_after_yank, "yanked");

    let download_after_yank_req = Request::builder()
        .method(Method::GET)
        .uri(format!("/rubygems/gems/{gem_name}-1.0.0.gem"))
        .header("x-gem-api-key", &rubygems_token)
        .body(Body::empty())
        .unwrap();
    let download_after_yank_resp = app.clone().oneshot(download_after_yank_req).await.unwrap();
    assert_eq!(download_after_yank_resp.status(), StatusCode::OK);
    let download_after_yank_body = body_bytes(download_after_yank_resp).await;
    assert_eq!(download_after_yank_body, gem_bytes);
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_native_nuget_push_unlist_and_relist_roundtrip(pool: PgPool) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, repository_body) = create_repository(
        &app,
        &alice_jwt,
        "Personal NuGet Packages",
        "personal-nuget-packages",
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    sqlx::query(
        "UPDATE repositories \
         SET visibility = 'public', updated_at = NOW() \
         WHERE owner_user_id = (SELECT id FROM users WHERE username = $1 LIMIT 1)",
    )
    .bind("alice")
    .execute(&pool)
    .await
    .expect("alice personal repositories should be made public for NuGet read checks");

    let (status, token_body) =
        create_personal_access_token(&app, &alice_jwt, "alice-nuget-publish", &["packages:write"])
            .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected token response: {token_body}"
    );
    let nuget_token = token_body["token"]
        .as_str()
        .expect("nuget token should be returned")
        .to_owned();

    let package_id = "Native.NuGet.Widget";
    let normalized_id = package_id.to_ascii_lowercase();
    let nupkg_bytes = build_nuget_package(package_id, "1.0.0");

    let (status, push_body) =
        push_nuget_package(&app, &nuget_token, package_id, "1.0.0", &nupkg_bytes).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected NuGet push response: {push_body}"
    );
    assert_eq!(push_body, Value::Null);

    let release_status: String = sqlx::query_scalar(
        "SELECT r.status::text \
         FROM releases r \
         JOIN packages p ON p.id = r.package_id \
         WHERE p.ecosystem = 'nuget' AND p.name = $1 AND r.version = $2",
    )
    .bind(package_id)
    .bind("1.0.0")
    .fetch_one(&pool)
    .await
    .expect("nuget release status should be queryable");
    assert_eq!(release_status, "published");

    let service_index_req = Request::builder()
        .method(Method::GET)
        .uri("/nuget/v3/index.json")
        .body(Body::empty())
        .unwrap();
    let service_index_resp = app.clone().oneshot(service_index_req).await.unwrap();
    assert_eq!(service_index_resp.status(), StatusCode::OK);
    let service_index_body = body_json(service_index_resp).await;
    assert_eq!(service_index_body["version"], "3.0.0");

    let (status, version_listing_body) =
        get_nuget_version_listing(&app, Some(&nuget_token), &normalized_id).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(version_listing_body["versions"], json!(["1.0.0"]));

    let download_req = Request::builder()
        .method(Method::GET)
        .uri(format!(
            "/nuget/v3-flatcontainer/{normalized_id}/1.0.0/{normalized_id}.1.0.0.nupkg"
        ))
        .header("x-nuget-apikey", &nuget_token)
        .body(Body::empty())
        .unwrap();
    let download_resp = app.clone().oneshot(download_req).await.unwrap();
    assert_eq!(download_resp.status(), StatusCode::OK);
    let download_body = body_bytes(download_resp).await;
    assert_eq!(download_body, nupkg_bytes);

    let nuspec_req = Request::builder()
        .method(Method::GET)
        .uri(format!(
            "/nuget/v3-flatcontainer/{normalized_id}/1.0.0/{normalized_id}.nuspec"
        ))
        .header("x-nuget-apikey", &nuget_token)
        .body(Body::empty())
        .unwrap();
    let nuspec_resp = app.clone().oneshot(nuspec_req).await.unwrap();
    assert_eq!(nuspec_resp.status(), StatusCode::OK);
    let nuspec_body = body_text(nuspec_resp).await;
    assert!(nuspec_body.contains("<id>Native.NuGet.Widget</id>"));

    let (status, registration_body) =
        get_nuget_registration_index(&app, Some(&nuget_token), &normalized_id).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        registration_body["items"][0]["items"][0]["catalogEntry"]["listed"],
        true
    );

    let unlist_status =
        set_nuget_listing_state(&app, &nuget_token, package_id, "1.0.0", Method::DELETE).await;
    assert_eq!(unlist_status, StatusCode::NO_CONTENT);

    let release_status_after_unlist: String = sqlx::query_scalar(
        "SELECT r.status::text \
         FROM releases r \
         JOIN packages p ON p.id = r.package_id \
         WHERE p.ecosystem = 'nuget' AND p.name = $1 AND r.version = $2",
    )
    .bind(package_id)
    .bind("1.0.0")
    .fetch_one(&pool)
    .await
    .expect("nuget release status after unlist should be queryable");
    assert_eq!(release_status_after_unlist, "yanked");

    let (_, registration_after_unlist_body) =
        get_nuget_registration_index(&app, Some(&nuget_token), &normalized_id).await;
    assert_eq!(
        registration_after_unlist_body["items"][0]["items"][0]["catalogEntry"]["listed"],
        false
    );

    let relist_status =
        set_nuget_listing_state(&app, &nuget_token, package_id, "1.0.0", Method::POST).await;
    assert_eq!(relist_status, StatusCode::OK);

    let release_status_after_relist: String = sqlx::query_scalar(
        "SELECT r.status::text \
         FROM releases r \
         JOIN packages p ON p.id = r.package_id \
         WHERE p.ecosystem = 'nuget' AND p.name = $1 AND r.version = $2",
    )
    .bind(package_id)
    .bind("1.0.0")
    .fetch_one(&pool)
    .await
    .expect("nuget release status after relist should be queryable");
    assert_eq!(release_status_after_relist, "published");

    let (_, registration_after_relist_body) =
        get_nuget_registration_index(&app, Some(&nuget_token), &normalized_id).await;
    assert_eq!(
        registration_after_relist_body["items"][0]["items"][0]["catalogEntry"]["listed"],
        true
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_publish_release_enters_scanning_and_scan_completion_enqueues_reindex_search_job(
    pool: PgPool,
) {
    let (state, app) = app_with_state(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, repository_body) = create_repository(
        &app,
        &alice_jwt,
        "Async Search Repository",
        "async-search-repository",
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, package_body) = create_package(
        &app,
        &alice_jwt,
        "npm",
        "async-search-widget",
        "async-search-repository",
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );

    sqlx::query("DELETE FROM background_jobs")
        .execute(&pool)
        .await
        .expect("test should be able to clear pre-existing background jobs");

    let (status, create_release_body) =
        create_release_for_package(&app, &alice_jwt, "npm", "async-search-widget", "1.0.0").await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected create release response: {create_release_body}"
    );

    let (status, upload_body) = upload_release_artifact(
        &app,
        &alice_jwt,
        "npm",
        "async-search-widget",
        "1.0.0",
        "async-search-widget-1.0.0.tgz",
        "tarball",
        "application/octet-stream",
        br#"pretend npm tarball bytes"#,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected upload response: {upload_body}"
    );

    let (status, publish_body) =
        publish_release_for_package(&app, &alice_jwt, "npm", "async-search-widget", "1.0.0").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected publish response: {publish_body}"
    );
    assert_eq!(publish_body["status"], "scanning");

    let (status, _) = get_release_detail(&app, None, "npm", "async-search-widget", "1.0.0").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let release_id = get_release_id(&pool, "npm", "async-search-widget", "1.0.0").await;

    let package_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT id FROM packages WHERE ecosystem = 'npm' AND normalized_name = $1",
    )
    .bind("async-search-widget")
    .fetch_one(&pool)
    .await
    .expect("package id should be queryable");

    let scan_jobs = fetch_background_jobs(&pool, "scan_artifact").await;
    assert_eq!(scan_jobs.len(), 1, "scan jobs: {scan_jobs:?}");
    assert_eq!(scan_jobs[0].0, "scan_artifact");
    assert_eq!(scan_jobs[0].2, "pending");
    assert_eq!(scan_jobs[0].1["release_id"], release_id.to_string());

    let reindex_jobs_before = fetch_background_jobs(&pool, "reindex_search").await;
    assert!(
        reindex_jobs_before.is_empty(),
        "reindex jobs should not be enqueued before scanning finishes: {reindex_jobs_before:?}"
    );

    let scan_handler = ScanArtifactHandler {
        db: pool.clone(),
        artifact_store: Arc::new(ArtifactStoreReaderAdapter::new(
            state.artifact_store.clone(),
        )),
        scanners: vec![
            Box::new(PolicyScanner {
                max_artifact_bytes: 500 * 1024 * 1024,
            }),
            Box::new(SecretsScanner::new()),
        ],
    };

    scan_handler
        .handle(scan_jobs[0].1.clone())
        .await
        .expect("scan handler should finish successfully");

    let release_status: String =
        sqlx::query_scalar("SELECT status::text FROM releases WHERE id = $1")
            .bind(release_id)
            .fetch_one(&pool)
            .await
            .expect("release status should be queryable after scanning");
    assert_eq!(release_status, "published");

    let reindex_jobs_after = fetch_background_jobs(&pool, "reindex_search").await;
    assert_eq!(reindex_jobs_after.len(), 1, "jobs: {reindex_jobs_after:?}");
    assert_eq!(reindex_jobs_after[0].0, "reindex_search");
    assert_eq!(reindex_jobs_after[0].2, "pending");
    assert_eq!(
        reindex_jobs_after[0].1["package_id"],
        package_id.to_string()
    );

    let (status, anonymous_published_release) =
        get_release_detail(&app, None, "npm", "async-search-widget", "1.0.0").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(anonymous_published_release["status"], "published");
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_trusted_publisher_management_roundtrip_with_audit(pool: PgPool) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, repository_body) = create_repository(
        &app,
        &alice_jwt,
        "Alice PyPI Packages",
        "alice-pypi-packages",
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, package_body) = create_package_with_options(
        &app,
        &alice_jwt,
        "pypi",
        "demo-widget",
        "alice-pypi-packages",
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );
    let package_id = uuid::Uuid::parse_str(
        package_body["id"]
            .as_str()
            .expect("package id should be returned"),
    )
    .expect("package id should parse");

    let (status, owner_detail) =
        get_package_detail(&app, Some(&alice_jwt), "pypi", "demo-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(owner_detail["can_manage_trusted_publishers"], true);

    let (status, anonymous_detail) = get_package_detail(&app, None, "pypi", "demo-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(anonymous_detail["can_manage_trusted_publishers"], false);

    let (status, create_body) = create_trusted_publisher_for_package(
        &app,
        &alice_jwt,
        "pypi",
        "demo-widget",
        json!({
            "issuer": "https://token.actions.githubusercontent.com",
            "subject": "repo:acme/demo-widget:ref:refs/heads/main",
            "repository": "acme/demo-widget",
            "workflow_ref": ".github/workflows/publish.yml@refs/heads/main",
            "environment": "production",
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected trusted publisher response: {create_body}"
    );
    let publisher_id = create_body["id"]
        .as_str()
        .expect("publisher id should be returned")
        .to_owned();

    let (status, anonymous_list_body) =
        list_trusted_publishers_for_package(&app, None, "pypi", "demo-widget").await;
    assert_eq!(status, StatusCode::OK);
    let anonymous_publishers = anonymous_list_body["trusted_publishers"]
        .as_array()
        .expect("trusted publishers response should be an array");
    assert_eq!(
        anonymous_publishers.len(),
        1,
        "response: {anonymous_list_body}"
    );
    assert_eq!(
        anonymous_publishers[0]["issuer"],
        "https://token.actions.githubusercontent.com"
    );
    assert_eq!(
        anonymous_publishers[0]["subject"],
        "repo:acme/demo-widget:ref:refs/heads/main"
    );
    assert_eq!(anonymous_publishers[0]["repository"], "acme/demo-widget");
    assert_eq!(
        anonymous_publishers[0]["workflow_ref"],
        ".github/workflows/publish.yml@refs/heads/main"
    );
    assert_eq!(anonymous_publishers[0]["environment"], "production");

    let create_audit_row = sqlx::query(
        "SELECT metadata \
         FROM audit_logs \
         WHERE action = 'trusted_publisher_create'::audit_action AND target_package_id = $1 \
         ORDER BY occurred_at DESC \
         LIMIT 1",
    )
    .bind(package_id)
    .fetch_one(&pool)
    .await
    .expect("trusted publisher create audit row should exist");
    let create_metadata: Value = create_audit_row
        .try_get("metadata")
        .expect("create audit metadata should be readable");
    assert_eq!(create_metadata["trusted_publisher_id"], publisher_id);
    assert_eq!(
        create_metadata["issuer"],
        "https://token.actions.githubusercontent.com"
    );
    assert_eq!(
        create_metadata["subject"],
        "repo:acme/demo-widget:ref:refs/heads/main"
    );
    assert_eq!(create_metadata["repository"], "acme/demo-widget");
    assert_eq!(
        create_metadata["workflow_ref"],
        ".github/workflows/publish.yml@refs/heads/main"
    );
    assert_eq!(create_metadata["environment"], "production");

    let (status, delete_body) = delete_trusted_publisher_for_package(
        &app,
        &alice_jwt,
        "pypi",
        "demo-widget",
        &publisher_id,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected delete response: {delete_body}"
    );
    assert_eq!(delete_body["message"], "Trusted publisher removed");

    let (status, empty_list_body) =
        list_trusted_publishers_for_package(&app, None, "pypi", "demo-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(empty_list_body["trusted_publishers"], json!([]));

    let delete_audit_row = sqlx::query(
        "SELECT metadata \
         FROM audit_logs \
         WHERE action = 'trusted_publisher_delete'::audit_action AND target_package_id = $1 \
         ORDER BY occurred_at DESC \
         LIMIT 1",
    )
    .bind(package_id)
    .fetch_one(&pool)
    .await
    .expect("trusted publisher delete audit row should exist");
    let delete_metadata: Value = delete_audit_row
        .try_get("metadata")
        .expect("delete audit metadata should be readable");
    assert_eq!(delete_metadata["trusted_publisher_id"], publisher_id);
    assert_eq!(
        delete_metadata["issuer"],
        "https://token.actions.githubusercontent.com"
    );
    assert_eq!(
        delete_metadata["subject"],
        "repo:acme/demo-widget:ref:refs/heads/main"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_trusted_publisher_mutations_require_package_admin_access(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, repository_body) = create_repository(
        &app,
        &alice_jwt,
        "Alice PyPI Packages",
        "alice-pypi-packages",
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, package_body) = create_package_with_options(
        &app,
        &alice_jwt,
        "pypi",
        "restricted-widget",
        "alice-pypi-packages",
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );

    let (status, denied_create_body) = create_trusted_publisher_for_package(
        &app,
        &bob_jwt,
        "pypi",
        "restricted-widget",
        json!({
            "issuer": "https://token.actions.githubusercontent.com",
            "subject": "repo:acme/restricted-widget:ref:refs/heads/main",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(denied_create_body["error"]
        .as_str()
        .expect("error should be present")
        .contains("administration permission"));

    let (status, create_body) = create_trusted_publisher_for_package(
        &app,
        &alice_jwt,
        "pypi",
        "restricted-widget",
        json!({
            "issuer": "https://token.actions.githubusercontent.com",
            "subject": "repo:acme/restricted-widget:ref:refs/heads/main",
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected trusted publisher response: {create_body}"
    );
    let publisher_id = create_body["id"]
        .as_str()
        .expect("publisher id should be returned")
        .to_owned();

    let (status, denied_delete_body) = delete_trusted_publisher_for_package(
        &app,
        &bob_jwt,
        "pypi",
        "restricted-widget",
        &publisher_id,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(denied_delete_body["error"]
        .as_str()
        .expect("error should be present")
        .contains("administration permission"));

    let (status, list_body) =
        list_trusted_publishers_for_package(&app, Some(&bob_jwt), "pypi", "restricted-widget")
            .await;
    assert_eq!(status, StatusCode::OK);
    let publishers = list_body["trusted_publishers"]
        .as_array()
        .expect("trusted publishers response should be an array");
    assert_eq!(publishers.len(), 1, "response: {list_body}");
    assert_eq!(publishers[0]["id"], publisher_id);
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_team_transfer_permission_surfaces_in_package_detail_and_allows_transfer(
    pool: PgPool,
) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, source_org_body) = create_org(&app, &alice_jwt, "Source Org", "source-org").await;
    assert_eq!(status, StatusCode::CREATED);
    let source_org_id = source_org_body["id"].as_str().expect("source org id");

    let (status, _) = create_org(&app, &bob_jwt, "Target Org", "target-org").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "Source Packages",
        "source-packages",
        Some(source_org_id),
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, package_body) = create_package_with_options(
        &app,
        &alice_jwt,
        "npm",
        "transfer-widget",
        "source-packages",
        Some("private"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );

    let (status, _) = add_org_member(&app, &alice_jwt, "source-org", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_team(
        &app,
        &alice_jwt,
        "source-org",
        "Transfer Team",
        "transfer-team",
        Some("Owns controlled package handoff workflows."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) =
        add_team_member_to_team(&app, &alice_jwt, "source-org", "transfer-team", "bob").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, detail_before_grant) =
        get_package_detail(&app, Some(&bob_jwt), "npm", "transfer-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail_before_grant["owner_org_slug"], "source-org");
    assert_eq!(detail_before_grant["can_transfer"], false);

    let (status, grant_body) = grant_team_package_access(
        &app,
        &alice_jwt,
        "source-org",
        "transfer-team",
        "npm",
        "transfer-widget",
        &["transfer_ownership"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected team package access response: {grant_body}"
    );

    let (status, detail_after_grant) =
        get_package_detail(&app, Some(&bob_jwt), "npm", "transfer-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail_after_grant["can_transfer"], true);

    let (status, transfer_body) =
        transfer_package_ownership(&app, &bob_jwt, "npm", "transfer-widget", "target-org").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected package transfer response: {transfer_body}"
    );
    assert_eq!(transfer_body["message"], "Package ownership transferred");
    assert_eq!(transfer_body["owner"]["slug"], "target-org");

    let (status, detail_after_transfer) =
        get_package_detail(&app, Some(&bob_jwt), "npm", "transfer-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail_after_transfer["owner_org_slug"], "target-org");
    assert_eq!(detail_after_transfer["can_transfer"], true);
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_team_transfer_requires_transfer_specific_permission(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, source_org_body) = create_org(&app, &alice_jwt, "Source Org", "source-org").await;
    assert_eq!(status, StatusCode::CREATED);
    let source_org_id = source_org_body["id"].as_str().expect("source org id");

    let (status, _) = create_org(&app, &bob_jwt, "Target Org", "target-org").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "Source Packages",
        "source-packages",
        Some(source_org_id),
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, package_body) = create_package_with_options(
        &app,
        &alice_jwt,
        "npm",
        "limited-transfer-widget",
        "source-packages",
        Some("private"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );

    let (status, _) = add_org_member(&app, &alice_jwt, "source-org", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_team(
        &app,
        &alice_jwt,
        "source-org",
        "Release Team",
        "release-team",
        Some("Can publish releases but cannot transfer ownership."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) =
        add_team_member_to_team(&app, &alice_jwt, "source-org", "release-team", "bob").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, grant_body) = grant_team_package_access(
        &app,
        &alice_jwt,
        "source-org",
        "release-team",
        "npm",
        "limited-transfer-widget",
        &["publish"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected team package access response: {grant_body}"
    );

    let (status, detail_before_transfer) =
        get_package_detail(&app, Some(&bob_jwt), "npm", "limited-transfer-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail_before_transfer["can_transfer"], false);

    let (status, denied_transfer_body) = transfer_package_ownership(
        &app,
        &bob_jwt,
        "npm",
        "limited-transfer-widget",
        "target-org",
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(denied_transfer_body["error"]
        .as_str()
        .expect("error should be present")
        .contains("transfer ownership"));

    let (status, detail_after_transfer_attempt) =
        get_package_detail(&app, Some(&alice_jwt), "npm", "limited-transfer-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        detail_after_transfer_attempt["owner_org_slug"],
        "source-org"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_package_transfer_clears_stale_team_package_access_rows(pool: PgPool) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let alice_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, source_org_body) = create_org(&app, &alice_jwt, "Source Org", "source-org").await;
    assert_eq!(status, StatusCode::CREATED);
    let source_org_id = source_org_body["id"].as_str().expect("source org id");

    let (status, _) = create_org(&app, &alice_jwt, "Target Org", "target-org").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, repository_body) = create_repository_with_options(
        &app,
        &alice_jwt,
        "Source Packages",
        "source-packages",
        Some(source_org_id),
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected repository response: {repository_body}"
    );

    let (status, package_body) = create_package_with_options(
        &app,
        &alice_jwt,
        "npm",
        "cleanup-widget",
        "source-packages",
        Some("private"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected package response: {package_body}"
    );
    let package_id = uuid::Uuid::parse_str(
        package_body["id"]
            .as_str()
            .expect("package id should be returned"),
    )
    .expect("package id should be a UUID");

    let (status, _) = add_org_member(&app, &alice_jwt, "source-org", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_team(
        &app,
        &alice_jwt,
        "source-org",
        "Cleanup Team",
        "cleanup-team",
        Some("Receives temporary delegated package access before transfer."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) =
        add_team_member_to_team(&app, &alice_jwt, "source-org", "cleanup-team", "bob").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, grant_body) = grant_team_package_access(
        &app,
        &alice_jwt,
        "source-org",
        "cleanup-team",
        "npm",
        "cleanup-widget",
        &["publish", "write_metadata"],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected team package access response: {grant_body}"
    );

    let team_grants_before_transfer: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM team_package_access WHERE package_id = $1",
    )
    .bind(package_id)
    .fetch_one(&pool)
    .await
    .expect("team package access count before transfer");
    assert_eq!(team_grants_before_transfer, 2);

    let (status, transfer_body) =
        transfer_package_ownership(&app, &alice_jwt, "npm", "cleanup-widget", "target-org").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected package transfer response: {transfer_body}"
    );
    assert_eq!(transfer_body["owner"]["slug"], "target-org");

    let team_grants_after_transfer: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM team_package_access WHERE package_id = $1",
    )
    .bind(package_id)
    .fetch_one(&pool)
    .await
    .expect("team package access count after transfer");
    assert_eq!(team_grants_after_transfer, 0);

    let (status, _) = get_package_detail(&app, Some(&bob_jwt), "npm", "cleanup-widget").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, owner_detail) =
        get_package_detail(&app, Some(&alice_jwt), "npm", "cleanup-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(owner_detail["owner_org_slug"], "target-org");
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
    let (status, _) =
        register_user(&app, "owner_user", "owner@example.com", "Str0ngP@ssword!").await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) =
        register_user(&app, "target_user", "target@example.com", "Str0ngP@ssword!").await;
    assert_eq!(status, StatusCode::OK);

    let owner_jwt = login_user(&app, "owner_user", "Str0ngP@ssword!").await;

    // Create org (owner_user becomes owner)
    let (status, _) = create_org(&app, &owner_jwt, "Transfer Org", "transfer-org").await;
    assert_eq!(status, StatusCode::CREATED);

    // Add target_user as a maintainer
    let (status, _) = add_org_member(
        &app,
        &owner_jwt,
        "transfer-org",
        "target_user",
        "maintainer",
    )
    .await;
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

    let (status, _) = register_user(
        &app,
        "selfowner",
        "selfowner@example.com",
        "Str0ngP@ssword!",
    )
    .await;
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

    let (status, _) = register_user(
        &app,
        "real_owner",
        "real_owner@example.com",
        "Str0ngP@ssword!",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = register_user(
        &app,
        "admin_user",
        "admin_user@example.com",
        "Str0ngP@ssword!",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = register_user(
        &app,
        "bystander",
        "bystander@example.com",
        "Str0ngP@ssword!",
    )
    .await;
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

    let (status, _) =
        register_user(&app, "alice_ot", "alice_ot@example.com", "Str0ngP@ssword!").await;
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

// ══════════════════════════════════════════════════════════════════════════════
// Security finding resolve / reopen
// ══════════════════════════════════════════════════════════════════════════════

/// Send a PATCH request to the security-finding update endpoint and return
/// `(status, body)`.
async fn update_security_finding_request(
    app: &axum::Router,
    jwt: &str,
    ecosystem: &str,
    package_name: &str,
    finding_id: uuid::Uuid,
    payload: Value,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::PATCH)
        .uri(format!(
            "/v1/packages/{ecosystem}/{package_name}/security-findings/{finding_id}"
        ))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(Body::from(payload.to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_security_finding_resolve_and_reopen_happy_path(pool: PgPool) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let owner_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, _) = create_repository_with_options(
        &app,
        &owner_jwt,
        "Personal Pkgs",
        "personal-pkgs",
        None,
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_package_with_options(
        &app,
        &owner_jwt,
        "npm",
        "finding-widget",
        "personal-pkgs",
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) =
        create_release_for_package(&app, &owner_jwt, "npm", "finding-widget", "1.0.0").await;
    assert_eq!(status, StatusCode::CREATED);

    let release_id = get_release_id(&pool, "npm", "finding-widget", "1.0.0").await;
    let finding_id = insert_security_finding(
        &pool,
        release_id,
        "vulnerability",
        "high",
        "A high severity finding",
        false,
    )
    .await;

    let (status, resolved_body) = update_security_finding_request(
        &app,
        &owner_jwt,
        "npm",
        "finding-widget",
        finding_id,
        serde_json::json!({ "is_resolved": true, "note": "mitigated upstream" }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected resolve response: {resolved_body}"
    );
    assert_eq!(resolved_body["is_resolved"], true);
    assert!(resolved_body["resolved_at"].is_string());
    assert!(resolved_body["resolved_by"].is_string());
    assert_eq!(resolved_body["release_version"], "1.0.0");

    let resolved_audit_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM audit_logs \
         WHERE action = 'security_finding_resolve' \
           AND target_release_id = $1",
    )
    .bind(release_id)
    .fetch_one(&pool)
    .await
    .expect("audit count should be queryable");
    assert_eq!(resolved_audit_count, 1);

    let resolved_target_org_id: Option<uuid::Uuid> = sqlx::query_scalar(
        "SELECT target_org_id FROM audit_logs \
         WHERE action = 'security_finding_resolve' \
           AND target_release_id = $1",
    )
    .bind(release_id)
    .fetch_one(&pool)
    .await
    .expect("resolved audit target org should be queryable");
    assert_eq!(resolved_target_org_id, None);

    let (status, reopened_body) = update_security_finding_request(
        &app,
        &owner_jwt,
        "npm",
        "finding-widget",
        finding_id,
        serde_json::json!({ "is_resolved": false }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected reopen response: {reopened_body}"
    );
    assert_eq!(reopened_body["is_resolved"], false);
    assert!(reopened_body["resolved_at"].is_null());
    assert!(reopened_body["resolved_by"].is_null());

    let reopened_audit_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM audit_logs \
         WHERE action = 'security_finding_reopen' \
           AND target_release_id = $1",
    )
    .bind(release_id)
    .fetch_one(&pool)
    .await
    .expect("audit count should be queryable");
    assert_eq!(reopened_audit_count, 1);

    let reopened_target_org_id: Option<uuid::Uuid> = sqlx::query_scalar(
        "SELECT target_org_id FROM audit_logs \
         WHERE action = 'security_finding_reopen' \
           AND target_release_id = $1",
    )
    .bind(release_id)
    .fetch_one(&pool)
    .await
    .expect("reopened audit target org should be queryable");
    assert_eq!(reopened_target_org_id, None);

    // No-op update (already reopened) must not emit another audit event.
    let (status, _) = update_security_finding_request(
        &app,
        &owner_jwt,
        "npm",
        "finding-widget",
        finding_id,
        serde_json::json!({ "is_resolved": false }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let reopened_audit_count_after_noop: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM audit_logs \
         WHERE action = 'security_finding_reopen' \
           AND target_release_id = $1",
    )
    .bind(release_id)
    .fetch_one(&pool)
    .await
    .expect("audit count should be queryable");
    assert_eq!(reopened_audit_count_after_noop, 1);
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_org_audit_includes_security_finding_triage_for_org_owned_packages(pool: PgPool) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let owner_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let reviewer_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &owner_jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id should be returned");

    let (status, _) = create_repository_with_options(
        &app,
        &owner_jwt,
        "Acme Public",
        "acme-public",
        Some(org_id),
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_package_with_options(
        &app,
        &owner_jwt,
        "npm",
        "acme-widget",
        "acme-public",
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) =
        create_release_for_package(&app, &owner_jwt, "npm", "acme-widget", "1.0.0").await;
    assert_eq!(status, StatusCode::CREATED);

    let release_id = get_release_id(&pool, "npm", "acme-widget", "1.0.0").await;
    let finding_id = insert_security_finding(
        &pool,
        release_id,
        "policy_violation",
        "medium",
        "License policy violation",
        false,
    )
    .await;

    let (status, _) = add_org_member(&app, &owner_jwt, "acme-corp", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_team(
        &app,
        &owner_jwt,
        "acme-corp",
        "Security Reviewers",
        "security-reviewers",
        Some("Triages security findings."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) =
        add_team_member_to_team(&app, &owner_jwt, "acme-corp", "security-reviewers", "bob").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = grant_team_repository_access(
        &app,
        &owner_jwt,
        "acme-corp",
        "security-reviewers",
        "acme-public",
        &["security_review"],
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, resolved_body) = update_security_finding_request(
        &app,
        &reviewer_jwt,
        "npm",
        "acme-widget",
        finding_id,
        serde_json::json!({ "is_resolved": true, "note": "policy waiver" }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected response: {resolved_body}"
    );

    let (status, reopened_body) = update_security_finding_request(
        &app,
        &reviewer_jwt,
        "npm",
        "acme-widget",
        finding_id,
        serde_json::json!({ "is_resolved": false, "note": "needs follow-up" }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected response: {reopened_body}"
    );

    let (status, _) = create_repository_with_options(
        &app,
        &owner_jwt,
        "Personal Pkgs",
        "personal-pkgs",
        None,
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_package_with_options(
        &app,
        &owner_jwt,
        "npm",
        "personal-widget",
        "personal-pkgs",
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) =
        create_release_for_package(&app, &owner_jwt, "npm", "personal-widget", "1.0.0").await;
    assert_eq!(status, StatusCode::CREATED);

    let personal_release_id = get_release_id(&pool, "npm", "personal-widget", "1.0.0").await;
    let personal_finding_id = insert_security_finding(
        &pool,
        personal_release_id,
        "vulnerability",
        "high",
        "Personal package issue",
        false,
    )
    .await;

    let (status, personal_resolved_body) = update_security_finding_request(
        &app,
        &owner_jwt,
        "npm",
        "personal-widget",
        personal_finding_id,
        serde_json::json!({ "is_resolved": true, "note": "personal-only triage" }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected personal resolve response: {personal_resolved_body}"
    );

    let (status, resolve_audit_body) = list_org_audit(
        &app,
        &owner_jwt,
        "acme-corp",
        Some("action=security_finding_resolve&per_page=20"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected org audit response: {resolve_audit_body}"
    );

    let resolve_logs = resolve_audit_body["logs"]
        .as_array()
        .expect("resolve audit logs response should be an array");
    assert_eq!(resolve_logs.len(), 1, "response: {resolve_audit_body}");
    let release_id_str = release_id.to_string();
    assert_eq!(resolve_logs[0]["action"], "security_finding_resolve");
    assert_eq!(resolve_logs[0]["actor_username"], "bob");
    assert_eq!(resolve_logs[0]["target_org_id"].as_str(), Some(org_id));
    assert_eq!(
        resolve_logs[0]["target_release_id"].as_str(),
        Some(release_id_str.as_str())
    );
    assert_eq!(resolve_logs[0]["metadata"]["package_name"], "acme-widget");
    assert_eq!(resolve_logs[0]["metadata"]["release_version"], "1.0.0");
    assert_eq!(resolve_logs[0]["metadata"]["note"], "policy waiver");
    assert!(resolve_logs.iter().all(|log| {
        match log["metadata"]["package_name"].as_str() {
            Some(package_name) => package_name != "personal-widget",
            None => true,
        }
    }));

    let bob_user_id: String = sqlx::query_scalar("SELECT id::text FROM users WHERE username = $1")
        .bind("bob")
        .fetch_one(&pool)
        .await
        .expect("bob user id should be queryable");

    let actor_query = format!("actor_user_id={bob_user_id}&per_page=20");
    let (status, actor_audit_body) =
        list_org_audit(&app, &owner_jwt, "acme-corp", Some(&actor_query)).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected actor-filtered org audit response: {actor_audit_body}"
    );

    let actor_logs = actor_audit_body["logs"]
        .as_array()
        .expect("actor-filtered logs response should be an array");
    assert_eq!(actor_logs.len(), 2, "response: {actor_audit_body}");
    assert!(actor_logs.iter().all(|log| log["actor_username"] == "bob"));

    let mut actor_actions = actor_logs
        .iter()
        .map(|log| {
            log["action"]
                .as_str()
                .expect("action should be present")
                .to_owned()
        })
        .collect::<Vec<_>>();
    actor_actions.sort();
    assert_eq!(
        actor_actions,
        vec![
            "security_finding_reopen".to_owned(),
            "security_finding_resolve".to_owned(),
        ]
    );

    let reopen_export_query = format!("actor_user_id={bob_user_id}&action=security_finding_reopen");
    let reopen_export_resp =
        export_org_audit_csv(&app, &owner_jwt, "acme-corp", Some(&reopen_export_query)).await;
    assert_eq!(reopen_export_resp.status(), StatusCode::OK);

    let reopen_export_body = body_text(reopen_export_resp).await;
    let reopen_export_lines = reopen_export_body.lines().collect::<Vec<_>>();
    assert_eq!(
        reopen_export_lines.len(),
        2,
        "unexpected CSV export body: {reopen_export_body}"
    );
    assert!(reopen_export_lines[1].contains(",security_finding_reopen,"));
    assert!(reopen_export_lines[1].contains(org_id));
    assert!(reopen_export_body.contains("acme-widget"));
    assert!(reopen_export_body.contains("needs follow-up"));
    assert!(!reopen_export_body.contains("security_finding_resolve"));
    assert!(!reopen_export_body.contains("personal-widget"));
    assert!(!reopen_export_body.contains("personal-only triage"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_release_lifecycle_audit_sets_target_org_id_for_org_owned_packages(pool: PgPool) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let owner_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &owner_jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id_str = org_body["id"]
        .as_str()
        .expect("org id should be returned")
        .to_owned();
    let org_id = uuid::Uuid::parse_str(&org_id_str).expect("org id should be a uuid");

    let (status, _) = create_repository_with_options(
        &app,
        &owner_jwt,
        "Acme Public",
        "acme-public",
        Some(&org_id_str),
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_package_with_options(
        &app,
        &owner_jwt,
        "npm",
        "acme-release-widget",
        "acme-public",
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) =
        create_release_for_package(&app, &owner_jwt, "npm", "acme-release-widget", "1.0.0").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, upload_body) = upload_release_artifact(
        &app,
        &owner_jwt,
        "npm",
        "acme-release-widget",
        "1.0.0",
        "acme-release-widget-1.0.0.tgz",
        "tarball",
        "application/octet-stream",
        b"release artifact bytes",
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected upload response: {upload_body}"
    );

    let (status, publish_body) =
        publish_release_for_package(&app, &owner_jwt, "npm", "acme-release-widget", "1.0.0").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected publish response: {publish_body}"
    );

    promote_release_to_published(&pool, "npm", "acme-release-widget", "1.0.0").await;

    let release_id = get_release_id(&pool, "npm", "acme-release-widget", "1.0.0").await;

    let (status, _) = yank_release_for_package(
        &app,
        &owner_jwt,
        "npm",
        "acme-release-widget",
        "1.0.0",
        Some("CVE-2026-0001"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, unyank_body) =
        unyank_release_for_package(&app, &owner_jwt, "npm", "acme-release-widget", "1.0.0").await;
    assert_eq!(status, StatusCode::OK, "unyank body: {unyank_body}");

    let (status, _) = deprecate_release_for_package(
        &app,
        &owner_jwt,
        "npm",
        "acme-release-widget",
        "1.0.0",
        Some("use v2 instead"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, undeprecate_body) =
        undeprecate_release_for_package(&app, &owner_jwt, "npm", "acme-release-widget", "1.0.0")
            .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "undeprecate body: {undeprecate_body}"
    );

    for action in [
        "release_publish",
        "release_yank",
        "release_unyank",
        "release_deprecate",
        "release_undeprecate",
    ] {
        let target_org_id: Option<uuid::Uuid> = sqlx::query_scalar(
            "SELECT target_org_id FROM audit_logs \
             WHERE action = $1::audit_action AND target_release_id = $2",
        )
        .bind(action)
        .bind(release_id)
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|_| panic!("audit row for {action} should exist"));
        assert_eq!(
            target_org_id,
            Some(org_id),
            "{action} audit row should carry the org id"
        );
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_release_lifecycle_audit_leaves_target_org_id_null_for_personal_packages(
    pool: PgPool,
) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let owner_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, _) = create_repository_with_options(
        &app,
        &owner_jwt,
        "Personal Pkgs",
        "personal-pkgs",
        None,
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_package_with_options(
        &app,
        &owner_jwt,
        "npm",
        "personal-release-widget",
        "personal-pkgs",
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) =
        create_release_for_package(&app, &owner_jwt, "npm", "personal-release-widget", "1.0.0")
            .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = upload_release_artifact(
        &app,
        &owner_jwt,
        "npm",
        "personal-release-widget",
        "1.0.0",
        "personal-release-widget-1.0.0.tgz",
        "tarball",
        "application/octet-stream",
        b"release artifact bytes",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) =
        publish_release_for_package(&app, &owner_jwt, "npm", "personal-release-widget", "1.0.0")
            .await;
    assert_eq!(status, StatusCode::OK);

    promote_release_to_published(&pool, "npm", "personal-release-widget", "1.0.0").await;

    let release_id = get_release_id(&pool, "npm", "personal-release-widget", "1.0.0").await;

    let (status, _) = yank_release_for_package(
        &app,
        &owner_jwt,
        "npm",
        "personal-release-widget",
        "1.0.0",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) =
        unyank_release_for_package(&app, &owner_jwt, "npm", "personal-release-widget", "1.0.0")
            .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = deprecate_release_for_package(
        &app,
        &owner_jwt,
        "npm",
        "personal-release-widget",
        "1.0.0",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = undeprecate_release_for_package(
        &app,
        &owner_jwt,
        "npm",
        "personal-release-widget",
        "1.0.0",
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    for action in [
        "release_publish",
        "release_yank",
        "release_unyank",
        "release_deprecate",
        "release_undeprecate",
    ] {
        let target_org_id: Option<uuid::Uuid> = sqlx::query_scalar(
            "SELECT target_org_id FROM audit_logs \
             WHERE action = $1::audit_action AND target_release_id = $2",
        )
        .bind(action)
        .bind(release_id)
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|_| panic!("audit row for {action} should exist"));
        assert_eq!(
            target_org_id, None,
            "{action} audit row for a personal package should leave target_org_id NULL"
        );
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_org_audit_includes_release_lifecycle_events_for_org_owned_packages(pool: PgPool) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let owner_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &owner_jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id should be returned");

    let (status, _) = create_repository_with_options(
        &app,
        &owner_jwt,
        "Acme Public",
        "acme-public",
        Some(org_id),
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_package_with_options(
        &app,
        &owner_jwt,
        "npm",
        "acme-lifecycle-widget",
        "acme-public",
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) =
        create_release_for_package(&app, &owner_jwt, "npm", "acme-lifecycle-widget", "1.0.0").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = upload_release_artifact(
        &app,
        &owner_jwt,
        "npm",
        "acme-lifecycle-widget",
        "1.0.0",
        "acme-lifecycle-widget-1.0.0.tgz",
        "tarball",
        "application/octet-stream",
        b"release artifact bytes",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) =
        publish_release_for_package(&app, &owner_jwt, "npm", "acme-lifecycle-widget", "1.0.0")
            .await;
    assert_eq!(status, StatusCode::OK);

    promote_release_to_published(&pool, "npm", "acme-lifecycle-widget", "1.0.0").await;

    let (status, _) = yank_release_for_package(
        &app,
        &owner_jwt,
        "npm",
        "acme-lifecycle-widget",
        "1.0.0",
        Some("supply chain issue"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = deprecate_release_for_package(
        &app,
        &owner_jwt,
        "npm",
        "acme-lifecycle-widget",
        "1.0.0",
        Some("prefer 2.0.0"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) =
        undeprecate_release_for_package(&app, &owner_jwt, "npm", "acme-lifecycle-widget", "1.0.0")
            .await;
    assert_eq!(status, StatusCode::OK);

    // Also publish a personal-package release that must NOT appear in org audit.
    let (status, _) = create_repository_with_options(
        &app,
        &owner_jwt,
        "Personal Pkgs",
        "personal-pkgs",
        None,
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_package_with_options(
        &app,
        &owner_jwt,
        "npm",
        "personal-lifecycle-widget",
        "personal-pkgs",
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_release_for_package(
        &app,
        &owner_jwt,
        "npm",
        "personal-lifecycle-widget",
        "1.0.0",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = upload_release_artifact(
        &app,
        &owner_jwt,
        "npm",
        "personal-lifecycle-widget",
        "1.0.0",
        "personal-lifecycle-widget-1.0.0.tgz",
        "tarball",
        "application/octet-stream",
        b"personal release artifact bytes",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = publish_release_for_package(
        &app,
        &owner_jwt,
        "npm",
        "personal-lifecycle-widget",
        "1.0.0",
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    promote_release_to_published(&pool, "npm", "personal-lifecycle-widget", "1.0.0").await;

    let (status, _) = deprecate_release_for_package(
        &app,
        &owner_jwt,
        "npm",
        "personal-lifecycle-widget",
        "1.0.0",
        Some("personal migration"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = undeprecate_release_for_package(
        &app,
        &owner_jwt,
        "npm",
        "personal-lifecycle-widget",
        "1.0.0",
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, publish_audit_body) = list_org_audit(
        &app,
        &owner_jwt,
        "acme-corp",
        Some("action=release_publish&per_page=20"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let publish_logs = publish_audit_body["logs"]
        .as_array()
        .expect("publish audit logs response should be an array");
    assert_eq!(publish_logs.len(), 1, "response: {publish_audit_body}");
    assert_eq!(publish_logs[0]["action"], "release_publish");
    assert_eq!(publish_logs[0]["target_org_id"].as_str(), Some(org_id));
    assert_eq!(publish_logs[0]["metadata"]["name"], "acme-lifecycle-widget");
    assert_eq!(publish_logs[0]["metadata"]["version"], "1.0.0");

    let (status, yank_audit_body) = list_org_audit(
        &app,
        &owner_jwt,
        "acme-corp",
        Some("action=release_yank&per_page=20"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let yank_logs = yank_audit_body["logs"]
        .as_array()
        .expect("yank audit logs response should be an array");
    assert_eq!(yank_logs.len(), 1, "response: {yank_audit_body}");
    assert_eq!(yank_logs[0]["action"], "release_yank");
    assert_eq!(yank_logs[0]["target_org_id"].as_str(), Some(org_id));
    assert_eq!(yank_logs[0]["metadata"]["reason"], "supply chain issue");

    let (status, undeprecate_audit_body) = list_org_audit(
        &app,
        &owner_jwt,
        "acme-corp",
        Some("action=release_undeprecate&per_page=20"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let undeprecate_logs = undeprecate_audit_body["logs"]
        .as_array()
        .expect("undeprecate audit logs response should be an array");
    assert_eq!(
        undeprecate_logs.len(),
        1,
        "response: {undeprecate_audit_body}"
    );
    assert_eq!(undeprecate_logs[0]["action"], "release_undeprecate");
    assert_eq!(undeprecate_logs[0]["target_org_id"].as_str(), Some(org_id));
    assert_eq!(
        undeprecate_logs[0]["metadata"]["name"],
        "acme-lifecycle-widget"
    );
    assert_eq!(undeprecate_logs[0]["metadata"]["version"], "1.0.0");
    assert_eq!(undeprecate_logs[0]["metadata"]["restored_status"], "yanked");

    let publish_export_resp = export_org_audit_csv(
        &app,
        &owner_jwt,
        "acme-corp",
        Some("action=release_publish"),
    )
    .await;
    assert_eq!(publish_export_resp.status(), StatusCode::OK);
    let publish_export_body = body_text(publish_export_resp).await;
    let publish_export_lines = publish_export_body.lines().collect::<Vec<_>>();
    assert_eq!(
        publish_export_lines.len(),
        2,
        "unexpected CSV export body: {publish_export_body}"
    );
    assert!(publish_export_lines[1].contains(",release_publish,"));
    assert!(publish_export_lines[1].contains(org_id));
    assert!(publish_export_body.contains("acme-lifecycle-widget"));
    assert!(!publish_export_body.contains("personal-lifecycle-widget"));

    let undeprecate_export_resp = export_org_audit_csv(
        &app,
        &owner_jwt,
        "acme-corp",
        Some("action=release_undeprecate"),
    )
    .await;
    assert_eq!(undeprecate_export_resp.status(), StatusCode::OK);
    let undeprecate_export_body = body_text(undeprecate_export_resp).await;
    let undeprecate_export_lines = undeprecate_export_body.lines().collect::<Vec<_>>();
    assert_eq!(
        undeprecate_export_lines.len(),
        2,
        "unexpected CSV export body: {undeprecate_export_body}"
    );
    assert!(undeprecate_export_lines[1].contains(",release_undeprecate,"));
    assert!(undeprecate_export_lines[1].contains(org_id));
    assert!(undeprecate_export_body.contains("acme-lifecycle-widget"));
    assert!(undeprecate_export_body.contains("restored_status"));
    assert!(undeprecate_export_body.contains("yanked"));
    assert!(!undeprecate_export_body.contains("personal-lifecycle-widget"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_package_tag_lifecycle_supports_create_update_and_delete(pool: PgPool) {
    let app = app(pool);
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let owner_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, _) = create_repository_with_options(
        &app,
        &owner_jwt,
        "Personal Pkgs",
        "personal-pkgs",
        None,
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_package_with_options(
        &app,
        &owner_jwt,
        "npm",
        "taggable-widget",
        "personal-pkgs",
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) =
        create_release_for_package(&app, &owner_jwt, "npm", "taggable-widget", "1.0.0").await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) =
        create_release_for_package(&app, &owner_jwt, "npm", "taggable-widget", "1.1.0").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, initial_tags) = list_package_tags(&app, None, "npm", "taggable-widget").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected list response: {initial_tags}"
    );
    assert_eq!(initial_tags["tags"], json!({}));

    let (status, create_body) = upsert_package_tag(
        &app,
        &owner_jwt,
        "npm",
        "taggable-widget",
        "latest",
        "1.0.0",
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected create tag response: {create_body}"
    );
    assert_eq!(create_body["message"], "Tag updated");
    assert_eq!(create_body["tag"], "latest");
    assert_eq!(create_body["version"], "1.0.0");

    let (status, created_tags) = list_package_tags(&app, None, "npm", "taggable-widget").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected list response: {created_tags}"
    );
    assert_eq!(created_tags["tags"]["latest"]["version"], "1.0.0");
    assert!(created_tags["tags"]["latest"]["updated_at"].is_string());

    let (status, update_body) = upsert_package_tag(
        &app,
        &owner_jwt,
        "npm",
        "taggable-widget",
        "latest",
        "1.1.0",
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected update tag response: {update_body}"
    );
    assert_eq!(update_body["version"], "1.1.0");

    let (status, updated_tags) = list_package_tags(&app, None, "npm", "taggable-widget").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected list response: {updated_tags}"
    );
    assert_eq!(updated_tags["tags"]["latest"]["version"], "1.1.0");

    let (status, delete_body) =
        delete_package_tag(&app, &owner_jwt, "npm", "taggable-widget", "latest").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected delete tag response: {delete_body}"
    );
    assert_eq!(delete_body["message"], "Tag deleted");
    assert_eq!(delete_body["tag"], "latest");

    let (status, final_tags) = list_package_tags(&app, None, "npm", "taggable-widget").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected list response: {final_tags}"
    );
    assert_eq!(final_tags["tags"], json!({}));

    let (status, missing_delete_body) =
        delete_package_tag(&app, &owner_jwt, "npm", "taggable-widget", "latest").await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "unexpected delete missing tag response: {missing_delete_body}"
    );
    assert!(missing_delete_body["error"]
        .as_str()
        .expect("missing tag error should be returned")
        .contains("Tag 'latest' not found"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_undeprecate_release_restores_previous_visible_status(pool: PgPool) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let owner_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, _) = create_repository_with_options(
        &app,
        &owner_jwt,
        "Personal Pkgs",
        "personal-pkgs",
        None,
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_package_with_options(
        &app,
        &owner_jwt,
        "npm",
        "undeprecate-widget",
        "personal-pkgs",
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) =
        create_release_for_package(&app, &owner_jwt, "npm", "undeprecate-widget", "1.0.0").await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) =
        create_release_for_package(&app, &owner_jwt, "npm", "undeprecate-widget", "2.0.0").await;
    assert_eq!(status, StatusCode::CREATED);
    promote_release_to_published(&pool, "npm", "undeprecate-widget", "1.0.0").await;
    promote_release_to_published(&pool, "npm", "undeprecate-widget", "2.0.0").await;

    let (status, _) = deprecate_release_for_package(
        &app,
        &owner_jwt,
        "npm",
        "undeprecate-widget",
        "1.0.0",
        Some("use 2.0.0"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, undeprecate_body) =
        undeprecate_release_for_package(&app, &owner_jwt, "npm", "undeprecate-widget", "1.0.0")
            .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected undeprecate response: {undeprecate_body}"
    );
    assert_eq!(undeprecate_body["message"], "Release undeprecated");
    assert_eq!(undeprecate_body["status"], "published");

    let (status, release_body) =
        get_release_detail(&app, Some(&owner_jwt), "npm", "undeprecate-widget", "1.0.0").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected release detail: {release_body}"
    );
    assert_eq!(release_body["status"], "published");
    assert_eq!(release_body["is_deprecated"], false);
    assert_eq!(release_body["deprecation_message"], Value::Null);

    let (status, _) = deprecate_release_for_package(
        &app,
        &owner_jwt,
        "npm",
        "undeprecate-widget",
        "2.0.0",
        Some("paused rollout"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = yank_release_for_package(
        &app,
        &owner_jwt,
        "npm",
        "undeprecate-widget",
        "2.0.0",
        Some("investigating regression"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, yanked_undeprecate_body) =
        undeprecate_release_for_package(&app, &owner_jwt, "npm", "undeprecate-widget", "2.0.0")
            .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected yanked undeprecate response: {yanked_undeprecate_body}"
    );
    assert_eq!(yanked_undeprecate_body["status"], "yanked");

    let (status, yanked_release_body) =
        get_release_detail(&app, Some(&owner_jwt), "npm", "undeprecate-widget", "2.0.0").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected yanked release detail: {yanked_release_body}"
    );
    assert_eq!(yanked_release_body["status"], "yanked");
    assert_eq!(yanked_release_body["is_yanked"], true);
    assert_eq!(yanked_release_body["is_deprecated"], false);
    assert_eq!(yanked_release_body["deprecation_message"], Value::Null);

    let (status, conflict_body) =
        undeprecate_release_for_package(&app, &owner_jwt, "npm", "undeprecate-widget", "1.0.0")
            .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(conflict_body["error"]
        .as_str()
        .expect("conflict error should be present")
        .contains("not deprecated"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_security_finding_update_forbidden_for_unrelated_user(pool: PgPool) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let owner_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let other_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, _) = create_repository_with_options(
        &app,
        &owner_jwt,
        "Personal Pkgs",
        "personal-pkgs",
        None,
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_package_with_options(
        &app,
        &owner_jwt,
        "npm",
        "finding-widget",
        "personal-pkgs",
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) =
        create_release_for_package(&app, &owner_jwt, "npm", "finding-widget", "1.0.0").await;
    assert_eq!(status, StatusCode::CREATED);

    let release_id = get_release_id(&pool, "npm", "finding-widget", "1.0.0").await;
    let finding_id = insert_security_finding(
        &pool,
        release_id,
        "vulnerability",
        "high",
        "A high severity finding",
        false,
    )
    .await;

    let (status, body) = update_security_finding_request(
        &app,
        &other_jwt,
        "npm",
        "finding-widget",
        finding_id,
        serde_json::json!({ "is_resolved": true }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "unexpected forbidden response: {body}"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_security_finding_update_404_when_finding_belongs_to_other_package(pool: PgPool) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let owner_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, _) = create_repository_with_options(
        &app,
        &owner_jwt,
        "Personal Pkgs",
        "personal-pkgs",
        None,
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_package_with_options(
        &app,
        &owner_jwt,
        "npm",
        "target-widget",
        "personal-pkgs",
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = create_package_with_options(
        &app,
        &owner_jwt,
        "npm",
        "other-widget",
        "personal-pkgs",
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) =
        create_release_for_package(&app, &owner_jwt, "npm", "target-widget", "1.0.0").await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) =
        create_release_for_package(&app, &owner_jwt, "npm", "other-widget", "1.0.0").await;
    assert_eq!(status, StatusCode::CREATED);

    let other_release_id = get_release_id(&pool, "npm", "other-widget", "1.0.0").await;
    let other_finding_id = insert_security_finding(
        &pool,
        other_release_id,
        "vulnerability",
        "medium",
        "Unrelated finding",
        false,
    )
    .await;

    // PATCH against the wrong package path must be 404, not a cross-package mutation.
    let (status, body) = update_security_finding_request(
        &app,
        &owner_jwt,
        "npm",
        "target-widget",
        other_finding_id,
        serde_json::json!({ "is_resolved": true }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "unexpected response: {body}");

    let still_unresolved: bool =
        sqlx::query_scalar("SELECT is_resolved FROM security_findings WHERE id = $1")
            .bind(other_finding_id)
            .fetch_one(&pool)
            .await
            .expect("finding row should be queryable");
    assert!(!still_unresolved);
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_security_finding_update_allowed_via_team_security_review_permission(pool: PgPool) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    let owner_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let bob_jwt = login_user(&app, "bob", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &owner_jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id");

    let (status, _) = create_repository_with_options(
        &app,
        &owner_jwt,
        "Acme Public",
        "acme-public",
        Some(org_id),
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_package_with_options(
        &app,
        &owner_jwt,
        "npm",
        "acme-widget",
        "acme-public",
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) =
        create_release_for_package(&app, &owner_jwt, "npm", "acme-widget", "1.0.0").await;
    assert_eq!(status, StatusCode::CREATED);

    let release_id = get_release_id(&pool, "npm", "acme-widget", "1.0.0").await;
    let finding_id = insert_security_finding(
        &pool,
        release_id,
        "policy_violation",
        "medium",
        "License policy violation",
        false,
    )
    .await;

    let (status, _) = add_org_member(&app, &owner_jwt, "acme-corp", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_team(
        &app,
        &owner_jwt,
        "acme-corp",
        "Security Reviewers",
        "security-reviewers",
        Some("Triages security findings."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) =
        add_team_member_to_team(&app, &owner_jwt, "acme-corp", "security-reviewers", "bob").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = grant_team_repository_access(
        &app,
        &owner_jwt,
        "acme-corp",
        "security-reviewers",
        "acme-public",
        &["security_review"],
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Delegated team-member Bob can resolve the finding via security_review permission.
    let (status, body) = update_security_finding_request(
        &app,
        &bob_jwt,
        "npm",
        "acme-widget",
        finding_id,
        serde_json::json!({ "is_resolved": true, "note": "policy waiver" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "unexpected response: {body}");
    assert_eq!(body["is_resolved"], true);

    let audit_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM audit_logs \
         WHERE action = 'security_finding_resolve' \
           AND target_release_id = $1 \
           AND metadata->>'note' = $2",
    )
    .bind(release_id)
    .bind("policy waiver")
    .fetch_one(&pool)
    .await
    .expect("audit count should be queryable");
    assert_eq!(audit_count, 1);
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_package_detail_reports_can_manage_security_for_security_reviewer(pool: PgPool) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    register_user(&app, "bob", "bob@test.dev", "super_secret_pw!").await;
    register_user(&app, "carol", "carol@test.dev", "super_secret_pw!").await;
    let owner_jwt = login_user(&app, "alice", "super_secret_pw!").await;
    let reviewer_jwt = login_user(&app, "bob", "super_secret_pw!").await;
    let unrelated_jwt = login_user(&app, "carol", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &owner_jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id = org_body["id"].as_str().expect("org id");

    let (status, _) = create_repository_with_options(
        &app,
        &owner_jwt,
        "Acme Public",
        "acme-public",
        Some(org_id),
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_package_with_options(
        &app,
        &owner_jwt,
        "npm",
        "acme-sec-widget",
        "acme-public",
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // Baseline: owner has can_manage_security = true.
    let (status, owner_detail) =
        get_package_detail(&app, Some(&owner_jwt), "npm", "acme-sec-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(owner_detail["can_manage_security"], true);

    // Anonymous readers and unrelated users must see can_manage_security = false.
    let (status, anon_detail) = get_package_detail(&app, None, "npm", "acme-sec-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(anon_detail["can_manage_security"], false);

    let (status, unrelated_detail) =
        get_package_detail(&app, Some(&unrelated_jwt), "npm", "acme-sec-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(unrelated_detail["can_manage_security"], false);
    assert_eq!(unrelated_detail["can_manage_releases"], false);

    // Bob is added to the org and to a team with repository-scoped security_review.
    let (status, _) = add_org_member(&app, &owner_jwt, "acme-corp", "bob", "viewer").await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = create_team(
        &app,
        &owner_jwt,
        "acme-corp",
        "Security Reviewers",
        "security-reviewers",
        Some("Triages findings."),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) =
        add_team_member_to_team(&app, &owner_jwt, "acme-corp", "security-reviewers", "bob").await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = grant_team_repository_access(
        &app,
        &owner_jwt,
        "acme-corp",
        "security-reviewers",
        "acme-public",
        &["security_review"],
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Bob now sees can_manage_security = true, but still cannot manage releases or metadata.
    let (status, reviewer_detail) =
        get_package_detail(&app, Some(&reviewer_jwt), "npm", "acme-sec-widget").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reviewer_detail["can_manage_security"], true);
    assert_eq!(reviewer_detail["can_manage_releases"], false);
    assert_eq!(reviewer_detail["can_manage_metadata"], false);
    assert_eq!(reviewer_detail["can_manage_trusted_publishers"], false);
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_package_lifecycle_audit_sets_target_org_id_for_org_owned_packages(pool: PgPool) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let owner_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, source_org_body) = create_org(&app, &owner_jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let source_org_id_str = source_org_body["id"]
        .as_str()
        .expect("source org id should be returned")
        .to_owned();
    let source_org_id =
        uuid::Uuid::parse_str(&source_org_id_str).expect("source org id should be a uuid");

    let (status, target_org_body) =
        create_org(&app, &owner_jwt, "Acme Successor", "acme-successor").await;
    assert_eq!(status, StatusCode::CREATED);
    let target_org_id_str = target_org_body["id"]
        .as_str()
        .expect("target org id should be returned")
        .to_owned();
    let target_org_id =
        uuid::Uuid::parse_str(&target_org_id_str).expect("target org id should be a uuid");

    let (status, _) = create_repository_with_options(
        &app,
        &owner_jwt,
        "Acme Public",
        "acme-public",
        Some(&source_org_id_str),
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // The target org must have at least one repository to receive transferred packages.
    let (status, _) = create_repository_with_options(
        &app,
        &owner_jwt,
        "Acme Successor Repo",
        "acme-successor-repo",
        Some(&target_org_id_str),
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, create_body) = create_package_with_options(
        &app,
        &owner_jwt,
        "npm",
        "acme-lifecycle-package",
        "acme-public",
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let package_id_str = create_body["id"]
        .as_str()
        .expect("package id should be returned")
        .to_owned();
    let package_id = uuid::Uuid::parse_str(&package_id_str).expect("package id should be a uuid");

    let create_target_org_id: Option<uuid::Uuid> = sqlx::query_scalar(
        "SELECT target_org_id FROM audit_logs \
         WHERE action = 'package_create'::audit_action AND target_package_id = $1",
    )
    .bind(package_id)
    .fetch_one(&pool)
    .await
    .expect("package_create audit row should exist");
    assert_eq!(
        create_target_org_id,
        Some(source_org_id),
        "package_create audit row should carry the source org id"
    );

    // Create a second package that we will transfer to the other org.
    let (status, transfer_body) = create_package_with_options(
        &app,
        &owner_jwt,
        "npm",
        "acme-transfer-package",
        "acme-public",
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let transfer_package_id_str = transfer_body["id"]
        .as_str()
        .expect("transfer package id should be returned")
        .to_owned();
    let transfer_package_id = uuid::Uuid::parse_str(&transfer_package_id_str)
        .expect("transfer package id should be a uuid");

    let (status, transfer_resp) = transfer_package_ownership(
        &app,
        &owner_jwt,
        "npm",
        "acme-transfer-package",
        "acme-successor",
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected transfer response: {transfer_resp}"
    );

    let transfer_target_org_id: Option<uuid::Uuid> = sqlx::query_scalar(
        "SELECT target_org_id FROM audit_logs \
         WHERE action = 'package_transfer'::audit_action AND target_package_id = $1",
    )
    .bind(transfer_package_id)
    .fetch_one(&pool)
    .await
    .expect("package_transfer audit row should exist");
    assert_eq!(
        transfer_target_org_id,
        Some(target_org_id),
        "package_transfer audit row should carry the new owner org id"
    );

    // Archive the first package; the audit row should carry the still-source org id.
    let (status, delete_resp) =
        delete_package_for_ecosystem(&app, &owner_jwt, "npm", "acme-lifecycle-package").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected delete response: {delete_resp}"
    );

    let delete_target_org_id: Option<uuid::Uuid> = sqlx::query_scalar(
        "SELECT target_org_id FROM audit_logs \
         WHERE action = 'package_delete'::audit_action AND target_package_id = $1",
    )
    .bind(package_id)
    .fetch_one(&pool)
    .await
    .expect("package_delete audit row should exist");
    assert_eq!(
        delete_target_org_id,
        Some(source_org_id),
        "package_delete audit row should carry the owner org id"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_package_lifecycle_audit_leaves_target_org_id_null_for_personal_packages(
    pool: PgPool,
) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let owner_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, _) = create_repository_with_options(
        &app,
        &owner_jwt,
        "Personal Pkgs",
        "personal-pkgs",
        None,
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, create_body) = create_package_with_options(
        &app,
        &owner_jwt,
        "npm",
        "personal-lifecycle-package",
        "personal-pkgs",
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let package_id_str = create_body["id"]
        .as_str()
        .expect("package id should be returned")
        .to_owned();
    let package_id = uuid::Uuid::parse_str(&package_id_str).expect("package id should be a uuid");

    let (status, _) =
        delete_package_for_ecosystem(&app, &owner_jwt, "npm", "personal-lifecycle-package").await;
    assert_eq!(status, StatusCode::OK);

    for action in ["package_create", "package_delete"] {
        let target_org_id: Option<uuid::Uuid> = sqlx::query_scalar(
            "SELECT target_org_id FROM audit_logs \
             WHERE action = $1::audit_action AND target_package_id = $2",
        )
        .bind(action)
        .bind(package_id)
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|_| panic!("{action} audit row should exist"));
        assert_eq!(
            target_org_id, None,
            "{action} audit row for a personal package should leave target_org_id NULL"
        );
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn test_trusted_publisher_audit_sets_target_org_id_for_org_owned_packages(pool: PgPool) {
    let app = app(pool.clone());
    register_user(&app, "alice", "alice@test.dev", "super_secret_pw!").await;
    let owner_jwt = login_user(&app, "alice", "super_secret_pw!").await;

    let (status, org_body) = create_org(&app, &owner_jwt, "Acme Corp", "acme-corp").await;
    assert_eq!(status, StatusCode::CREATED);
    let org_id_str = org_body["id"]
        .as_str()
        .expect("org id should be returned")
        .to_owned();
    let org_id = uuid::Uuid::parse_str(&org_id_str).expect("org id should be a uuid");

    let (status, _) = create_repository_with_options(
        &app,
        &owner_jwt,
        "Acme Public",
        "acme-public",
        Some(&org_id_str),
        Some("public"),
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, create_body) = create_package_with_options(
        &app,
        &owner_jwt,
        "pypi",
        "acme-tp-widget",
        "acme-public",
        Some("public"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let package_id_str = create_body["id"]
        .as_str()
        .expect("package id should be returned")
        .to_owned();
    let package_id = uuid::Uuid::parse_str(&package_id_str).expect("package id should be a uuid");

    let (status, publisher_body) = create_trusted_publisher_for_package(
        &app,
        &owner_jwt,
        "pypi",
        "acme-tp-widget",
        json!({
            "issuer": "https://token.actions.githubusercontent.com",
            "subject": "repo:acme/tp-widget:ref:refs/heads/main",
            "repository": "acme/tp-widget",
            "workflow_ref": ".github/workflows/publish.yml@refs/heads/main",
            "environment": "production",
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected trusted publisher response: {publisher_body}"
    );
    let publisher_id = publisher_body["id"]
        .as_str()
        .expect("publisher id should be returned")
        .to_owned();

    let create_target_org_id: Option<uuid::Uuid> = sqlx::query_scalar(
        "SELECT target_org_id FROM audit_logs \
         WHERE action = 'trusted_publisher_create'::audit_action AND target_package_id = $1",
    )
    .bind(package_id)
    .fetch_one(&pool)
    .await
    .expect("trusted_publisher_create audit row should exist");
    assert_eq!(
        create_target_org_id,
        Some(org_id),
        "trusted_publisher_create audit row should carry the org id"
    );

    let (status, _) = delete_trusted_publisher_for_package(
        &app,
        &owner_jwt,
        "pypi",
        "acme-tp-widget",
        &publisher_id,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let delete_target_org_id: Option<uuid::Uuid> = sqlx::query_scalar(
        "SELECT target_org_id FROM audit_logs \
         WHERE action = 'trusted_publisher_delete'::audit_action AND target_package_id = $1",
    )
    .bind(package_id)
    .fetch_one(&pool)
    .await
    .expect("trusted_publisher_delete audit row should exist");
    assert_eq!(
        delete_target_org_id,
        Some(org_id),
        "trusted_publisher_delete audit row should carry the org id"
    );

    // Confirm org audit surfaces both events with the new target_org_id.
    let (status, audit_body) = list_org_audit(
        &app,
        &owner_jwt,
        "acme-corp",
        Some("action=trusted_publisher_create&per_page=20"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let logs = audit_body["logs"]
        .as_array()
        .expect("audit logs response should be an array");
    assert_eq!(logs.len(), 1, "response: {audit_body}");
    assert_eq!(logs[0]["action"], "trusted_publisher_create");
    assert_eq!(logs[0]["target_org_id"].as_str(), Some(org_id_str.as_str()));
}
