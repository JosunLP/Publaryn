use axum::{
    body::Body,
    extract::{Path, State},
    http::{
        header::{ACCEPT, AUTHORIZATION, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE, LOCATION, WWW_AUTHENTICATE},
        HeaderMap, HeaderValue, StatusCode,
    },
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use bytes::Bytes;
use chrono::Utc;
use serde::Serialize;
use sqlx::{PgPool, Row};
use std::collections::BTreeMap;
use uuid::Uuid;

use publaryn_core::{
    domain::{namespace::Ecosystem, package::normalize_package_name},
    error::Error,
};

use crate::{
    name::{canonicalize_project_name, is_canonical_project_name},
    simple::{
        build_index_json, build_project_json, render_index_html, render_project_html,
        select_response_format, ProjectFile, ProjectLink, ResponseFormat,
        PYPI_SIMPLE_JSON_CONTENT_TYPE,
    },
};

const PYPI_SIMPLE_ROOT_PATH: &str = "/pypi/simple/";
const PYPI_SIMPLE_FILES_PATH: &str = "/pypi/files";
const PYPI_READABLE_RELEASE_STATUSES: &[&str] = &["published", "deprecated", "yanked"];
const PYPI_AUTH_REALM: &str = "Basic realm=\"Publaryn PyPI\"";

pub trait PyPiAppState: Clone + Send + Sync + 'static {
    fn db(&self) -> &PgPool;
    fn artifact_get(
        &self,
        key: &str,
    ) -> impl std::future::Future<Output = Result<Option<StoredObject>, Error>> + Send;
    fn base_url(&self) -> &str;
    fn jwt_secret(&self) -> &str;
    fn jwt_issuer(&self) -> &str;
}

#[derive(Debug, Clone)]
pub struct StoredObject {
    pub content_type: String,
    pub bytes: Bytes,
}

#[derive(Debug, Clone)]
struct PyPiIdentity {
    user_id: Uuid,
}

#[derive(Debug, Clone)]
struct PackageAccessRow {
    package_id: Uuid,
    canonical_name: String,
    package_visibility: String,
    package_owner_user_id: Option<Uuid>,
    package_owner_org_id: Option<Uuid>,
    repository_visibility: String,
    repository_owner_user_id: Option<Uuid>,
    repository_owner_org_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize)]
struct ErrorDocument<'a> {
    error: &'a str,
}

pub fn router<S: PyPiAppState>() -> Router<S> {
    Router::new()
        .route("/simple", get(redirect_simple_root::<S>))
        .route("/simple/", get(simple_index::<S>))
        .route("/simple/:project", get(project_detail_without_trailing_slash::<S>))
        .route("/simple/:project/", get(project_detail::<S>))
        .route("/files/:artifact_id/:filename", get(download_distribution::<S>))
}

async fn redirect_simple_root<S: PyPiAppState>(State(state): State<S>) -> Response {
    redirect_response(&format!("{}{}", trimmed_base_url(&state), PYPI_SIMPLE_ROOT_PATH))
}

async fn project_detail_without_trailing_slash<S: PyPiAppState>(
    State(state): State<S>,
    Path(project): Path<String>,
) -> Response {
    redirect_project_response(&state, &project)
}

async fn simple_index<S: PyPiAppState>(State(state): State<S>, headers: HeaderMap) -> Response {
    let identity = match authenticate_optional(&state, &headers).await {
        Ok(identity) => identity,
        Err(response) => return response,
    };

    let selected_format = match select_response_format(header_value(&headers, ACCEPT)) {
        Ok(selected_format) => selected_format,
        Err(()) => return not_acceptable_response(),
    };

    let actor_user_id = identity.as_ref().map(|identity| identity.user_id);
    let projects = match load_visible_projects(state.db(), actor_user_id).await {
        Ok(projects) => projects,
        Err(response) => return response,
    };

    match selected_format.format {
        ResponseFormat::Json => json_response(
            StatusCode::OK,
            PYPI_SIMPLE_JSON_CONTENT_TYPE,
            build_index_json(&projects),
        ),
        ResponseFormat::Html => html_response(
            StatusCode::OK,
            selected_format.content_type,
            render_index_html(&projects),
        ),
    }
}

