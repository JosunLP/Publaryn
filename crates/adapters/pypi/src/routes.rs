use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Path, State},
    http::{
        header::{ACCEPT, AUTHORIZATION, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE, LOCATION, WWW_AUTHENTICATE},
        HeaderMap, HeaderValue, StatusCode,
    },
    response::{IntoResponse, Response},
    routing::{get, post},
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
    domain::{
        artifact::{Artifact, ArtifactKind},
        namespace::Ecosystem,
        package::{normalize_package_name, Package},
        release::Release,
        repository::Visibility,
    },
    error::Error,
    validation,
};

use crate::{
    name::{canonicalize_project_name, is_canonical_project_name},
    simple::{
        build_index_json, build_project_json, render_index_html, render_project_html,
        select_response_format, ProjectFile, ProjectLink, ResponseFormat,
        PYPI_SIMPLE_JSON_CONTENT_TYPE,
    },
    upload::{LegacyPackageMetadata, LegacyUploadBuilder, LegacyUploadRequest},
};

const PYPI_SIMPLE_ROOT_PATH: &str = "/pypi/simple/";
const PYPI_SIMPLE_FILES_PATH: &str = "/pypi/files";
const PYPI_READABLE_RELEASE_STATUSES: &[&str] = &["published", "deprecated", "yanked"];
const PYPI_AUTH_REALM: &str = "Basic realm=\"Publaryn PyPI\"";
const PYPI_UPLOAD_ALLOWED_REPOSITORY_KINDS: &[&str] = &["public", "private", "staging", "release"];
const PYPI_UPLOAD_ALLOWED_USER_REPOSITORY_VISIBILITIES: &[&str] =
    &["public", "private", "unlisted", "quarantined"];
const ORG_REPOSITORY_WRITE_ROLES: &[&str] = &["owner", "admin"];
const PACKAGE_PUBLISH_ROLES: &[&str] = &["owner", "admin", "maintainer", "publisher"];
const TEAM_PACKAGE_PUBLISH_PERMISSIONS: &[&str] = &["admin", "publish"];

pub trait PyPiAppState: Clone + Send + Sync + 'static {
    fn db(&self) -> &PgPool;
    fn artifact_put(
        &self,
        key: String,
        content_type: String,
        bytes: Bytes,
    ) -> impl std::future::Future<Output = Result<(), Error>> + Send;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CredentialKind {
    Jwt,
    ApiToken,
}

#[derive(Debug, Clone)]
struct PyPiIdentity {
    user_id: Uuid,
    token_id: Option<Uuid>,
    scopes: Vec<String>,
    credential_kind: CredentialKind,
    package_scope_id: Option<Uuid>,
    oidc_derived: bool,
}

impl PyPiIdentity {
    fn audit_actor_token_id(&self) -> Option<Uuid> {
        match self.credential_kind {
            CredentialKind::Jwt => None,
            CredentialKind::ApiToken => self.token_id,
        }
    }
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

#[derive(Debug, Clone)]
struct UploadPackageContext {
    package_id: Uuid,
    repository_slug: String,
}

#[derive(Debug, Clone)]
struct UploadReleaseContext {
    release_id: Uuid,
    status: String,
    is_yanked: bool,
    is_deprecated: bool,
    was_created: bool,
}

#[derive(Debug, Clone)]
struct UploadedArtifactContext {
    artifact_id: Uuid,
    created: bool,
}

#[derive(Debug, Clone)]
struct ExistingUploadPackageContext {
    package_id: Uuid,
    repository_slug: String,
    owner_user_id: Option<Uuid>,
    owner_org_id: Option<Uuid>,
}

#[derive(Debug, Clone)]
struct UploadRepositoryTarget {
    repository_id: Uuid,
    repository_slug: String,
    repository_visibility: String,
    owner_user_id: Option<Uuid>,
    owner_org_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize)]
struct ErrorDocument<'a> {
    error: &'a str,
}

