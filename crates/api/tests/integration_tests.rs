use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
};
use base64::engine::{general_purpose::STANDARD as BASE64, Engine};
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use std::net::ToSocketAddrs;
use tokio::time::Duration;
use tower::ServiceExt;
use url::Url;

use publaryn_api::{config::Config, router::build_router, state::AppState};

// ── Helpers ──────────────────────────────────────────────────────────────────

const TEST_RESPONSE_BODY_LIMIT: usize = 8 * 1024 * 1024;

/// Build an Axum app backed by the given DB pool.
fn app(pool: PgPool) -> axum::Router {
    // When constructing state with `new_with_pool`, the provided pool is used for
    // database access and `config.database.url` is not used to establish a
    // connection in this test helper. Keep the fallback as an explicit
    // placeholder to avoid accidental coupling to a real database.
    let database_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "unused://database-url".into());
    let config = Config::test_config(&database_url);
    let state = AppState::new_with_pool(pool, config);
    build_router(state).expect("router should build")
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

/// List security findings aggregated for an organization and return the response.
async fn list_org_security_findings(
    app: &axum::Router,
    jwt: Option<&str>,
    org_slug: &str,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(Method::GET)
        .uri(format!("/v1/orgs/{org_slug}/security-findings"));

    if let Some(jwt) = jwt {
        request = request.header(header::AUTHORIZATION, format!("Bearer {jwt}"));
    }

    let req = request.body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
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
    let mut request = Request::builder()
        .method(Method::GET)
        .uri(format!("/v1/packages/{ecosystem}/{name}"));

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
    let uri = match ecosystem {
        Some(ecosystem) => format!("/v1/search?q={query}&ecosystem={ecosystem}"),
        None => format!("/v1/search?q={query}"),
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
        .uri(format!("/v1/packages/{ecosystem}/{name}/releases"))
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
        "/v1/packages/{ecosystem}/{name}/releases/{version}"
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
        "/v1/packages/{ecosystem}/{name}/releases/{version}/artifacts"
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
            "/v1/packages/{ecosystem}/{name}/releases/{version}/artifacts/{filename}?kind={kind}"
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
            "/v1/packages/{ecosystem}/{name}/releases/{version}/publish"
        ))
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
        bob_memberships[0]["role"], "maintainer",
        "members response: {body}"
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
async fn test_team_repository_write_metadata_permission_allows_package_creation_and_metadata_updates_but_not_repository_settings(
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
    assert_eq!(publish_body["status"], "published");
    assert_eq!(publish_body["artifact_count"], 1);

    let (status, anonymous_published_release) =
        get_release_detail(&app, None, "npm", "release-ui-widget", "1.2.3").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(anonymous_published_release["status"], "published");
    assert_eq!(anonymous_published_release["can_manage_releases"], false);
    assert_eq!(
        anonymous_published_release["description"],
        "First managed release"
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