async fn project_detail<S: PyPiAppState>(
    State(state): State<S>,
    Path(project): Path<String>,
    headers: HeaderMap,
) -> Response {
    if !is_canonical_project_name(&project) {
        return redirect_project_response(&state, &project);
    }

    let identity = match authenticate_optional(&state, &headers).await {
        Ok(identity) => identity,
        Err(response) => return response,
    };

    let selected_format = match select_response_format(header_value(&headers, ACCEPT)) {
        Ok(selected_format) => selected_format,
        Err(()) => return not_acceptable_response(),
    };

    let package = match load_package_access_row(state.db(), &project).await {
        Ok(package) => package,
        Err(response) => return response,
    };

    let actor_user_id = identity.as_ref().map(|identity| identity.user_id);
    if !can_read_package(
        state.db(),
        package.package_id,
        &package.package_visibility,
        &package.repository_visibility,
        package.package_owner_user_id,
        package.package_owner_org_id,
        package.repository_owner_user_id,
        package.repository_owner_org_id,
        actor_user_id,
    )
    .await
    {
        return not_found_response("Project not found");
    }

    let (versions, files) = match load_project_files(&state, &package).await {
        Ok(project_files) => project_files,
        Err(response) => return response,
    };

    match selected_format.format {
        ResponseFormat::Json => json_response(
            StatusCode::OK,
            PYPI_SIMPLE_JSON_CONTENT_TYPE,
            build_project_json(&package.canonical_name, &versions, &files),
        ),
        ResponseFormat::Html => html_response(
            StatusCode::OK,
            selected_format.content_type,
            render_project_html(&package.canonical_name, &files),
        ),
    }
}

async fn download_distribution<S: PyPiAppState>(
    State(state): State<S>,
    Path((artifact_id, filename)): Path<(Uuid, String)>,
    headers: HeaderMap,
) -> Response {
    let identity = match authenticate_optional(&state, &headers).await {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let actor_user_id = identity.as_ref().map(|identity| identity.user_id);

    let artifact_row = match sqlx::query(
        "SELECT a.storage_key, a.content_type, a.sha256, p.id AS package_id, p.visibility AS package_visibility, \
                p.owner_user_id AS package_owner_user_id, p.owner_org_id AS package_owner_org_id, \
                r.visibility AS repository_visibility, r.owner_user_id AS repository_owner_user_id, \
                r.owner_org_id AS repository_owner_org_id \
         FROM artifacts a \
         JOIN releases rel ON rel.id = a.release_id \
         JOIN packages p ON p.id = rel.package_id \
         JOIN repositories r ON r.id = p.repository_id \
         WHERE a.id = $1 \
           AND a.filename = $2 \
           AND a.kind IN ('wheel', 'sdist') \
           AND rel.status::text = ANY($3)",
    )
    .bind(artifact_id)
    .bind(&filename)
    .bind(&readable_release_statuses())
    .fetch_optional(state.db())
    .await
    {
        Ok(Some(row)) => row,
        Ok(None) => return not_found_response("Distribution file not found"),
        Err(_) => return internal_error_response("Database error"),
    };

    if !can_read_package(
        state.db(),
        uuid_column(&artifact_row, "package_id"),
        &string_column(&artifact_row, "package_visibility"),
        &string_column(&artifact_row, "repository_visibility"),
        optional_uuid_column(&artifact_row, "package_owner_user_id"),
        optional_uuid_column(&artifact_row, "package_owner_org_id"),
        optional_uuid_column(&artifact_row, "repository_owner_user_id"),
        optional_uuid_column(&artifact_row, "repository_owner_org_id"),
        actor_user_id,
    )
    .await
    {
        return not_found_response("Distribution file not found");
    }

    let storage_key = string_column(&artifact_row, "storage_key");
    let stored_object = match state.artifact_get(&storage_key).await {
        Ok(Some(stored_object)) => stored_object,
        Ok(None) => return not_found_response("Distribution file not found"),
        Err(_) => return internal_error_response("Artifact storage error"),
    };

    let package_id = uuid_column(&artifact_row, "package_id");
    let _ = sqlx::query("UPDATE packages SET download_count = download_count + 1 WHERE id = $1")
        .bind(package_id)
        .execute(state.db())
        .await;

    let disposition = format!(
        "attachment; filename=\"{}\"",
        filename.replace('"', "")
    );
    let sha256 = string_column(&artifact_row, "sha256");
    let content_type = if stored_object.content_type.is_empty() {
        string_column(&artifact_row, "content_type")
    } else {
        stored_object.content_type
    };

    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, content_type)
        .header(CONTENT_LENGTH, stored_object.bytes.len().to_string())
        .header(CONTENT_DISPOSITION, disposition)
        .header("x-checksum-sha256", sha256)
        .body(Body::from(stored_object.bytes))
        .unwrap_or_else(|_| internal_error_response("Failed to build file response"))
}