pub fn router<S: PyPiAppState>() -> Router<S> {
    Router::new()
        .route(
            "/legacy",
            post(upload_distribution::<S>).layer(DefaultBodyLimit::disable()),
        )
        .route(
            "/legacy/",
            post(upload_distribution::<S>).layer(DefaultBodyLimit::disable()),
        )
        .route(
            "/legacy/:repository_slug",
            post(upload_distribution_to_repository::<S>).layer(DefaultBodyLimit::disable()),
        )
        .route(
            "/legacy/:repository_slug/",
            post(upload_distribution_to_repository::<S>).layer(DefaultBodyLimit::disable()),
        )
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
    let identity = match authenticate_optional(&state, &headers, false).await {
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

    let identity = match authenticate_optional(&state, &headers, false).await {
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
    let identity = match authenticate_optional(&state, &headers, false).await {
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

async fn upload_distribution<S: PyPiAppState>(
    State(state): State<S>,
    headers: HeaderMap,
    multipart: Multipart,
) -> Response {
    upload_distribution_inner(state, None, headers, multipart).await
}

async fn upload_distribution_to_repository<S: PyPiAppState>(
    State(state): State<S>,
    Path(repository_slug): Path<String>,
    headers: HeaderMap,
    multipart: Multipart,
) -> Response {
    upload_distribution_inner(state, Some(repository_slug), headers, multipart).await
}

async fn upload_distribution_inner<S: PyPiAppState>(
    state: S,
    repository_slug: Option<String>,
    headers: HeaderMap,
    multipart: Multipart,
) -> Response {
    if let Some(repository_slug) = repository_slug.as_deref() {
        if validation::validate_slug(repository_slug).is_err() {
            return bad_request_response("The repository slug in the PyPI upload URL is invalid");
        }
    }

    let identity = match authenticate_required(&state, &headers).await {
        Ok(identity) => identity,
        Err(response) => return response,
    };

    if !identity_has_scope(&identity, "packages:write") {
        return forbidden_response("The supplied credential does not include the packages:write scope");
    }

    let upload = match parse_legacy_upload(multipart).await {
        Ok(upload) => upload,
        Err(response) => return response,
    };

    let package = match resolve_or_create_upload_package(
        &state,
        &identity,
        &upload,
        repository_slug.as_deref(),
    )
    .await
    {
        Ok(package) => package,
        Err(response) => return response,
    };

    let release = match resolve_or_create_upload_release(state.db(), package.package_id, &upload, identity.user_id).await {
        Ok(release) => release,
        Err(response) => return response,
    };

    let artifact = match upload_artifact_for_release(&state, release.release_id, &upload).await {
        Ok(artifact) => artifact,
        Err(response) => return response,
    };

    let final_status = match finalize_upload_release(state.db(), &release).await {
        Ok(status) => status,
        Err(response) => return response,
    };

    if artifact.created {
        if let Err(response) = record_upload_audit(
            state.db(),
            &identity,
            package.package_id,
            release.release_id,
            artifact.artifact_id,
            &package.repository_slug,
            &upload,
            &final_status,
            release.was_created,
        )
        .await
        {
            return response;
        }
    }

    let _ = touch_package_after_upload(state.db(), package.package_id).await;

    legacy_upload_success_response(&state, &upload.package_name)
}

async fn parse_legacy_upload(mut multipart: Multipart) -> Result<LegacyUploadRequest, Response> {
    let mut builder = LegacyUploadBuilder::default();

    loop {
        let field = multipart
            .next_field()
            .await
            .map_err(|_| bad_request_response("The multipart upload payload could not be parsed"))?;

        let Some(field) = field else {
            break;
        };

        let Some(name) = field.name().map(str::to_owned) else {
            return Err(bad_request_response(
                "Every multipart field must include a field name",
            ));
        };

        let file_name = field.file_name().map(str::to_owned);
        let content_type = field.content_type().map(|value| value.to_string());

        if file_name.is_some() {
            let bytes = field
                .bytes()
                .await
                .map_err(|_| bad_request_response("The uploaded distribution file could not be read"))?;
            builder
                .add_file_field(&name, file_name.as_deref(), content_type.as_deref(), bytes)
                .map_err(|message| bad_request_response(&message))?;
        } else {
            let text = field
                .text()
                .await
                .map_err(|_| bad_request_response("A multipart text field could not be decoded as UTF-8"))?;
            builder.add_text_field(&name, text);
        }
    }

    builder.build().map_err(|message| bad_request_response(&message))
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
    allow_oidc_derived: bool,
) -> Result<Option<PyPiIdentity>, Response> {
    let Some(header_value) = headers.get(AUTHORIZATION) else {
        return Ok(None);
    };

    let authorization = header_value
        .to_str()
        .map_err(|_| unauthorized_response("Invalid Authorization header"))?;

    if let Some(token) = authorization.strip_prefix("Bearer ") {
        return authenticate_bearer_token(state, token.trim(), allow_oidc_derived)
            .await
            .map(Some);
    }

    if let Some(token) = authorization.strip_prefix("Basic ") {
        return authenticate_basic_token(state, token.trim(), allow_oidc_derived)
            .await
            .map(Some);
    }

    Err(unauthorized_response(
        "Unsupported authorization scheme for PyPI access",
    ))
}

async fn authenticate_required<S: PyPiAppState>(
    state: &S,
    headers: &HeaderMap,
) -> Result<PyPiIdentity, Response> {
    authenticate_optional(state, headers, true)
        .await?
        .ok_or_else(|| unauthorized_response("Authentication is required for PyPI uploads"))
}

async fn authenticate_bearer_token<S: PyPiAppState>(
    state: &S,
    token: &str,
    allow_oidc_derived: bool,
) -> Result<PyPiIdentity, Response> {
    if token.is_empty() {
        return Err(unauthorized_response("Bearer token must not be empty"));
    }

    if token.starts_with("pub_") {
        return authenticate_api_token(state, token, allow_oidc_derived).await;
    }

    let claims = publaryn_auth::validate_token(token, state.jwt_secret(), state.jwt_issuer())
        .map_err(|_| unauthorized_response("Invalid or expired token"))?;

    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| unauthorized_response("Token subject is not a valid user identifier"))?;

    Ok(PyPiIdentity {
        user_id,
        token_id: None,
        scopes: claims.scopes,
        credential_kind: CredentialKind::Jwt,
        package_scope_id: None,
        oidc_derived: false,
    })
}

async fn authenticate_basic_token<S: PyPiAppState>(
    state: &S,
    encoded_credentials: &str,
    allow_oidc_derived: bool,
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

    authenticate_api_token(state, token, allow_oidc_derived).await
}

async fn authenticate_api_token<S: PyPiAppState>(
    state: &S,
    token: &str,
    allow_oidc_derived: bool,
) -> Result<PyPiIdentity, Response> {
    let token_hash = publaryn_core::security::hash_token(token);
    let row = sqlx::query(
        "SELECT id, user_id, scopes, expires_at, kind, package_id \
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

    let token_kind = string_column(&row, "kind");
    let package_scope_id = optional_uuid_column(&row, "package_id");
    let oidc_derived = token_kind == "oidc_derived";
    if oidc_derived && !allow_oidc_derived {
        return Err(unauthorized_response(
            "OIDC-derived tokens are only valid for PyPI uploads",
        ));
    }
    if oidc_derived && package_scope_id.is_none() {
        return Err(unauthorized_response(
            "OIDC-derived tokens must be scoped to a PyPI package",
        ));
    }

    let token_id = uuid_column(&row, "id");
    let user_id = optional_uuid_column(&row, "user_id")
        .ok_or_else(|| unauthorized_response("API token is not associated with a user account"))?;
    let scopes = row
        .try_get::<Vec<String>, _>("scopes")
        .unwrap_or_default();

    let _ = sqlx::query("UPDATE tokens SET last_used_at = NOW() WHERE id = $1")
        .bind(token_id)
        .execute(state.db())
        .await;

    Ok(PyPiIdentity {
        user_id,
        token_id: Some(token_id),
        scopes,
        credential_kind: CredentialKind::ApiToken,
        package_scope_id,
        oidc_derived,
    })
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

fn identity_has_scope(identity: &PyPiIdentity, scope: &str) -> bool {
    identity.scopes.iter().any(|candidate| candidate == scope)
}

fn identity_can_upload_existing_package(identity: &PyPiIdentity, package_id: Uuid) -> bool {
    identity.package_scope_id == Some(package_id)
}

async fn resolve_or_create_upload_package<S: PyPiAppState>(
    state: &S,
    identity: &PyPiIdentity,
    upload: &LegacyUploadRequest,
    requested_repository_slug: Option<&str>,
) -> Result<UploadPackageContext, Response> {
    let normalized_name = normalize_package_name(&upload.package_name, &Ecosystem::Pypi);
    let metadata = upload.package_metadata();

    if let Some(existing_package) = load_existing_upload_package(state.db(), &normalized_name).await? {
        let package_id = existing_package.package_id;
        if !identity_can_upload_existing_package(identity, package_id)
            && !actor_can_publish_package(
                state.db(),
                package_id,
                existing_package.owner_user_id,
                existing_package.owner_org_id,
                identity.user_id,
            )
            .await
        {
            return Err(forbidden_response(
                "You do not have permission to upload distributions for this package",
            ));
        }

        ensure_requested_repository_matches_existing_package(
            requested_repository_slug,
            &existing_package.repository_slug,
        )?;

        update_upload_package_metadata(state.db(), package_id, &metadata).await?;
        return Ok(UploadPackageContext {
            package_id,
            repository_slug: existing_package.repository_slug,
        });
    }

    if identity.oidc_derived {
        return Err(forbidden_response(
            "PyPI trusted publishing currently supports only existing packages with a configured trusted publisher",
        ));
    }

    let repository = match requested_repository_slug {
        Some(repository_slug) => {
            load_target_upload_repository(state.db(), repository_slug, identity.user_id).await?
        }
        None => load_default_upload_repository(state.db(), identity.user_id).await?,
    };

    let visibility = derive_upload_package_visibility(
        &repository.repository_visibility,
        repository.owner_org_id.is_some(),
    )?;
    let mut package = Package::new(
        repository.repository_id,
        Ecosystem::Pypi,
        upload.package_name.clone(),
        visibility.clone(),
    );
    package.description = metadata.description.clone();
    package.readme = metadata.readme.clone();
    package.homepage = metadata.homepage.clone();
    package.repository_url = metadata.repository_url.clone();
    package.license = metadata.license.clone();
    package.keywords = metadata.keywords.clone();
    package.owner_user_id = repository.owner_user_id;
    package.owner_org_id = repository.owner_org_id;

    let mut tx = state
        .db()
        .begin()
        .await
        .map_err(|_| internal_error_response("Database error"))?;

    let insert_result = sqlx::query(
        "INSERT INTO packages (id, repository_id, ecosystem, name, normalized_name, display_name, \
         description, readme, homepage, repository_url, license, keywords, visibility, \
         owner_user_id, owner_org_id, is_deprecated, deprecation_message, is_archived, \
         download_count, created_at, updated_at) \
         VALUES ($1, $2, 'pypi', $3, $4, NULL, $5, $6, $7, $8, $9, $10, $11, $12, $13, false, NULL, false, 0, $14, $15)",
    )
    .bind(package.id)
    .bind(package.repository_id)
    .bind(&package.name)
    .bind(&package.normalized_name)
    .bind(&package.description)
    .bind(&package.readme)
    .bind(&package.homepage)
    .bind(&package.repository_url)
    .bind(&package.license)
    .bind(&package.keywords)
    .bind(visibility_as_str(&visibility))
    .bind(package.owner_user_id)
    .bind(package.owner_org_id)
    .bind(package.created_at)
    .bind(package.updated_at)
    .execute(&mut *tx)
    .await;

    match insert_result {
        Ok(_) => {
            sqlx::query(
                "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_package_id, metadata, occurred_at) \
                 VALUES ($1, 'package_create', $2, $3, $4, $5, NOW())",
            )
            .bind(Uuid::new_v4())
            .bind(identity.user_id)
            .bind(identity.audit_actor_token_id())
            .bind(package.id)
            .bind(serde_json::json!({
                "ecosystem": "pypi",
                "name": package.name,
                "normalized_name": package.normalized_name,
                "repository_slug": repository.repository_slug,
                "source": "pypi_legacy_upload",
            }))
            .execute(&mut *tx)
            .await
            .map_err(|_| internal_error_response("Database error"))?;

            tx.commit()
                .await
                .map_err(|_| internal_error_response("Database error"))?;

            Ok(UploadPackageContext {
                package_id: package.id,
                repository_slug: repository.repository_slug,
            })
        }
        Err(sqlx::Error::Database(db_error)) if db_error.is_unique_violation() => {
            tx.rollback().await.ok();

            let existing_package = load_existing_upload_package(state.db(), &normalized_name)
                .await?
                .ok_or_else(|| {
                    internal_error_response(
                        "Package creation raced but the package could not be reloaded",
                    )
                })?;

            let package_id = existing_package.package_id;
            if !identity_can_upload_existing_package(identity, package_id)
                && !actor_can_publish_package(
                    state.db(),
                    package_id,
                    existing_package.owner_user_id,
                    existing_package.owner_org_id,
                    identity.user_id,
                )
                .await
            {
                return Err(forbidden_response(
                    "You do not have permission to upload distributions for this package",
                ));
            }

            ensure_requested_repository_matches_existing_package(
                requested_repository_slug,
                &existing_package.repository_slug,
            )?;

            update_upload_package_metadata(state.db(), package_id, &metadata).await?;
            Ok(UploadPackageContext {
                package_id,
                repository_slug: existing_package.repository_slug,
            })
        }
        Err(_) => Err(internal_error_response("Database error")),
    }
}

async fn load_existing_upload_package(
    db: &PgPool,
    normalized_name: &str,
) -> Result<Option<ExistingUploadPackageContext>, Response> {
    let row = sqlx::query(
        "SELECT p.id, p.owner_user_id, p.owner_org_id, r.slug AS repository_slug \
         FROM packages p \
         JOIN repositories r ON r.id = p.repository_id \
         WHERE p.ecosystem = 'pypi' AND p.normalized_name = $1",
    )
    .bind(normalized_name)
    .fetch_optional(db)
    .await
    .map_err(|_| internal_error_response("Database error"))?;

    Ok(row.map(|row| ExistingUploadPackageContext {
        package_id: uuid_column(&row, "id"),
        repository_slug: string_column(&row, "repository_slug"),
        owner_user_id: optional_uuid_column(&row, "owner_user_id"),
        owner_org_id: optional_uuid_column(&row, "owner_org_id"),
    }))
}

fn ensure_requested_repository_matches_existing_package(
    requested_repository_slug: Option<&str>,
    existing_repository_slug: &str,
) -> Result<(), Response> {
    let Some(requested_repository_slug) = requested_repository_slug else {
        return Ok(());
    };

    if requested_repository_slug == existing_repository_slug {
        return Ok(());
    }

    Err(conflict_response(&format!(
        "The PyPI package already belongs to repository '{existing_repository_slug}'",
    )))
}

async fn load_target_upload_repository(
    db: &PgPool,
    repository_slug: &str,
    actor_user_id: Uuid,
) -> Result<UploadRepositoryTarget, Response> {
    let repository = sqlx::query(
        "SELECT id, slug, kind, visibility, owner_user_id, owner_org_id \
         FROM repositories \
         WHERE slug = $1",
    )
    .bind(repository_slug)
    .fetch_optional(db)
    .await
    .map_err(|_| internal_error_response("Database error"))?
    .ok_or_else(|| not_found_response("The requested upload repository was not found"))?;

    if !repository_kind_allows_pypi_upload(&string_column(&repository, "kind")) {
        return Err(conflict_response(
            "PyPI packages can only be created in public, private, staging, or release repositories",
        ));
    }

    let owner_user_id = optional_uuid_column(&repository, "owner_user_id");
    let owner_org_id = optional_uuid_column(&repository, "owner_org_id");
    if !actor_can_write_repository(db, owner_user_id, owner_org_id, actor_user_id).await {
        return Err(forbidden_response(
            "You do not have permission to create PyPI packages in the selected repository",
        ));
    }

    Ok(UploadRepositoryTarget {
        repository_id: uuid_column(&repository, "id"),
        repository_slug: string_column(&repository, "slug"),
        repository_visibility: string_column(&repository, "visibility"),
        owner_user_id,
        owner_org_id,
    })
}

async fn load_default_upload_repository(
    db: &PgPool,
    actor_user_id: Uuid,
) -> Result<UploadRepositoryTarget, Response> {
    let repository = sqlx::query(
        "SELECT id, slug, visibility, owner_user_id, owner_org_id \
         FROM repositories \
         WHERE owner_user_id = $1 \
           AND kind::text = ANY($2) \
           AND visibility::text = ANY($3) \
         ORDER BY created_at ASC \
         LIMIT 1",
    )
    .bind(actor_user_id)
    .bind(&PYPI_UPLOAD_ALLOWED_REPOSITORY_KINDS
        .iter()
        .map(|kind| (*kind).to_owned())
        .collect::<Vec<_>>())
    .bind(&PYPI_UPLOAD_ALLOWED_USER_REPOSITORY_VISIBILITIES
        .iter()
        .map(|visibility| (*visibility).to_owned())
        .collect::<Vec<_>>())
    .fetch_optional(db)
    .await
    .map_err(|_| internal_error_response("Database error"))?
    .ok_or_else(|| {
        forbidden_response(
            "You have no user-owned repository suitable for PyPI uploads. Create one via the Publaryn API first.",
        )
    })?;

    Ok(UploadRepositoryTarget {
        repository_id: uuid_column(&repository, "id"),
        repository_slug: string_column(&repository, "slug"),
        repository_visibility: string_column(&repository, "visibility"),
        owner_user_id: optional_uuid_column(&repository, "owner_user_id"),
        owner_org_id: optional_uuid_column(&repository, "owner_org_id"),
    })
}

async fn update_upload_package_metadata(
    db: &PgPool,
    package_id: Uuid,
    metadata: &LegacyPackageMetadata,
) -> Result<(), Response> {
    sqlx::query(
        "UPDATE packages \
         SET description = COALESCE($1, description), \
             homepage = COALESCE($2, homepage), \
             repository_url = COALESCE($3, repository_url), \
             license = COALESCE($4, license), \
             keywords = COALESCE($5, keywords), \
             readme = COALESCE($6, readme), \
             updated_at = NOW() \
         WHERE id = $7",
    )
    .bind(metadata.description.clone())
    .bind(metadata.homepage.clone())
    .bind(metadata.repository_url.clone())
    .bind(metadata.license.clone())
    .bind(if metadata.keywords.is_empty() {
        None::<Vec<String>>
    } else {
        Some(metadata.keywords.clone())
    })
    .bind(metadata.readme.clone())
    .bind(package_id)
    .execute(db)
    .await
    .map_err(|_| internal_error_response("Database error"))?;

    Ok(())
}

async fn actor_can_publish_package(
    db: &PgPool,
    package_id: Uuid,
    owner_user_id: Option<Uuid>,
    owner_org_id: Option<Uuid>,
    actor_user_id: Uuid,
) -> bool {
    if owner_user_id == Some(actor_user_id) {
        return true;
    }

    if let Some(owner_org_id) = owner_org_id {
        let allowed_roles = PACKAGE_PUBLISH_ROLES
            .iter()
            .map(|role| (*role).to_owned())
            .collect::<Vec<_>>();

        let org_role_match = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (\
                 SELECT 1 \
                 FROM org_memberships \
                 WHERE org_id = $1 AND user_id = $2 AND role::text = ANY($3)\
             )",
        )
        .bind(owner_org_id)
        .bind(actor_user_id)
        .bind(&allowed_roles)
        .fetch_one(db)
        .await
        .unwrap_or(false);

        if org_role_match {
            return true;
        }

        let allowed_permissions = TEAM_PACKAGE_PUBLISH_PERMISSIONS
            .iter()
            .map(|permission| (*permission).to_owned())
            .collect::<Vec<_>>();

        return sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (\
                 SELECT 1 \
                 FROM team_package_access tpa \
                 JOIN team_memberships tm ON tm.team_id = tpa.team_id \
                 JOIN teams t ON t.id = tpa.team_id \
                 JOIN packages p ON p.id = tpa.package_id \
                 WHERE tpa.package_id = $1 \
                   AND tm.user_id = $2 \
                   AND t.org_id = p.owner_org_id \
                   AND tpa.permission::text = ANY($3)\
             )",
        )
        .bind(package_id)
        .bind(actor_user_id)
        .bind(&allowed_permissions)
        .fetch_one(db)
        .await
        .unwrap_or(false);
    }

    false
}

async fn actor_can_write_repository(
    db: &PgPool,
    owner_user_id: Option<Uuid>,
    owner_org_id: Option<Uuid>,
    actor_user_id: Uuid,
) -> bool {
    if owner_user_id == Some(actor_user_id) {
        return true;
    }

    let Some(owner_org_id) = owner_org_id else {
        return false;
    };

    let allowed_roles = ORG_REPOSITORY_WRITE_ROLES
        .iter()
        .map(|role| (*role).to_owned())
        .collect::<Vec<_>>();

    sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (\
             SELECT 1 \
             FROM org_memberships \
             WHERE org_id = $1 AND user_id = $2 AND role::text = ANY($3)\
         )",
    )
    .bind(owner_org_id)
    .bind(actor_user_id)
    .bind(&allowed_roles)
    .fetch_one(db)
    .await
    .unwrap_or(false)
}

fn repository_kind_allows_pypi_upload(kind: &str) -> bool {
    PYPI_UPLOAD_ALLOWED_REPOSITORY_KINDS.contains(&kind)
}

async fn resolve_or_create_upload_release(
    db: &PgPool,
    package_id: Uuid,
    upload: &LegacyUploadRequest,
    actor_user_id: Uuid,
) -> Result<UploadReleaseContext, Response> {
    if let Some(existing_release) = sqlx::query(
        "SELECT id, status, is_yanked, is_deprecated \
         FROM releases \
         WHERE package_id = $1 AND version = $2",
    )
    .bind(package_id)
    .bind(&upload.version)
    .fetch_optional(db)
    .await
    .map_err(|_| internal_error_response("Database error"))?
    {
        let status = string_column(&existing_release, "status");
        if status == "deleted" {
            return Err(conflict_response(
                "The requested PyPI version has been deleted and cannot accept new uploads",
            ));
        }

        return Ok(UploadReleaseContext {
            release_id: uuid_column(&existing_release, "id"),
            status,
            is_yanked: bool_column(&existing_release, "is_yanked"),
            is_deprecated: bool_column(&existing_release, "is_deprecated"),
            was_created: false,
        });
    }

    let release = Release::new(package_id, upload.version.clone(), actor_user_id);
    let insert_result = sqlx::query(
        "INSERT INTO releases (id, package_id, version, status, published_by, description, changelog, \
         is_prerelease, is_yanked, yank_reason, is_deprecated, deprecation_message, source_ref, provenance, \
         published_at, updated_at) \
         VALUES ($1, $2, $3, 'quarantine', $4, $5, NULL, $6, false, NULL, false, NULL, NULL, $7, $8, $9)",
    )
    .bind(release.id)
    .bind(package_id)
    .bind(&upload.version)
    .bind(actor_user_id)
    .bind(upload.release_description())
    .bind(upload.is_prerelease())
    .bind(upload.provenance_json())
    .bind(release.published_at)
    .bind(release.updated_at)
    .execute(db)
    .await;

    match insert_result {
        Ok(_) => Ok(UploadReleaseContext {
            release_id: release.id,
            status: "quarantine".into(),
            is_yanked: false,
            is_deprecated: false,
            was_created: true,
        }),
        Err(sqlx::Error::Database(db_error)) if db_error.is_unique_violation() => {
            let existing_release = sqlx::query(
                "SELECT id, status, is_yanked, is_deprecated \
                 FROM releases \
                 WHERE package_id = $1 AND version = $2",
            )
            .bind(package_id)
            .bind(&upload.version)
            .fetch_optional(db)
            .await
            .map_err(|_| internal_error_response("Database error"))?
            .ok_or_else(|| internal_error_response("Release creation raced but the release could not be reloaded"))?;

            let status = string_column(&existing_release, "status");
            if status == "deleted" {
                return Err(conflict_response(
                    "The requested PyPI version has been deleted and cannot accept new uploads",
                ));
            }

            Ok(UploadReleaseContext {
                release_id: uuid_column(&existing_release, "id"),
                status,
                is_yanked: bool_column(&existing_release, "is_yanked"),
                is_deprecated: bool_column(&existing_release, "is_deprecated"),
                was_created: false,
            })
        }
        Err(_) => Err(internal_error_response("Database error")),
    }
}

async fn upload_artifact_for_release<S: PyPiAppState>(
    state: &S,
    release_id: Uuid,
    upload: &LegacyUploadRequest,
) -> Result<UploadedArtifactContext, Response> {
    if let Some(existing_artifact) = sqlx::query(
        "SELECT id, sha256 \
         FROM artifacts \
         WHERE release_id = $1 AND filename = $2",
    )
    .bind(release_id)
    .bind(&upload.filename)
    .fetch_optional(state.db())
    .await
    .map_err(|_| internal_error_response("Database error"))?
    {
        if string_column(&existing_artifact, "sha256") != upload.digests.sha256_hex.as_str() {
            return Err(conflict_response(
                "A file with the same name already exists for this PyPI version with different content",
            ));
        }

        return Ok(UploadedArtifactContext {
            artifact_id: uuid_column(&existing_artifact, "id"),
            created: false,
        });
    }

    let storage_key = format!(
        "releases/{}/artifacts/{}/{}",
        release_id, upload.digests.sha256_hex, upload.filename
    );
    state
        .artifact_put(
            storage_key.clone(),
            upload.content_type.clone(),
            upload.bytes.clone(),
        )
        .await
        .map_err(|_| internal_error_response("Artifact storage error"))?;

    let mut artifact = Artifact::new(
        release_id,
        upload.artifact_kind.clone(),
        upload.filename.clone(),
        storage_key.clone(),
        upload.content_type.clone(),
        i64::try_from(upload.bytes.len())
            .map_err(|_| internal_error_response("The uploaded file is too large"))?,
        upload.digests.sha256_hex.clone(),
    );
    artifact.sha512 = Some(upload.digests.sha512_hex.clone());

    let insert_result = sqlx::query(
        "INSERT INTO artifacts (id, release_id, kind, filename, storage_key, content_type, size_bytes, sha256, sha512, md5, is_signed, signature_key_id, uploaded_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, false, NULL, $11) \
         ON CONFLICT (release_id, filename) DO NOTHING",
    )
    .bind(artifact.id)
    .bind(release_id)
    .bind(artifact.kind.clone())
    .bind(&artifact.filename)
    .bind(&artifact.storage_key)
    .bind(&artifact.content_type)
    .bind(artifact.size_bytes)
    .bind(&artifact.sha256)
    .bind(&artifact.sha512)
    .bind(Some(upload.digests.md5_hex.clone()))
    .bind(artifact.uploaded_at)
    .execute(state.db())
    .await
    .map_err(|_| internal_error_response("Database error"))?;

    if insert_result.rows_affected() == 0 {
        let existing_artifact = sqlx::query(
            "SELECT id, sha256 \
             FROM artifacts \
             WHERE release_id = $1 AND filename = $2",
        )
        .bind(release_id)
        .bind(&upload.filename)
        .fetch_optional(state.db())
        .await
        .map_err(|_| internal_error_response("Database error"))?
        .ok_or_else(|| internal_error_response("Artifact creation raced but the artifact could not be reloaded"))?;

        if string_column(&existing_artifact, "sha256") != upload.digests.sha256_hex.as_str() {
            return Err(conflict_response(
                "A file with the same name already exists for this PyPI version with different content",
            ));
        }

        return Ok(UploadedArtifactContext {
            artifact_id: uuid_column(&existing_artifact, "id"),
            created: false,
        });
    }

    Ok(UploadedArtifactContext {
        artifact_id: artifact.id,
        created: true,
    })
}

async fn finalize_upload_release(
    db: &PgPool,
    release: &UploadReleaseContext,
) -> Result<String, Response> {
    let desired_status = desired_upload_release_status(release);
    if release.status == desired_status {
        return Ok(desired_status.to_owned());
    }

    if !matches!(release.status.as_str(), "quarantine" | "scanning") {
        return Ok(release.status.clone());
    }

    sqlx::query(
        "UPDATE releases \
         SET status = $1, updated_at = NOW() \
         WHERE id = $2",
    )
    .bind(desired_status)
    .bind(release.release_id)
    .execute(db)
    .await
    .map_err(|_| internal_error_response("Database error"))?;

    Ok(desired_status.to_owned())
}

fn desired_upload_release_status(release: &UploadReleaseContext) -> &'static str {
    if release.is_yanked {
        "yanked"
    } else if release.is_deprecated {
        "deprecated"
    } else {
        "published"
    }
}

async fn record_upload_audit(
    db: &PgPool,
    identity: &PyPiIdentity,
    package_id: Uuid,
    release_id: Uuid,
    artifact_id: Uuid,
    repository_slug: &str,
    upload: &LegacyUploadRequest,
    final_status: &str,
    release_was_created: bool,
) -> Result<(), Response> {
    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_package_id, target_release_id, metadata, occurred_at) \
         VALUES ($1, 'release_publish', $2, $3, $4, $5, $6, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(package_id)
    .bind(release_id)
    .bind(serde_json::json!({
        "ecosystem": "pypi",
        "name": upload.package_name.as_str(),
        "version": upload.version.as_str(),
        "repository_slug": repository_slug,
        "filename": upload.filename.as_str(),
        "artifact_id": artifact_id,
        "artifact_kind": match &upload.artifact_kind {
            ArtifactKind::Wheel => "wheel",
            ArtifactKind::Sdist => "sdist",
            _ => "artifact",
        },
        "status": final_status,
        "release_was_created": release_was_created,
        "source": "pypi_legacy_upload",
        "credential_kind": if identity.oidc_derived {
            "oidc_derived_token"
        } else {
            match identity.credential_kind {
                CredentialKind::Jwt => "jwt",
                CredentialKind::ApiToken => "api_token",
            }
        },
        "comment": upload.comment.as_deref(),
        "sha256": upload.digests.sha256_hex.as_str(),
    }))
    .execute(db)
    .await
    .map_err(|_| internal_error_response("Database error"))?;

    Ok(())
}

async fn touch_package_after_upload(db: &PgPool, package_id: Uuid) -> Result<(), Response> {
    sqlx::query("UPDATE packages SET updated_at = NOW() WHERE id = $1")
        .bind(package_id)
        .execute(db)
        .await
        .map_err(|_| internal_error_response("Database error"))?;

    Ok(())
}

fn derive_upload_package_visibility(
    repository_visibility: &str,
    repository_is_org_owned: bool,
) -> Result<Visibility, Response> {
    match repository_visibility {
        "public" => Ok(Visibility::Public),
        "private" => Ok(Visibility::Private),
        "internal_org" if repository_is_org_owned => Ok(Visibility::InternalOrg),
        "internal_org" => Err(conflict_response(
            "Organization-internal PyPI packages require an organization-owned repository",
        )),
        "unlisted" => Ok(Visibility::Unlisted),
        "quarantined" => Ok(Visibility::Quarantined),
        _ => Err(conflict_response(
            "PyPI uploads cannot target a repository with an unsupported visibility",
        )),
    }
}

fn visibility_as_str(visibility: &Visibility) -> &'static str {
    match visibility {
        Visibility::Public => "public",
        Visibility::Private => "private",
        Visibility::InternalOrg => "internal_org",
        Visibility::Unlisted => "unlisted",
        Visibility::Quarantined => "quarantined",
    }
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

fn bad_request_response(message: &str) -> Response {
    text_response(StatusCode::BAD_REQUEST, "text/plain; charset=utf-8", message)
}

fn forbidden_response(message: &str) -> Response {
    text_response(StatusCode::FORBIDDEN, "text/plain; charset=utf-8", message)
}

fn conflict_response(message: &str) -> Response {
    text_response(StatusCode::CONFLICT, "text/plain; charset=utf-8", message)
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

fn legacy_upload_success_response<S: PyPiAppState>(state: &S, package_name: &str) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(
            LOCATION,
            format!(
                "{}{}/{}/",
                trimmed_base_url(state),
                "/pypi/simple".trim_end_matches('/'),
                canonicalize_project_name(package_name),
            ),
        )
        .header(CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Body::from("OK"))
        .unwrap_or_else(|_| internal_error_response("Failed to build upload response"))
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

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use super::{
        derive_upload_package_visibility,
        ensure_requested_repository_matches_existing_package,
    };
    use publaryn_core::domain::repository::Visibility;

    #[test]
    fn org_owned_internal_repositories_keep_internal_org_visibility() {
        let visibility = derive_upload_package_visibility("internal_org", true)
            .expect("org-owned internal repositories should be accepted");

        assert_eq!(visibility, Visibility::InternalOrg);
    }

    #[test]
    fn user_owned_internal_repositories_are_rejected_for_pypi_auto_create() {
        let response = derive_upload_package_visibility("internal_org", false)
            .expect_err("user-owned internal repositories should be rejected");

        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    #[test]
    fn explicit_repository_target_must_match_existing_package_repository() {
        let response = ensure_requested_repository_matches_existing_package(
            Some("team-releases"),
            "personal-releases",
        )
        .expect_err("mismatched repositories must conflict");

        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    #[test]
    fn explicit_repository_target_accepts_matching_existing_package_repository() {
        ensure_requested_repository_matches_existing_package(
            Some("team-releases"),
            "team-releases",
        )
        .expect("matching repositories should be accepted");

        ensure_requested_repository_matches_existing_package(None, "team-releases")
            .expect("default legacy route should still accept existing packages");
    }
}