async fn load_visible_projects(
    db: &PgPool,
    actor_user_id: Option<Uuid>,
) -> Result<Vec<ProjectLink>, Response> {
    let rows = sqlx::query(
        "SELECT DISTINCT p.name \
         FROM packages p \
         JOIN repositories r ON r.id = p.repository_id \
         WHERE p.ecosystem = 'pypi' \
           AND p.visibility <> 'unlisted' \
           AND p.visibility <> 'quarantined' \
           AND r.visibility <> 'unlisted' \
           AND r.visibility <> 'quarantined' \
           AND (\
                p.visibility = 'public' \
                OR (\
                    $1::uuid IS NOT NULL \
                    AND (\
                        p.owner_user_id = $1 \
                        OR EXISTS (\
                            SELECT 1 \
                            FROM org_memberships om_pkg \
                            WHERE om_pkg.user_id = $1 AND om_pkg.org_id = p.owner_org_id\
                        ) \
                        OR EXISTS (\
                            SELECT 1 \
                            FROM team_package_access tpa \
                            JOIN team_memberships tm ON tm.team_id = tpa.team_id \
                            JOIN teams t ON t.id = tpa.team_id \
                            WHERE tpa.package_id = p.id \
                              AND tm.user_id = $1 \
                              AND t.org_id = p.owner_org_id\
                        )\
                    )\
                )\
           ) \
           AND (\
                r.visibility = 'public' \
                OR (\
                    $1::uuid IS NOT NULL \
                    AND (\
                        r.owner_user_id = $1 \
                        OR EXISTS (\
                            SELECT 1 \
                            FROM org_memberships om_repo \
                            WHERE om_repo.user_id = $1 AND om_repo.org_id = r.owner_org_id\
                        )\
                    )\
                )\
           ) \
         ORDER BY LOWER(p.name) ASC",
    )
    .bind(actor_user_id)
    .fetch_all(db)
    .await
    .map_err(|_| internal_error_response("Database error"))?;

    Ok(rows
        .into_iter()
        .filter_map(|row| row.try_get::<String, _>("name").ok())
        .map(|name| ProjectLink {
            normalized_name: canonicalize_project_name(&name),
            name,
        })
        .collect())
}

async fn load_package_access_row(
    db: &PgPool,
    canonical_name: &str,
) -> Result<PackageAccessRow, Response> {
    let normalized_name = normalize_package_name(canonical_name, &Ecosystem::Pypi);
    let row = sqlx::query(
        "SELECT p.id, p.name, p.normalized_name, p.visibility, p.owner_user_id, p.owner_org_id, \
                r.visibility AS repository_visibility, r.owner_user_id AS repository_owner_user_id, \
                r.owner_org_id AS repository_owner_org_id \
         FROM packages p \
         JOIN repositories r ON r.id = p.repository_id \
         WHERE p.ecosystem = 'pypi' AND p.normalized_name = $1",
    )
    .bind(&normalized_name)
    .fetch_optional(db)
    .await
    .map_err(|_| internal_error_response("Database error"))?
    .ok_or_else(|| not_found_response("Project not found"))?;

    Ok(PackageAccessRow {
        package_id: uuid_column(&row, "id"),
        canonical_name: canonicalize_project_name(&string_column(&row, "name")),
        package_visibility: string_column(&row, "visibility"),
        package_owner_user_id: optional_uuid_column(&row, "owner_user_id"),
        package_owner_org_id: optional_uuid_column(&row, "owner_org_id"),
        repository_visibility: string_column(&row, "repository_visibility"),
        repository_owner_user_id: optional_uuid_column(&row, "repository_owner_user_id"),
        repository_owner_org_id: optional_uuid_column(&row, "repository_owner_org_id"),
    })
}

async fn load_project_files<S: PyPiAppState>(
    state: &S,
    package: &PackageAccessRow,
) -> Result<(Vec<String>, Vec<ProjectFile>), Response> {
    let rows = sqlx::query(
        "SELECT rel.version, rel.is_yanked, rel.yank_reason, a.id AS artifact_id, a.filename, a.sha256, \
                a.sha512, a.size_bytes, a.uploaded_at \
         FROM releases rel \
         JOIN artifacts a ON a.release_id = rel.id \
         WHERE rel.package_id = $1 \
           AND rel.status::text = ANY($2) \
           AND a.kind IN ('wheel', 'sdist') \
         ORDER BY rel.published_at DESC, a.filename ASC",
    )
    .bind(package.package_id)
    .bind(&readable_release_statuses())
    .fetch_all(state.db())
    .await
    .map_err(|_| internal_error_response("Database error"))?;

    let mut versions = Vec::new();
    let mut seen_versions = std::collections::BTreeSet::new();
    let mut files = Vec::with_capacity(rows.len());

    for row in rows {
        let version = string_column(&row, "version");
        if seen_versions.insert(version.clone()) {
            versions.push(version);
        }

        let artifact_id = uuid_column(&row, "artifact_id");
        let filename = string_column(&row, "filename");
        let file_url = format!(
            "{}{}/{}/{}",
            trimmed_base_url(state),
            PYPI_SIMPLE_FILES_PATH,
            artifact_id,
            encode_path_segment(&filename),
        );

        let mut hashes = BTreeMap::new();
        let sha256 = string_column(&row, "sha256");
        if !sha256.is_empty() {
            hashes.insert("sha256".into(), sha256);
        }
        if let Some(sha512) = optional_string_column(&row, "sha512") {
            hashes.insert("sha512".into(), sha512);
        }

        let yanked_reason = optional_string_column(&row, "yank_reason");
        files.push(ProjectFile {
            filename,
            url: file_url,
            hashes,
            size_bytes: i64_column(&row, "size_bytes"),
            upload_time: optional_datetime_column(&row, "uploaded_at"),
            is_yanked: bool_column(&row, "is_yanked"),
            yanked_reason,
        });
    }

    Ok((versions, files))
}

async fn authenticate_optional<S: PyPiAppState>(
    state: &S,
    headers: &HeaderMap,
) -> Result<Option<PyPiIdentity>, Response> {
    let Some(header_value) = headers.get(AUTHORIZATION) else {
        return Ok(None);
    };

    let authorization = header_value
        .to_str()
        .map_err(|_| unauthorized_response("Invalid Authorization header"))?;

    if let Some(token) = authorization.strip_prefix("Bearer ") {
        return authenticate_bearer_token(state, token.trim()).await.map(Some);
    }

    if let Some(token) = authorization.strip_prefix("Basic ") {
        return authenticate_basic_token(state, token.trim()).await.map(Some);
    }

    Err(unauthorized_response(
        "Unsupported authorization scheme for PyPI access",
    ))
}

async fn authenticate_bearer_token<S: PyPiAppState>(
    state: &S,
    token: &str,
) -> Result<PyPiIdentity, Response> {
    if token.is_empty() {
        return Err(unauthorized_response("Bearer token must not be empty"));
    }

    if token.starts_with("pub_") {
        return authenticate_api_token(state, token).await;
    }

    let claims = publaryn_auth::validate_token(token, state.jwt_secret(), state.jwt_issuer())
        .map_err(|_| unauthorized_response("Invalid or expired token"))?;

    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| unauthorized_response("Token subject is not a valid user identifier"))?;

    Ok(PyPiIdentity { user_id })
}

async fn authenticate_basic_token<S: PyPiAppState>(
    state: &S,
    encoded_credentials: &str,
) -> Result<PyPiIdentity, Response> {
    let decoded = BASE64_STANDARD
        .decode(encoded_credentials)
        .map_err(|_| unauthorized_response("Basic credentials are not valid base64"))?;
    let decoded = String::from_utf8(decoded)
        .map_err(|_| unauthorized_response("Basic credentials are not valid UTF-8"))?;
    let (username, password) = decoded.split_once(':').unwrap_or((decoded.as_str(), ""));

    let token = if password.starts_with("pub_") {
        password
    } else if username.starts_with("pub_") {
        username
    } else {
        return Err(unauthorized_response(
            "Basic authentication must provide a Publaryn API token",
        ));
    };

    authenticate_api_token(state, token).await
}

async fn authenticate_api_token<S: PyPiAppState>(
    state: &S,
    token: &str,
) -> Result<PyPiIdentity, Response> {
    let token_hash = publaryn_core::security::hash_token(token);
    let row = sqlx::query(
        "SELECT id, user_id, expires_at \
         FROM tokens \
         WHERE token_hash = $1 AND is_revoked = false",
    )
    .bind(&token_hash)
    .fetch_optional(state.db())
    .await
    .map_err(|_| internal_error_response("Database error"))?
    .ok_or_else(|| unauthorized_response("Invalid or revoked token"))?;

    let expires_at = optional_datetime_column(&row, "expires_at");
    if expires_at.is_some_and(|expires_at| expires_at <= Utc::now()) {
        return Err(unauthorized_response("Token has expired"));
    }

    let token_id = uuid_column(&row, "id");
    let user_id = optional_uuid_column(&row, "user_id")
        .ok_or_else(|| unauthorized_response("API token is not associated with a user account"))?;

    let _ = sqlx::query("UPDATE tokens SET last_used_at = NOW() WHERE id = $1")
        .bind(token_id)
        .execute(state.db())
        .await;

    Ok(PyPiIdentity { user_id })
}

async fn can_read_package(
    db: &PgPool,
    package_id: Uuid,
    package_visibility: &str,
    repository_visibility: &str,
    package_owner_user_id: Option<Uuid>,
    package_owner_org_id: Option<Uuid>,
    repository_owner_user_id: Option<Uuid>,
    repository_owner_org_id: Option<Uuid>,
    actor_user_id: Option<Uuid>,
) -> bool {
    let package_access = can_read_owned_resource(
        db,
        package_visibility,
        package_owner_user_id,
        package_owner_org_id,
        actor_user_id,
    )
    .await;
    let team_package_access = match actor_user_id {
        Some(actor_user_id) if !package_access => actor_has_any_team_package_access(
            db,
            package_id,
            actor_user_id,
        )
        .await,
        _ => false,
    };
    let repository_access = can_read_owned_resource(
        db,
        repository_visibility,
        repository_owner_user_id,
        repository_owner_org_id,
        actor_user_id,
    )
    .await;

    (package_access || team_package_access) && repository_access
}

async fn can_read_owned_resource(
    db: &PgPool,
    visibility: &str,
    owner_user_id: Option<Uuid>,
    owner_org_id: Option<Uuid>,
    actor_user_id: Option<Uuid>,
) -> bool {
    if visibility_allows_anonymous_read(visibility) {
        return true;
    }

    let Some(actor_user_id) = actor_user_id else {
        return false;
    };

    if owner_user_id == Some(actor_user_id) {
        return true;
    }

    if let Some(owner_org_id) = owner_org_id {
        return actor_is_org_member(db, owner_org_id, actor_user_id).await;
    }

    false
}

fn visibility_allows_anonymous_read(visibility: &str) -> bool {
    matches!(visibility, "public" | "unlisted")
}

async fn actor_is_org_member(db: &PgPool, org_id: Uuid, actor_user_id: Uuid) -> bool {
    sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (\
             SELECT 1 \
             FROM org_memberships \
             WHERE org_id = $1 AND user_id = $2\
         )",
    )
    .bind(org_id)
    .bind(actor_user_id)
    .fetch_one(db)
    .await
    .unwrap_or(false)
}

async fn actor_has_any_team_package_access(db: &PgPool, package_id: Uuid, actor_user_id: Uuid) -> bool {
    sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (\
             SELECT 1 \
             FROM team_package_access tpa \
             JOIN team_memberships tm ON tm.team_id = tpa.team_id \
             JOIN teams t ON t.id = tpa.team_id \
             JOIN packages p ON p.id = tpa.package_id \
             WHERE tpa.package_id = $1 \
               AND tm.user_id = $2 \
               AND t.org_id = p.owner_org_id\
         )",
    )
    .bind(package_id)
    .bind(actor_user_id)
    .fetch_one(db)
    .await
    .unwrap_or(false)
}

fn header_value<'a>(headers: &'a HeaderMap, name: axum::http::header::HeaderName) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

fn readable_release_statuses() -> Vec<String> {
    PYPI_READABLE_RELEASE_STATUSES
        .iter()
        .map(|status| (*status).to_owned())
        .collect()
}

fn redirect_project_response<S: PyPiAppState>(state: &S, project: &str) -> Response {
    let canonical_project = canonicalize_project_name(project);
    redirect_response(&format!(
        "{}{}/{}/",
        trimmed_base_url(state),
        "/pypi/simple".trim_end_matches('/'),
        canonical_project,
    ))
}

fn redirect_response(location: &str) -> Response {
    Response::builder()
        .status(StatusCode::PERMANENT_REDIRECT)
        .header(LOCATION, location)
        .body(Body::empty())
        .unwrap_or_else(|_| internal_error_response("Failed to build redirect response"))
}

fn not_acceptable_response() -> Response {
    text_response(
        StatusCode::NOT_ACCEPTABLE,
        "text/plain; charset=utf-8",
        "The requested response format is not supported. Use HTML or application/vnd.pypi.simple.v1+json.",
    )
}

fn unauthorized_response(message: &str) -> Response {
    Response::builder()
        .status(StatusCode::UNAUTHORIZED)
        .header(WWW_AUTHENTICATE, PYPI_AUTH_REALM)
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&ErrorDocument { error: message }).unwrap_or_default(),
        ))
        .unwrap_or_else(|_| internal_error_response("Failed to build unauthorized response"))
}

fn not_found_response(message: &str) -> Response {
    json_response(
        StatusCode::NOT_FOUND,
        "application/json",
        serde_json::json!({ "error": message }),
    )
}

fn internal_error_response(message: &str) -> Response {
    json_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        "application/json",
        serde_json::json!({ "error": message }),
    )
}

fn json_response(status: StatusCode, content_type: &str, value: serde_json::Value) -> Response {
    let mut response = (status, Json(value)).into_response();
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_str(content_type).unwrap_or(HeaderValue::from_static("application/json")),
    );
    response
        .headers_mut()
        .insert(axum::http::header::VARY, HeaderValue::from_static("Accept"));
    response
}

fn html_response(status: StatusCode, content_type: &str, body: String) -> Response {
    let mut response = Response::builder()
        .status(status)
        .header(CONTENT_TYPE, content_type)
        .body(Body::from(body))
        .unwrap_or_else(|_| internal_error_response("Failed to build HTML response"));
    response
        .headers_mut()
        .insert(axum::http::header::VARY, HeaderValue::from_static("Accept"));
    response
}

fn text_response(status: StatusCode, content_type: &str, body: &str) -> Response {
    Response::builder()
        .status(status)
        .header(CONTENT_TYPE, content_type)
        .body(Body::from(body.to_owned()))
        .unwrap_or_else(|_| internal_error_response("Failed to build text response"))
}

fn trimmed_base_url<S: PyPiAppState>(state: &S) -> &str {
    state.base_url().trim_end_matches('/')
}

fn encode_path_segment(input: &str) -> String {
    let mut encoded = String::with_capacity(input.len());
    for byte in input.bytes() {
        let is_unreserved = matches!(byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~');
        if is_unreserved {
            encoded.push(byte as char);
        } else {
            encoded.push('%');
            encoded.push_str(&format!("{byte:02X}"));
        }
    }
    encoded
}

fn string_column(row: &sqlx::postgres::PgRow, column: &str) -> String {
    row.try_get(column).unwrap_or_default()
}

fn optional_string_column(row: &sqlx::postgres::PgRow, column: &str) -> Option<String> {
    row.try_get(column).ok().flatten()
}

fn uuid_column(row: &sqlx::postgres::PgRow, column: &str) -> Uuid {
    row.try_get(column).unwrap_or_else(|_| Uuid::nil())
}

fn optional_uuid_column(row: &sqlx::postgres::PgRow, column: &str) -> Option<Uuid> {
    row.try_get(column).ok().flatten()
}

fn bool_column(row: &sqlx::postgres::PgRow, column: &str) -> bool {
    row.try_get(column).unwrap_or(false)
}

fn i64_column(row: &sqlx::postgres::PgRow, column: &str) -> i64 {
    row.try_get(column).unwrap_or_default()
}

fn optional_datetime_column(
    row: &sqlx::postgres::PgRow,
    column: &str,
) -> Option<chrono::DateTime<chrono::Utc>> {
    row.try_get(column).ok().flatten()
}
