use axum::{
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{
        header::{CONTENT_LENGTH, CONTENT_TYPE, LINK, LOCATION},
        HeaderMap, HeaderName, StatusCode,
    },
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use bytes::Bytes as BytesAlias;
use chrono::Utc;
use serde::Deserialize;
use sha2::Digest;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use publaryn_core::{
    domain::{
        artifact::{Artifact, ArtifactKind},
        namespace::Ecosystem,
        package::normalize_package_name,
        release::Release,
    },
    error::Error,
};

use crate::{
    auth::{self, AuthFailure, OciIdentity},
    manifest::{self, ManifestReference, ManifestReferenceKind},
    name::{self, OciReference},
    upload::{self, UploadSessionRecord},
};

const ORG_PACKAGE_WRITE_ROLES: &[&str] = &["owner", "admin", "maintainer", "publisher"];
const ORG_PACKAGE_ADMIN_ROLES: &[&str] = &["owner", "admin"];
const TEAM_PACKAGE_PUBLISH_PERMISSIONS: &[&str] = &["admin", "publish"];
const TEAM_PACKAGE_ADMIN_PERMISSIONS: &[&str] = &["admin"];
const TEAM_REPOSITORY_PUBLISH_PERMISSIONS: &[&str] = &["admin", "publish"];
const TEAM_REPOSITORY_ADMIN_PERMISSIONS: &[&str] = &["admin"];
const OCI_IMAGE_INDEX_MEDIA_TYPE: &str = "application/vnd.oci.image.index.v1+json";
const OCI_IMAGE_MANIFEST_MEDIA_TYPE: &str = "application/vnd.oci.image.manifest.v1+json";

pub trait OciAppState: Clone + Send + Sync + 'static {
    fn db(&self) -> &PgPool;
    fn artifact_put(
        &self,
        key: String,
        content_type: String,
        bytes: BytesAlias,
    ) -> impl std::future::Future<Output = Result<(), Error>> + Send;
    fn artifact_get(
        &self,
        key: &str,
    ) -> impl std::future::Future<Output = Result<Option<StoredObject>, Error>> + Send;
    fn artifact_delete(
        &self,
        key: &str,
    ) -> impl std::future::Future<Output = Result<(), Error>> + Send;
    fn base_url(&self) -> &str;
    fn jwt_secret(&self) -> &str;
    fn jwt_issuer(&self) -> &str;
}

#[derive(Debug, Clone)]
pub struct StoredObject {
    pub content_type: String,
    pub bytes: BytesAlias,
}

#[derive(Debug, Clone)]
struct PackageContext {
    package_id: Uuid,
    repository_id: Uuid,
    package_name: String,
    package_visibility: String,
    package_owner_user_id: Option<Uuid>,
    package_owner_org_id: Option<Uuid>,
    repository_visibility: String,
    repository_owner_user_id: Option<Uuid>,
    repository_owner_org_id: Option<Uuid>,
}

#[derive(Debug, Clone)]
struct ManifestArtifactContext {
    release_id: Uuid,
    storage_key: String,
    content_type: String,
    size_bytes: i64,
    digest: String,
}

#[derive(Debug, Default, Deserialize)]
struct ListQuery {
    n: Option<usize>,
    last: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct UploadQuery {
    digest: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ReferrersQuery {
    n: Option<usize>,
    last: Option<String>,
    #[serde(rename = "artifactType")]
    artifact_type: Option<String>,
}

#[derive(Debug, Clone)]
struct ReferrerDescriptor {
    media_type: String,
    digest: String,
    size_bytes: i64,
    artifact_type: Option<String>,
    annotations: Option<serde_json::Map<String, serde_json::Value>>,
}

impl ListQuery {
    fn limit(&self) -> usize {
        self.n.unwrap_or(100).clamp(1, 1000)
    }
}

impl ReferrersQuery {
    fn limit(&self) -> Option<usize> {
        self.n.map(|limit| limit.min(1000))
    }
}

impl From<ReferrersQuery> for ListQuery {
    fn from(value: ReferrersQuery) -> Self {
        Self {
            n: value.n,
            last: value.last,
        }
    }
}

impl ReferrerDescriptor {
    fn into_json(self) -> serde_json::Value {
        let mut descriptor = serde_json::Map::new();
        descriptor.insert(
            "mediaType".into(),
            serde_json::Value::String(self.media_type),
        );
        descriptor.insert("size".into(), serde_json::json!(self.size_bytes));
        descriptor.insert("digest".into(), serde_json::Value::String(self.digest));
        if let Some(artifact_type) = self.artifact_type {
            descriptor.insert(
                "artifactType".into(),
                serde_json::Value::String(artifact_type),
            );
        }
        if let Some(annotations) = self
            .annotations
            .filter(|annotations| !annotations.is_empty())
        {
            descriptor.insert("annotations".into(), serde_json::Value::Object(annotations));
        }

        serde_json::Value::Object(descriptor)
    }
}

pub fn router<S: OciAppState>() -> Router<S> {
    Router::new()
        .route("/v2/", get(api_probe::<S>))
        .route("/v2/_catalog", get(catalog::<S>))
        .route(
            "/v2/{*path}",
            get(get_dispatch::<S>)
                .head(head_dispatch::<S>)
                .put(put_dispatch::<S>)
                .post(post_dispatch::<S>)
                .patch(patch_dispatch::<S>)
                .delete(delete_dispatch::<S>),
        )
        .layer(DefaultBodyLimit::disable())
}

async fn api_probe<S: OciAppState>(State(state): State<S>, headers: HeaderMap) -> Response {
    match auth::authenticate_required(&state, &headers).await {
        Ok(_) => auth::with_registry_headers(StatusCode::OK.into_response()),
        Err(_) => auth::challenge_response(
            &state,
            &auth::challenge_scope_for_catalog(),
            "Authentication required for OCI registry access",
        ),
    }
}

async fn catalog<S: OciAppState>(
    State(state): State<S>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Response {
    let identity = match auth::authenticate_optional(&state, &headers).await {
        Ok(identity) => identity,
        Err(_) => {
            return auth::challenge_response(
                &state,
                &auth::challenge_scope_for_catalog(),
                "Authentication required for OCI registry access",
            )
        }
    };
    let actor_user_id = identity.as_ref().map(|identity| identity.user_id);

    let rows = match sqlx::query(
        "SELECT p.id, p.repository_id, p.name, p.visibility AS package_visibility, \
                p.owner_user_id AS package_owner_user_id, p.owner_org_id AS package_owner_org_id, \
                r.visibility::text AS repository_visibility, \
                r.owner_user_id AS repository_owner_user_id, r.owner_org_id AS repository_owner_org_id, \
                p.normalized_name \
         FROM packages p \
         JOIN repositories r ON r.id = p.repository_id \
         WHERE p.ecosystem = 'oci' \
           AND p.is_archived = false \
           AND EXISTS (\
               SELECT 1 \
               FROM releases rel \
               JOIN artifacts a ON a.release_id = rel.id \
               WHERE rel.package_id = p.id \
                 AND rel.status = 'published' \
                 AND a.kind = 'oci_manifest'\
           ) \
         ORDER BY p.normalized_name, p.name",
    )
    .fetch_all(state.db())
    .await
    {
        Ok(rows) => rows,
        Err(_) => {
            return auth::oci_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "UNKNOWN",
                "Failed to list OCI repositories",
                None,
            )
        }
    };

    let mut repositories = Vec::new();
    for row in rows {
        let package = package_context_from_row(&row);
        if package_readable(&package, state.db(), actor_user_id).await {
            repositories.push((package.package_name.clone(), package.package_name));
        }
    }

    repositories.sort_by(|left, right| left.0.cmp(&right.0));
    let last = query.last.as_deref().map(name::normalize_repository_name);
    let repositories = repositories
        .into_iter()
        .filter(|(normalized, _)| last.as_ref().map(|last| normalized > last).unwrap_or(true))
        .take(query.limit())
        .map(|(_, package_name)| package_name)
        .collect::<Vec<_>>();

    auth::with_registry_headers(
        (
            StatusCode::OK,
            Json(serde_json::json!({
                "repositories": repositories,
            })),
        )
            .into_response(),
    )
}

async fn get_dispatch<S: OciAppState>(
    State(state): State<S>,
    Path(path): Path<String>,
    headers: HeaderMap,
    Query(query): Query<ReferrersQuery>,
) -> Response {
    if let Some((name, digest)) = parse_referrers_path(&path) {
        return referrers_list(state, headers, query, name, digest).await;
    }
    if let Some(name) = parse_tags_path(&path) {
        return tags_list(state, headers, query.into(), name).await;
    }
    if let Some((name, reference)) = parse_manifest_path(&path) {
        return manifest_get(state, headers, name, reference, true).await;
    }
    if let Some((name, digest)) = parse_blob_path(&path) {
        return blob_get(state, headers, name, digest, true).await;
    }

    auth::with_registry_headers(auth::oci_error_response(
        StatusCode::NOT_FOUND,
        "NAME_UNKNOWN",
        "OCI resource not found",
        None,
    ))
}

async fn head_dispatch<S: OciAppState>(
    State(state): State<S>,
    Path(path): Path<String>,
    headers: HeaderMap,
) -> Response {
    if let Some((name, reference)) = parse_manifest_path(&path) {
        return manifest_get(state, headers, name, reference, false).await;
    }
    if let Some((name, digest)) = parse_blob_path(&path) {
        return blob_get(state, headers, name, digest, false).await;
    }

    auth::with_registry_headers(auth::oci_error_response(
        StatusCode::NOT_FOUND,
        "NAME_UNKNOWN",
        "OCI resource not found",
        None,
    ))
}

async fn put_dispatch<S: OciAppState>(
    State(state): State<S>,
    Path(path): Path<String>,
    headers: HeaderMap,
    Query(query): Query<UploadQuery>,
    body: Bytes,
) -> Response {
    if let Some((name, reference)) = parse_manifest_path(&path) {
        return manifest_put(state, headers, name, reference, body).await;
    }
    if let Some((name, session_id)) = parse_upload_session_path(&path) {
        return finalize_blob_upload(state, headers, name, session_id, query, body).await;
    }

    auth::with_registry_headers(auth::oci_error_response(
        StatusCode::NOT_FOUND,
        "NAME_UNKNOWN",
        "OCI resource not found",
        None,
    ))
}

async fn post_dispatch<S: OciAppState>(
    State(state): State<S>,
    Path(path): Path<String>,
    headers: HeaderMap,
    Query(query): Query<UploadQuery>,
    body: Bytes,
) -> Response {
    if let Some(name) = parse_upload_start_path(&path) {
        return begin_blob_upload(state, headers, name, query, body).await;
    }

    auth::with_registry_headers(auth::oci_error_response(
        StatusCode::NOT_FOUND,
        "NAME_UNKNOWN",
        "OCI resource not found",
        None,
    ))
}

async fn patch_dispatch<S: OciAppState>(
    State(state): State<S>,
    Path(path): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some((name, session_id)) = parse_upload_session_path(&path) {
        return append_blob_upload(state, headers, name, session_id, body).await;
    }

    auth::with_registry_headers(auth::oci_error_response(
        StatusCode::NOT_FOUND,
        "BLOB_UPLOAD_UNKNOWN",
        "OCI upload session not found",
        None,
    ))
}

async fn delete_dispatch<S: OciAppState>(
    State(state): State<S>,
    Path(path): Path<String>,
    headers: HeaderMap,
) -> Response {
    if let Some((name, reference)) = parse_manifest_path(&path) {
        return manifest_delete(state, headers, name, reference).await;
    }
    if let Some((name, digest)) = parse_blob_path(&path) {
        return blob_delete(state, headers, name, digest).await;
    }

    auth::with_registry_headers(auth::oci_error_response(
        StatusCode::NOT_FOUND,
        "NAME_UNKNOWN",
        "OCI resource not found",
        None,
    ))
}

async fn tags_list<S: OciAppState>(
    state: S,
    headers: HeaderMap,
    query: ListQuery,
    package_name: String,
) -> Response {
    if let Err(error) = name::validate_repository_name(&package_name) {
        return auth::with_registry_headers(auth::oci_error_response(
            StatusCode::BAD_REQUEST,
            "NAME_INVALID",
            &error.to_string(),
            None,
        ));
    }

    let identity = match auth::authenticate_optional(&state, &headers).await {
        Ok(identity) => identity,
        Err(_) => {
            return auth::challenge_response(
                &state,
                &auth::challenge_scope_for_repository(&package_name, false),
                "Authentication required for OCI pulls",
            )
        }
    };
    let package = match load_package_context(state.db(), &package_name).await {
        Ok(package) => package,
        Err(response) => return response,
    };

    if !package_readable(
        &package,
        state.db(),
        identity.as_ref().map(|identity| identity.user_id),
    )
    .await
    {
        if identity.is_none() {
            return auth::challenge_response(
                &state,
                &auth::challenge_scope_for_repository(&package_name, false),
                "Authentication required for OCI pulls",
            );
        }

        return auth::with_registry_headers(auth::oci_error_response(
            StatusCode::NOT_FOUND,
            "NAME_UNKNOWN",
            "OCI repository not found",
            None,
        ));
    }

    let rows = match sqlx::query(
        "SELECT cr.name \
         FROM channel_refs cr \
         JOIN releases rel ON rel.id = cr.release_id \
         WHERE cr.package_id = $1 \
           AND cr.ecosystem = 'oci' \
           AND rel.status = 'published' \
         ORDER BY cr.name",
    )
    .bind(package.package_id)
    .fetch_all(state.db())
    .await
    {
        Ok(rows) => rows,
        Err(_) => {
            return auth::with_registry_headers(auth::oci_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "UNKNOWN",
                "Failed to load OCI tags",
                None,
            ))
        }
    };

    let last = query.last.as_deref();
    let tags = rows
        .into_iter()
        .filter_map(|row| row.try_get::<String, _>("name").ok())
        .filter(|tag| last.map(|last| tag.as_str() > last).unwrap_or(true))
        .take(query.limit())
        .collect::<Vec<_>>();

    auth::with_registry_headers(
        (
            StatusCode::OK,
            Json(serde_json::json!({
                "name": package.package_name,
                "tags": tags,
            })),
        )
            .into_response(),
    )
}

async fn referrers_list<S: OciAppState>(
    state: S,
    headers: HeaderMap,
    query: ReferrersQuery,
    package_name: String,
    subject_digest: String,
) -> Response {
    if let Err(error) = name::validate_repository_name(&package_name) {
        return auth::with_registry_headers(auth::oci_error_response(
            StatusCode::BAD_REQUEST,
            "NAME_INVALID",
            &error.to_string(),
            None,
        ));
    }

    let subject_digest = match name::validate_digest(&subject_digest) {
        Ok(digest) => digest,
        Err(error) => {
            return auth::with_registry_headers(auth::oci_error_response(
                StatusCode::BAD_REQUEST,
                "DIGEST_INVALID",
                &error.to_string(),
                None,
            ))
        }
    };

    let identity = match auth::authenticate_optional(&state, &headers).await {
        Ok(identity) => identity,
        Err(_) => {
            return auth::challenge_response(
                &state,
                &auth::challenge_scope_for_repository(&package_name, false),
                "Authentication required for OCI pulls",
            )
        }
    };
    let package = match load_package_context(state.db(), &package_name).await {
        Ok(package) => package,
        Err(response) => return response,
    };

    if !package_readable(
        &package,
        state.db(),
        identity.as_ref().map(|identity| identity.user_id),
    )
    .await
    {
        if identity.is_none() {
            return auth::challenge_response(
                &state,
                &auth::challenge_scope_for_repository(&package_name, false),
                "Authentication required for OCI pulls",
            );
        }

        return auth::with_registry_headers(auth::oci_error_response(
            StatusCode::NOT_FOUND,
            "NAME_UNKNOWN",
            "OCI repository not found",
            None,
        ));
    }

    let mut descriptors =
        match load_referrer_descriptors(state.db(), package.package_id, &subject_digest).await {
            Ok(descriptors) => descriptors,
            Err(_) => {
                return auth::with_registry_headers(auth::oci_error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "UNKNOWN",
                    "Failed to load OCI referrers",
                    None,
                ))
            }
        };

    descriptors.sort_by(|left, right| left.digest.cmp(&right.digest));

    if let Some(last) = query.last.as_deref() {
        descriptors.retain(|descriptor| descriptor.digest.as_str() > last);
    }

    if let Some(artifact_type) = query.artifact_type.as_deref() {
        descriptors.retain(|descriptor| descriptor.artifact_type.as_deref() == Some(artifact_type));
    }

    let next_link = match query.limit() {
        Some(0) => {
            descriptors.clear();
            None
        }
        Some(limit) if descriptors.len() > limit => {
            let last_digest = descriptors[limit - 1].digest.clone();
            descriptors.truncate(limit);
            Some(build_referrers_next_link(
                &package.package_name,
                &subject_digest,
                limit,
                &last_digest,
                query.artifact_type.as_deref(),
            ))
        }
        _ => None,
    };

    let body = serde_json::json!({
        "schemaVersion": 2,
        "mediaType": OCI_IMAGE_INDEX_MEDIA_TYPE,
        "manifests": descriptors
            .into_iter()
            .map(ReferrerDescriptor::into_json)
            .collect::<Vec<_>>(),
    });
    let body_bytes = match serde_json::to_vec(&body) {
        Ok(body_bytes) => body_bytes,
        Err(_) => {
            return auth::with_registry_headers(auth::oci_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "UNKNOWN",
                "Failed to serialize OCI referrers response",
                None,
            ))
        }
    };

    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, OCI_IMAGE_INDEX_MEDIA_TYPE)
        .header(CONTENT_LENGTH, body_bytes.len().to_string());
    if query.artifact_type.is_some() {
        builder = builder.header("OCI-Filters-Applied", "artifactType");
    }
    if let Some(next_link) = next_link {
        builder = builder.header(LINK, next_link);
    }

    auth::with_registry_headers(
        builder
            .body(Body::from(body_bytes))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()),
    )
}

async fn manifest_get<S: OciAppState>(
    state: S,
    headers: HeaderMap,
    package_name: String,
    reference: String,
    include_body: bool,
) -> Response {
    if let Err(error) = name::validate_repository_name(&package_name) {
        return auth::with_registry_headers(auth::oci_error_response(
            StatusCode::BAD_REQUEST,
            "NAME_INVALID",
            &error.to_string(),
            None,
        ));
    }

    let identity = match auth::authenticate_optional(&state, &headers).await {
        Ok(identity) => identity,
        Err(_) => {
            return auth::challenge_response(
                &state,
                &auth::challenge_scope_for_repository(&package_name, false),
                "Authentication required for OCI pulls",
            )
        }
    };
    let package = match load_package_context(state.db(), &package_name).await {
        Ok(package) => package,
        Err(response) => return response,
    };

    if !package_readable(
        &package,
        state.db(),
        identity.as_ref().map(|identity| identity.user_id),
    )
    .await
    {
        if identity.is_none() {
            return auth::challenge_response(
                &state,
                &auth::challenge_scope_for_repository(&package_name, false),
                "Authentication required for OCI pulls",
            );
        }

        return auth::with_registry_headers(auth::oci_error_response(
            StatusCode::NOT_FOUND,
            "NAME_UNKNOWN",
            "OCI repository not found",
            None,
        ));
    }

    let manifest = match load_manifest_artifact(state.db(), package.package_id, &reference).await {
        Ok(manifest) => manifest,
        Err(response) => return response,
    };

    if !include_body {
        return manifest_head_response(&manifest);
    }

    let stored = match state.artifact_get(&manifest.storage_key).await {
        Ok(Some(stored)) => stored,
        Ok(None) => {
            return auth::with_registry_headers(auth::oci_error_response(
                StatusCode::NOT_FOUND,
                "MANIFEST_UNKNOWN",
                "OCI manifest data is missing from storage",
                None,
            ))
        }
        Err(_) => {
            return auth::with_registry_headers(auth::oci_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "UNKNOWN",
                "Failed to load OCI manifest bytes",
                None,
            ))
        }
    };

    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(
            CONTENT_TYPE,
            if stored.content_type.is_empty() {
                manifest.content_type.as_str()
            } else {
                stored.content_type.as_str()
            },
        )
        .header(CONTENT_LENGTH, stored.bytes.len().to_string())
        .header("docker-content-digest", manifest.digest.clone())
        .body(Body::from(stored.bytes))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
    response = auth::with_registry_headers(response);
    response
}

async fn blob_get<S: OciAppState>(
    state: S,
    headers: HeaderMap,
    package_name: String,
    digest: String,
    include_body: bool,
) -> Response {
    if let Err(error) = name::validate_repository_name(&package_name) {
        return auth::with_registry_headers(auth::oci_error_response(
            StatusCode::BAD_REQUEST,
            "NAME_INVALID",
            &error.to_string(),
            None,
        ));
    }

    let digest = match name::validate_digest(&digest) {
        Ok(digest) => digest,
        Err(error) => {
            return auth::with_registry_headers(auth::oci_error_response(
                StatusCode::BAD_REQUEST,
                "DIGEST_INVALID",
                &error.to_string(),
                None,
            ))
        }
    };

    let identity = match auth::authenticate_optional(&state, &headers).await {
        Ok(identity) => identity,
        Err(_) => {
            return auth::challenge_response(
                &state,
                &auth::challenge_scope_for_repository(&package_name, false),
                "Authentication required for OCI pulls",
            )
        }
    };
    let package = match load_package_context(state.db(), &package_name).await {
        Ok(package) => package,
        Err(response) => return response,
    };

    if !package_readable(
        &package,
        state.db(),
        identity.as_ref().map(|identity| identity.user_id),
    )
    .await
    {
        if identity.is_none() {
            return auth::challenge_response(
                &state,
                &auth::challenge_scope_for_repository(&package_name, false),
                "Authentication required for OCI pulls",
            );
        }

        return auth::with_registry_headers(auth::oci_error_response(
            StatusCode::NOT_FOUND,
            "NAME_UNKNOWN",
            "OCI repository not found",
            None,
        ));
    }

    let blob_size =
        match blob_is_referenced_by_package(state.db(), package.package_id, &digest).await {
            Ok(Some(size_bytes)) => size_bytes,
            Ok(None) => {
                return auth::with_registry_headers(auth::oci_error_response(
                    StatusCode::NOT_FOUND,
                    "BLOB_UNKNOWN",
                    "OCI blob not found",
                    None,
                ))
            }
            Err(_) => {
                return auth::with_registry_headers(auth::oci_error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "UNKNOWN",
                    "Failed to load OCI blob metadata",
                    None,
                ))
            }
        };

    let storage_key = upload::blob_storage_key(&digest);
    if !include_body {
        let response = Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_TYPE, "application/octet-stream")
            .header(CONTENT_LENGTH, blob_size.to_string())
            .header("docker-content-digest", digest)
            .body(Body::empty())
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
        return auth::with_registry_headers(response);
    }

    let stored = match state.artifact_get(&storage_key).await {
        Ok(Some(stored)) => stored,
        Ok(None) => {
            return auth::with_registry_headers(auth::oci_error_response(
                StatusCode::NOT_FOUND,
                "BLOB_UNKNOWN",
                "OCI blob not found",
                None,
            ))
        }
        Err(_) => {
            return auth::with_registry_headers(auth::oci_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "UNKNOWN",
                "Failed to load OCI blob bytes",
                None,
            ))
        }
    };

    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(
            CONTENT_TYPE,
            if stored.content_type.is_empty() {
                "application/octet-stream"
            } else {
                stored.content_type.as_str()
            },
        )
        .header(CONTENT_LENGTH, stored.bytes.len().to_string())
        .header("docker-content-digest", digest)
        .body(Body::from(stored.bytes))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
    response = auth::with_registry_headers(response);
    response
}

async fn manifest_put<S: OciAppState>(
    state: S,
    headers: HeaderMap,
    package_name: String,
    reference: String,
    body: Bytes,
) -> Response {
    if let Err(error) = name::validate_repository_name(&package_name) {
        return auth::with_registry_headers(auth::oci_error_response(
            StatusCode::BAD_REQUEST,
            "NAME_INVALID",
            &error.to_string(),
            None,
        ));
    }

    let identity = match authenticate_push_identity(&state, &headers, &package_name).await {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let package = match resolve_or_create_package(&state, &identity, &package_name).await {
        Ok(package) => package,
        Err(response) => return response,
    };

    let request_content_type = headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let parsed_manifest = match manifest::parse_manifest(body, request_content_type.as_deref()) {
        Ok(manifest) => manifest,
        Err(error) => {
            return auth::with_registry_headers(auth::oci_error_response(
                StatusCode::BAD_REQUEST,
                "MANIFEST_INVALID",
                &error.to_string(),
                None,
            ))
        }
    };

    match name::parse_reference(&reference) {
        Ok(OciReference::Digest(digest)) if digest != parsed_manifest.digest => {
            return auth::with_registry_headers(auth::oci_error_response(
                StatusCode::BAD_REQUEST,
                "DIGEST_INVALID",
                "Manifest digest does not match the requested digest reference",
                Some(serde_json::json!({
                    "requested": digest,
                    "actual": parsed_manifest.digest,
                })),
            ))
        }
        Err(error) => {
            return auth::with_registry_headers(auth::oci_error_response(
                StatusCode::BAD_REQUEST,
                "NAME_INVALID",
                &error.to_string(),
                None,
            ))
        }
        _ => {}
    }

    for reference in &parsed_manifest.references {
        if matches!(reference.kind, ManifestReferenceKind::Subject) {
            continue;
        }
        let storage_key = upload::blob_storage_key(&reference.digest);
        match state.artifact_get(&storage_key).await {
            Ok(Some(_)) => {}
            Ok(None) => {
                return auth::with_registry_headers(auth::oci_error_response(
                    StatusCode::BAD_REQUEST,
                    "MANIFEST_BLOB_UNKNOWN",
                    "Manifest references a blob that is not present in storage",
                    Some(serde_json::json!({
                        "digest": reference.digest,
                    })),
                ))
            }
            Err(_) => {
                return auth::with_registry_headers(auth::oci_error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "UNKNOWN",
                    "Failed to validate OCI blob references",
                    None,
                ))
            }
        }
    }

    let release_id = match resolve_or_create_manifest_release(
        state.db(),
        package.package_id,
        &parsed_manifest.digest,
        identity.user_id,
        &parsed_manifest.raw,
    )
    .await
    {
        Ok(release_id) => release_id,
        Err(response) => return response,
    };

    let manifest_context =
        match upsert_manifest_artifact(&state, release_id, &parsed_manifest).await {
            Ok(context) => context,
            Err(response) => return response,
        };

    if let Err(response) =
        replace_manifest_references(state.db(), release_id, &parsed_manifest.references).await
    {
        return response;
    }

    if let Err(response) = publish_manifest_release(
        state.db(),
        &identity,
        package.package_id,
        release_id,
        &package.package_name,
        &parsed_manifest.raw,
    )
    .await
    {
        return response;
    }

    if let Ok(OciReference::Tag(tag)) = name::parse_reference(&reference) {
        if let Err(response) = upsert_tag_alias(
            state.db(),
            package.package_id,
            &tag,
            release_id,
            identity.user_id,
        )
        .await
        {
            return response;
        }
    }

    let location = format!(
        "{}/oci/v2/{}/manifests/{}",
        state.base_url().trim_end_matches('/'),
        package.package_name,
        parsed_manifest.digest,
    );

    let subject_digest = parsed_manifest.references.iter().find_map(|reference| {
        (reference.kind == ManifestReferenceKind::Subject).then_some(reference.digest.as_str())
    });
    let mut builder = Response::builder()
        .status(StatusCode::CREATED)
        .header(LOCATION, location)
        .header("docker-content-digest", parsed_manifest.digest)
        .header(CONTENT_LENGTH, "0");
    if let Some(subject_digest) = subject_digest {
        builder = builder.header("OCI-Subject", subject_digest);
    }

    let mut response = builder
        .body(Body::empty())
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
    response = auth::with_registry_headers(response);
    let _ = manifest_context;
    response
}

async fn begin_blob_upload<S: OciAppState>(
    state: S,
    headers: HeaderMap,
    package_name: String,
    query: UploadQuery,
    body: Bytes,
) -> Response {
    if let Err(error) = name::validate_repository_name(&package_name) {
        return auth::with_registry_headers(auth::oci_error_response(
            StatusCode::BAD_REQUEST,
            "NAME_INVALID",
            &error.to_string(),
            None,
        ));
    }

    let identity = match authenticate_push_identity(&state, &headers, &package_name).await {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let package = match resolve_or_create_package(&state, &identity, &package_name).await {
        Ok(package) => package,
        Err(response) => return response,
    };

    if let Some(digest) = query.digest {
        let digest = match name::validate_digest(&digest) {
            Ok(digest) => digest,
            Err(error) => {
                return auth::with_registry_headers(auth::oci_error_response(
                    StatusCode::BAD_REQUEST,
                    "DIGEST_INVALID",
                    &error.to_string(),
                    None,
                ))
            }
        };
        if body.is_empty() {
            return auth::with_registry_headers(auth::oci_error_response(
                StatusCode::BAD_REQUEST,
                "BLOB_UPLOAD_INVALID",
                "Monolithic blob uploads must include a request body",
                None,
            ));
        }

        let uploaded_digest = digest_for_bytes(&body);
        if uploaded_digest != digest {
            return auth::with_registry_headers(auth::oci_error_response(
                StatusCode::BAD_REQUEST,
                "DIGEST_INVALID",
                "Blob digest does not match the supplied digest query parameter",
                Some(serde_json::json!({
                    "requested": digest,
                    "actual": uploaded_digest,
                })),
            ));
        }

        let storage_key = upload::blob_storage_key(&digest);
        if state
            .artifact_put(storage_key.clone(), "application/octet-stream".into(), body)
            .await
            .is_err()
        {
            return auth::with_registry_headers(auth::oci_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "UNKNOWN",
                "Failed to persist OCI blob",
                None,
            ));
        }

        let location = format!(
            "{}/oci/v2/{}/blobs/{}",
            state.base_url().trim_end_matches('/'),
            package.package_name,
            digest,
        );
        let mut response = Response::builder()
            .status(StatusCode::CREATED)
            .header(LOCATION, location)
            .header("docker-content-digest", digest)
            .header(CONTENT_LENGTH, "0")
            .body(Body::empty())
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
        response = auth::with_registry_headers(response);
        return response;
    }

    if !body.is_empty() {
        return auth::with_registry_headers(auth::oci_error_response(
            StatusCode::BAD_REQUEST,
            "BLOB_UPLOAD_INVALID",
            "Use PATCH or PUT with ?digest= for OCI upload bodies",
            None,
        ));
    }

    let session = match upload::begin_upload_session(
        state.db(),
        package.repository_id,
        &package.package_name,
        identity.user_id,
    )
    .await
    {
        Ok(session) => session,
        Err(_) => {
            return auth::with_registry_headers(auth::oci_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "UNKNOWN",
                "Failed to start OCI upload session",
                None,
            ))
        }
    };

    upload_session_response(
        &state,
        &package.package_name,
        &session,
        StatusCode::ACCEPTED,
    )
}

async fn append_blob_upload<S: OciAppState>(
    state: S,
    headers: HeaderMap,
    package_name: String,
    session_id: Uuid,
    body: Bytes,
) -> Response {
    if body.is_empty() {
        return auth::with_registry_headers(auth::oci_error_response(
            StatusCode::BAD_REQUEST,
            "BLOB_UPLOAD_INVALID",
            "OCI upload chunks must not be empty",
            None,
        ));
    }

    let _identity = match authenticate_push_identity(&state, &headers, &package_name).await {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let session = match load_matching_upload_session(state.db(), session_id, &package_name).await {
        Ok(session) => session,
        Err(response) => return response,
    };

    let mut existing_bytes = match state.artifact_get(&session.storage_key).await {
        Ok(Some(stored)) => stored.bytes.to_vec(),
        Ok(None) => Vec::new(),
        Err(_) => {
            return auth::with_registry_headers(auth::oci_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "UNKNOWN",
                "Failed to load existing OCI upload session bytes",
                None,
            ))
        }
    };
    existing_bytes.extend_from_slice(&body);

    if state
        .artifact_put(
            session.storage_key.clone(),
            "application/octet-stream".into(),
            Bytes::from(existing_bytes.clone()),
        )
        .await
        .is_err()
    {
        return auth::with_registry_headers(auth::oci_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "UNKNOWN",
            "Failed to append OCI upload chunk",
            None,
        ));
    }

    let received_bytes = i64::try_from(existing_bytes.len()).unwrap_or(i64::MAX);
    if upload::update_upload_session_received_bytes(state.db(), session.id, received_bytes)
        .await
        .is_err()
    {
        return auth::with_registry_headers(auth::oci_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "UNKNOWN",
            "Failed to update OCI upload session state",
            None,
        ));
    }

    let session = UploadSessionRecord {
        received_bytes,
        ..session
    };
    upload_session_response(&state, &package_name, &session, StatusCode::ACCEPTED)
}

async fn finalize_blob_upload<S: OciAppState>(
    state: S,
    headers: HeaderMap,
    package_name: String,
    session_id: Uuid,
    query: UploadQuery,
    body: Bytes,
) -> Response {
    let _identity = match authenticate_push_identity(&state, &headers, &package_name).await {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let digest = match query.digest.as_deref() {
        Some(digest) => match name::validate_digest(digest) {
            Ok(digest) => digest,
            Err(error) => {
                return auth::with_registry_headers(auth::oci_error_response(
                    StatusCode::BAD_REQUEST,
                    "DIGEST_INVALID",
                    &error.to_string(),
                    None,
                ))
            }
        },
        None => {
            return auth::with_registry_headers(auth::oci_error_response(
                StatusCode::BAD_REQUEST,
                "DIGEST_INVALID",
                "Finalizing an OCI upload requires the digest query parameter",
                None,
            ))
        }
    };

    let session = match load_matching_upload_session(state.db(), session_id, &package_name).await {
        Ok(session) => session,
        Err(response) => return response,
    };

    let mut bytes = match state.artifact_get(&session.storage_key).await {
        Ok(Some(stored)) => stored.bytes.to_vec(),
        Ok(None) => Vec::new(),
        Err(_) => {
            return auth::with_registry_headers(auth::oci_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "UNKNOWN",
                "Failed to load OCI upload session bytes",
                None,
            ))
        }
    };
    if !body.is_empty() {
        bytes.extend_from_slice(&body);
    }
    if bytes.is_empty() {
        return auth::with_registry_headers(auth::oci_error_response(
            StatusCode::BAD_REQUEST,
            "BLOB_UPLOAD_INVALID",
            "OCI uploads must contain at least one byte before finalization",
            None,
        ));
    }

    let uploaded_digest = digest_for_bytes(&bytes);
    if uploaded_digest != digest {
        return auth::with_registry_headers(auth::oci_error_response(
            StatusCode::BAD_REQUEST,
            "DIGEST_INVALID",
            "Blob digest does not match the supplied digest query parameter",
            Some(serde_json::json!({
                "requested": digest,
                "actual": uploaded_digest,
            })),
        ));
    }

    let final_storage_key = upload::blob_storage_key(&digest);
    if state
        .artifact_put(
            final_storage_key.clone(),
            "application/octet-stream".into(),
            Bytes::from(bytes),
        )
        .await
        .is_err()
    {
        return auth::with_registry_headers(auth::oci_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "UNKNOWN",
            "Failed to persist OCI blob",
            None,
        ));
    }

    let _ = state.artifact_delete(&session.storage_key).await;
    let _ = upload::delete_upload_session(state.db(), session.id).await;

    let location = format!(
        "{}/oci/v2/{}/blobs/{}",
        state.base_url().trim_end_matches('/'),
        package_name,
        digest,
    );
    let mut response = Response::builder()
        .status(StatusCode::CREATED)
        .header(LOCATION, location)
        .header("docker-content-digest", digest)
        .header(CONTENT_LENGTH, "0")
        .body(Body::empty())
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
    response = auth::with_registry_headers(response);
    response
}

async fn manifest_delete<S: OciAppState>(
    state: S,
    headers: HeaderMap,
    package_name: String,
    reference: String,
) -> Response {
    let identity = match authenticate_push_identity(&state, &headers, &package_name).await {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let package =
        match resolve_existing_package_for_write(state.db(), identity.user_id, &package_name).await
        {
            Ok(package) => package,
            Err(response) => return response,
        };

    match name::parse_reference(&reference) {
        Ok(OciReference::Tag(tag)) => {
            let deleted = match sqlx::query(
                "DELETE FROM channel_refs WHERE package_id = $1 AND ecosystem = 'oci' AND name = $2",
            )
            .bind(package.package_id)
            .bind(&tag)
            .execute(state.db())
            .await
            {
                Ok(result) => result.rows_affected(),
                Err(_) => {
                    return auth::with_registry_headers(auth::oci_error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "UNKNOWN",
                        "Failed to delete OCI tag",
                        None,
                    ))
                }
            };

            if deleted == 0 {
                return auth::with_registry_headers(auth::oci_error_response(
                    StatusCode::NOT_FOUND,
                    "MANIFEST_UNKNOWN",
                    "OCI tag not found",
                    None,
                ));
            }

            auth::with_registry_headers(StatusCode::ACCEPTED.into_response())
        }
        Ok(OciReference::Digest(digest)) => {
            let manifest =
                match load_manifest_artifact(state.db(), package.package_id, &digest).await {
                    Ok(manifest) => manifest,
                    Err(response) => return response,
                };

            let _ = sqlx::query("DELETE FROM channel_refs WHERE package_id = $1 AND release_id = $2 AND ecosystem = 'oci'")
                .bind(package.package_id)
                .bind(manifest.release_id)
                .execute(state.db())
                .await;
            let _ = sqlx::query("DELETE FROM oci_manifest_references WHERE release_id = $1")
                .bind(manifest.release_id)
                .execute(state.db())
                .await;
            let _ = sqlx::query(
                "DELETE FROM artifacts WHERE release_id = $1 AND kind = 'oci_manifest'",
            )
            .bind(manifest.release_id)
            .execute(state.db())
            .await;
            let _ = state.artifact_delete(&manifest.storage_key).await;
            let _ = sqlx::query(
                "UPDATE releases SET status = 'deleted', updated_at = NOW() WHERE id = $1",
            )
            .bind(manifest.release_id)
            .execute(state.db())
            .await;

            auth::with_registry_headers(StatusCode::ACCEPTED.into_response())
        }
        Err(error) => auth::with_registry_headers(auth::oci_error_response(
            StatusCode::BAD_REQUEST,
            "NAME_INVALID",
            &error.to_string(),
            None,
        )),
    }
}

async fn blob_delete<S: OciAppState>(
    state: S,
    headers: HeaderMap,
    package_name: String,
    digest: String,
) -> Response {
    let identity = match authenticate_push_identity(&state, &headers, &package_name).await {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let package =
        match resolve_existing_package_for_write(state.db(), identity.user_id, &package_name).await
        {
            Ok(package) => package,
            Err(response) => return response,
        };

    if !has_package_admin_access(
        state.db(),
        package.package_id,
        package.repository_id,
        package.package_owner_user_id,
        package.package_owner_org_id,
        identity.user_id,
    )
    .await
    {
        return auth::with_registry_headers(auth::oci_error_response(
            StatusCode::FORBIDDEN,
            "DENIED",
            "Deleting OCI blobs requires package administration permission",
            None,
        ));
    }

    let digest = match name::validate_digest(&digest) {
        Ok(digest) => digest,
        Err(error) => {
            return auth::with_registry_headers(auth::oci_error_response(
                StatusCode::BAD_REQUEST,
                "DIGEST_INVALID",
                &error.to_string(),
                None,
            ))
        }
    };

    match blob_is_referenced_anywhere(state.db(), &digest).await {
        Ok(true) => {
            return auth::with_registry_headers(auth::oci_error_response(
                StatusCode::CONFLICT,
                "DENIED",
                "The OCI blob is still referenced by at least one published manifest",
                Some(serde_json::json!({ "digest": digest })),
            ))
        }
        Ok(false) => {}
        Err(_) => {
            return auth::with_registry_headers(auth::oci_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "UNKNOWN",
                "Failed to validate OCI blob references",
                None,
            ))
        }
    }

    let storage_key = upload::blob_storage_key(&digest);
    match state.artifact_get(&storage_key).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return auth::with_registry_headers(auth::oci_error_response(
                StatusCode::NOT_FOUND,
                "BLOB_UNKNOWN",
                "OCI blob not found",
                None,
            ))
        }
        Err(_) => {
            return auth::with_registry_headers(auth::oci_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "UNKNOWN",
                "Failed to load OCI blob metadata",
                None,
            ))
        }
    }

    if state.artifact_delete(&storage_key).await.is_err() {
        return auth::with_registry_headers(auth::oci_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "UNKNOWN",
            "Failed to delete OCI blob",
            None,
        ));
    }

    auth::with_registry_headers(StatusCode::ACCEPTED.into_response())
}

async fn authenticate_push_identity<S: OciAppState>(
    state: &S,
    headers: &HeaderMap,
    package_name: &str,
) -> Result<OciIdentity, Response> {
    let identity = match auth::authenticate_required(state, headers).await {
        Ok(identity) => identity,
        Err(AuthFailure::Missing | AuthFailure::Invalid(_)) => {
            return Err(auth::challenge_response(
                state,
                &auth::challenge_scope_for_repository(package_name, true),
                "Authentication required for OCI pushes",
            ))
        }
    };

    if !auth::has_scope(&identity, "packages:write") {
        return Err(auth::with_registry_headers(auth::oci_error_response(
            StatusCode::FORBIDDEN,
            "DENIED",
            "The supplied credential does not include the packages:write scope",
            None,
        )));
    }

    Ok(identity)
}

fn manifest_head_response(manifest: &ManifestArtifactContext) -> Response {
    auth::with_registry_headers(
        Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_TYPE, manifest.content_type.as_str())
            .header(CONTENT_LENGTH, manifest.size_bytes.to_string())
            .header("docker-content-digest", manifest.digest.clone())
            .body(Body::empty())
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()),
    )
}

fn upload_session_response<S: OciAppState>(
    state: &S,
    package_name: &str,
    session: &UploadSessionRecord,
    status: StatusCode,
) -> Response {
    let location = format!(
        "{}/oci/v2/{}/blobs/uploads/{}",
        state.base_url().trim_end_matches('/'),
        package_name,
        session.id,
    );
    let mut builder = Response::builder()
        .status(status)
        .header(LOCATION, location)
        .header(CONTENT_LENGTH, "0");
    if session.received_bytes > 0 {
        builder = builder.header(
            HeaderName::from_static("range"),
            format!("0-{}", session.received_bytes - 1),
        );
    }
    auth::with_registry_headers(
        builder
            .body(Body::empty())
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()),
    )
}

async fn load_package_context(db: &PgPool, package_name: &str) -> Result<PackageContext, Response> {
    let normalized_name = normalize_package_name(package_name, &Ecosystem::Oci);
    let row = sqlx::query(
        "SELECT p.id, p.repository_id, p.name, p.visibility AS package_visibility, \
                p.owner_user_id AS package_owner_user_id, p.owner_org_id AS package_owner_org_id, \
                r.visibility::text AS repository_visibility, \
                r.owner_user_id AS repository_owner_user_id, r.owner_org_id AS repository_owner_org_id \
         FROM packages p \
         JOIN repositories r ON r.id = p.repository_id \
         WHERE p.ecosystem = 'oci' AND p.normalized_name = $1",
    )
    .bind(&normalized_name)
    .fetch_optional(db)
    .await
    .map_err(|_| {
        auth::with_registry_headers(auth::oci_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "UNKNOWN",
            "Failed to load OCI repository",
            None,
        ))
    })?
    .ok_or_else(|| {
        auth::with_registry_headers(auth::oci_error_response(
            StatusCode::NOT_FOUND,
            "NAME_UNKNOWN",
            "OCI repository not found",
            None,
        ))
    })?;

    Ok(package_context_from_row(&row))
}

fn package_context_from_row(row: &sqlx::postgres::PgRow) -> PackageContext {
    PackageContext {
        package_id: row.try_get("id").unwrap_or_default(),
        repository_id: row.try_get("repository_id").unwrap_or_default(),
        package_name: row.try_get("name").unwrap_or_default(),
        package_visibility: row
            .try_get("package_visibility")
            .unwrap_or_else(|_| "public".into()),
        package_owner_user_id: row.try_get("package_owner_user_id").unwrap_or(None),
        package_owner_org_id: row.try_get("package_owner_org_id").unwrap_or(None),
        repository_visibility: row
            .try_get("repository_visibility")
            .unwrap_or_else(|_| "public".into()),
        repository_owner_user_id: row.try_get("repository_owner_user_id").unwrap_or(None),
        repository_owner_org_id: row.try_get("repository_owner_org_id").unwrap_or(None),
    }
}

async fn resolve_or_create_package<S: OciAppState>(
    state: &S,
    identity: &OciIdentity,
    package_name: &str,
) -> Result<PackageContext, Response> {
    let normalized_name = normalize_package_name(package_name, &Ecosystem::Oci);
    let existing = match sqlx::query(
        "SELECT p.id, p.repository_id, p.name, p.visibility AS package_visibility, \
                p.owner_user_id AS package_owner_user_id, p.owner_org_id AS package_owner_org_id, \
                r.visibility::text AS repository_visibility, \
                r.owner_user_id AS repository_owner_user_id, r.owner_org_id AS repository_owner_org_id \
         FROM packages p \
         JOIN repositories r ON r.id = p.repository_id \
         WHERE p.ecosystem = 'oci' AND p.normalized_name = $1",
    )
    .bind(&normalized_name)
    .fetch_optional(state.db())
    .await
    {
        Ok(row) => row,
        Err(_) => {
            return Err(auth::with_registry_headers(auth::oci_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "UNKNOWN",
                "Failed to load OCI repository",
                None,
            )))
        }
    };

    if let Some(row) = existing {
        let package = package_context_from_row(&row);
        if !has_package_write_access(
            state.db(),
            package.package_id,
            package.repository_id,
            package.package_owner_user_id,
            package.package_owner_org_id,
            identity.user_id,
        )
        .await
        {
            return Err(auth::with_registry_headers(auth::oci_error_response(
                StatusCode::FORBIDDEN,
                "DENIED",
                "You do not have permission to push to this OCI repository",
                None,
            )));
        }
        return Ok(package);
    }

    let repository = match upload::select_default_repository(state.db(), identity.user_id).await {
        Ok(repository) => repository,
        Err(Error::Forbidden(message)) => {
            return Err(auth::with_registry_headers(auth::oci_error_response(
                StatusCode::FORBIDDEN,
                "DENIED",
                &message,
                None,
            )))
        }
        Err(_) => {
            return Err(auth::with_registry_headers(auth::oci_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "UNKNOWN",
                "Failed to resolve a target repository for OCI pushes",
                None,
            )))
        }
    };

    let package_id = Uuid::new_v4();
    let now = Utc::now();
    let insert_result = sqlx::query(
        "INSERT INTO packages (id, repository_id, ecosystem, name, normalized_name, visibility, owner_user_id, owner_org_id, is_deprecated, is_archived, download_count, created_at, updated_at) \
         VALUES ($1, $2, 'oci', $3, $4, $5, $6, $7, false, false, 0, $8, $9)",
    )
    .bind(package_id)
    .bind(repository.id)
    .bind(package_name)
    .bind(&normalized_name)
    .bind(&repository.visibility)
    .bind(repository.owner_user_id)
    .bind(repository.owner_org_id)
    .bind(now)
    .bind(now)
    .execute(state.db())
    .await;

    match insert_result {
        Ok(_) => {
            let _ = sqlx::query(
                "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_package_id, metadata, occurred_at) \
                 VALUES ($1, 'package_create', $2, $3, $4, $5, NOW())",
            )
            .bind(Uuid::new_v4())
            .bind(identity.user_id)
            .bind(identity.token_id)
            .bind(package_id)
            .bind(serde_json::json!({
                "ecosystem": "oci",
                "name": package_name,
                "source": "oci_push",
            }))
            .execute(state.db())
            .await;
        }
        Err(sqlx::Error::Database(db_error)) if db_error.is_unique_violation() => {}
        Err(_) => {
            return Err(auth::with_registry_headers(auth::oci_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "UNKNOWN",
                "Failed to create OCI repository package",
                None,
            )))
        }
    }

    resolve_existing_package_for_write(state.db(), identity.user_id, package_name).await
}

async fn resolve_existing_package_for_write(
    db: &PgPool,
    actor_user_id: Uuid,
    package_name: &str,
) -> Result<PackageContext, Response> {
    let package = load_package_context(db, package_name).await?;
    if !has_package_write_access(
        db,
        package.package_id,
        package.repository_id,
        package.package_owner_user_id,
        package.package_owner_org_id,
        actor_user_id,
    )
    .await
    {
        return Err(auth::with_registry_headers(auth::oci_error_response(
            StatusCode::FORBIDDEN,
            "DENIED",
            "You do not have permission to push to this OCI repository",
            None,
        )));
    }
    Ok(package)
}

async fn resolve_or_create_manifest_release(
    db: &PgPool,
    package_id: Uuid,
    digest: &str,
    actor_user_id: Uuid,
    manifest_json: &serde_json::Value,
) -> Result<Uuid, Response> {
    let existing = match sqlx::query(
        "SELECT id, status::text AS status FROM releases WHERE package_id = $1 AND version = $2",
    )
    .bind(package_id)
    .bind(digest)
    .fetch_optional(db)
    .await
    {
        Ok(existing) => existing,
        Err(_) => {
            return Err(auth::with_registry_headers(auth::oci_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "UNKNOWN",
                "Failed to load OCI manifest release",
                None,
            )))
        }
    };

    if let Some(existing) = existing {
        let release_id: Uuid = existing.try_get("id").unwrap_or_default();
        let status: String = existing.try_get("status").unwrap_or_default();
        if status == "deleted" {
            let _ = sqlx::query(
                "UPDATE releases SET status = 'quarantine', published_by = $1, provenance = $2, updated_at = NOW() WHERE id = $3",
            )
            .bind(actor_user_id)
            .bind(manifest_json)
            .bind(release_id)
            .execute(db)
            .await;
        }
        return Ok(release_id);
    }

    let mut release = Release::new(package_id, digest.to_owned(), actor_user_id);
    release.description = Some(format!("OCI manifest {digest}"));
    release.provenance = Some(manifest_json.clone());

    sqlx::query(
        "INSERT INTO releases (id, package_id, version, status, published_by, description, changelog, is_prerelease, is_yanked, yank_reason, is_deprecated, deprecation_message, source_ref, provenance, published_at, updated_at) \
         VALUES ($1, $2, $3, 'quarantine', $4, $5, NULL, false, false, NULL, false, NULL, NULL, $6, $7, $8)",
    )
    .bind(release.id)
    .bind(release.package_id)
    .bind(&release.version)
    .bind(release.published_by)
    .bind(&release.description)
    .bind(&release.provenance)
    .bind(release.published_at)
    .bind(release.updated_at)
    .execute(db)
    .await
    .map_err(|_| {
        auth::with_registry_headers(auth::oci_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "UNKNOWN",
            "Failed to create OCI manifest release",
            None,
        ))
    })?;

    Ok(release.id)
}

async fn upsert_manifest_artifact<S: OciAppState>(
    state: &S,
    release_id: Uuid,
    manifest: &manifest::ParsedManifest,
) -> Result<ManifestArtifactContext, Response> {
    let existing = match sqlx::query(
        "SELECT storage_key, content_type, size_bytes, sha256 \
         FROM artifacts \
         WHERE release_id = $1 AND kind = 'oci_manifest' \
         LIMIT 1",
    )
    .bind(release_id)
    .fetch_optional(state.db())
    .await
    {
        Ok(existing) => existing,
        Err(_) => {
            return Err(auth::with_registry_headers(auth::oci_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "UNKNOWN",
                "Failed to load OCI manifest artifact metadata",
                None,
            )))
        }
    };

    if let Some(existing) = existing {
        let existing_digest: String = existing.try_get("sha256").unwrap_or_default();
        if existing_digest != manifest.sha256 {
            return Err(auth::with_registry_headers(auth::oci_error_response(
                StatusCode::CONFLICT,
                "DENIED",
                "The OCI manifest release already stores different content for this digest",
                None,
            )));
        }

        return Ok(ManifestArtifactContext {
            release_id,
            storage_key: existing.try_get("storage_key").unwrap_or_default(),
            content_type: existing
                .try_get("content_type")
                .unwrap_or_else(|_| manifest.content_type.clone()),
            size_bytes: existing
                .try_get("size_bytes")
                .unwrap_or(manifest.size_bytes),
            digest: manifest.digest.clone(),
        });
    }

    let storage_key = upload::manifest_storage_key(release_id, &manifest.digest);
    if state
        .artifact_put(
            storage_key.clone(),
            manifest.content_type.clone(),
            manifest.bytes.clone(),
        )
        .await
        .is_err()
    {
        return Err(auth::with_registry_headers(auth::oci_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "UNKNOWN",
            "Failed to persist OCI manifest bytes",
            None,
        )));
    }

    let mut artifact = Artifact::new(
        release_id,
        ArtifactKind::OciManifest,
        upload::manifest_filename(&manifest.digest),
        storage_key.clone(),
        manifest.content_type.clone(),
        manifest.size_bytes,
        manifest.sha256.clone(),
    );
    artifact.sha512 = Some(manifest.sha512.clone());

    sqlx::query(
        "INSERT INTO artifacts (id, release_id, kind, filename, storage_key, content_type, size_bytes, sha256, sha512, md5, is_signed, signature_key_id, uploaded_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NULL, false, NULL, $10)",
    )
    .bind(artifact.id)
    .bind(artifact.release_id)
    .bind(artifact.kind)
    .bind(&artifact.filename)
    .bind(&artifact.storage_key)
    .bind(&artifact.content_type)
    .bind(artifact.size_bytes)
    .bind(&artifact.sha256)
    .bind(&artifact.sha512)
    .bind(artifact.uploaded_at)
    .execute(state.db())
    .await
    .map_err(|_| {
        auth::with_registry_headers(auth::oci_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "UNKNOWN",
            "Failed to record OCI manifest artifact metadata",
            None,
        ))
    })?;

    Ok(ManifestArtifactContext {
        release_id,
        storage_key,
        content_type: manifest.content_type.clone(),
        size_bytes: manifest.size_bytes,
        digest: manifest.digest.clone(),
    })
}

async fn replace_manifest_references(
    db: &PgPool,
    release_id: Uuid,
    references: &[ManifestReference],
) -> Result<(), Response> {
    sqlx::query("DELETE FROM oci_manifest_references WHERE release_id = $1")
        .bind(release_id)
        .execute(db)
        .await
        .map_err(|_| {
            auth::with_registry_headers(auth::oci_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "UNKNOWN",
                "Failed to clear OCI manifest references",
                None,
            ))
        })?;

    for reference in references {
        sqlx::query(
            "INSERT INTO oci_manifest_references (release_id, ref_digest, ref_kind, ref_size, created_at) \
             VALUES ($1, $2, $3, $4, NOW())",
        )
        .bind(release_id)
        .bind(&reference.digest)
        .bind(reference.kind.as_str())
        .bind(reference.size_bytes)
        .execute(db)
        .await
        .map_err(|_| {
            auth::with_registry_headers(auth::oci_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "UNKNOWN",
                "Failed to persist OCI manifest references",
                None,
            ))
        })?;
    }

    Ok(())
}

async fn publish_manifest_release(
    db: &PgPool,
    identity: &OciIdentity,
    package_id: Uuid,
    release_id: Uuid,
    package_name: &str,
    manifest_json: &serde_json::Value,
) -> Result<(), Response> {
    let current_status: String =
        sqlx::query_scalar("SELECT status::text FROM releases WHERE id = $1")
            .bind(release_id)
            .fetch_one(db)
            .await
            .map_err(|_| {
                auth::with_registry_headers(auth::oci_error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "UNKNOWN",
                    "Failed to load OCI manifest release status",
                    None,
                ))
            })?;

    if current_status != "published" {
        sqlx::query(
            "UPDATE releases SET status = 'published', provenance = COALESCE($1, provenance), updated_at = NOW() WHERE id = $2",
        )
        .bind(manifest_json)
        .bind(release_id)
        .execute(db)
        .await
        .map_err(|_| {
            auth::with_registry_headers(auth::oci_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "UNKNOWN",
                "Failed to publish OCI manifest release",
                None,
            ))
        })?;

        let _ = sqlx::query(
            "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_package_id, target_release_id, metadata, occurred_at) \
             VALUES ($1, 'release_publish', $2, $3, $4, $5, $6, NOW())",
        )
        .bind(Uuid::new_v4())
        .bind(identity.user_id)
        .bind(identity.token_id)
        .bind(package_id)
        .bind(release_id)
        .bind(serde_json::json!({
            "ecosystem": "oci",
            "name": package_name,
            "source": "oci_manifest_put",
        }))
        .execute(db)
        .await;
    }

    Ok(())
}

async fn upsert_tag_alias(
    db: &PgPool,
    package_id: Uuid,
    tag: &str,
    release_id: Uuid,
    created_by: Uuid,
) -> Result<(), Response> {
    sqlx::query(
        "INSERT INTO channel_refs (id, package_id, ecosystem, name, release_id, created_by, created_at, updated_at) \
         VALUES ($1, $2, 'oci', $3, $4, $5, NOW(), NOW()) \
         ON CONFLICT (package_id, name) \
         DO UPDATE SET release_id = EXCLUDED.release_id, updated_at = NOW()",
    )
    .bind(Uuid::new_v4())
    .bind(package_id)
    .bind(tag)
    .bind(release_id)
    .bind(created_by)
    .execute(db)
    .await
    .map_err(|_| {
        auth::with_registry_headers(auth::oci_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "UNKNOWN",
            "Failed to update OCI tag alias",
            None,
        ))
    })?;

    Ok(())
}

async fn load_manifest_artifact(
    db: &PgPool,
    package_id: Uuid,
    reference: &str,
) -> Result<ManifestArtifactContext, Response> {
    let parsed_reference = match name::parse_reference(reference) {
        Ok(reference) => reference,
        Err(error) => {
            return Err(auth::with_registry_headers(auth::oci_error_response(
                StatusCode::BAD_REQUEST,
                "NAME_INVALID",
                &error.to_string(),
                None,
            )))
        }
    };

    let row = match parsed_reference {
        OciReference::Tag(tag) => sqlx::query(
            "SELECT rel.id AS release_id, a.storage_key, a.content_type, a.size_bytes, a.sha256 \
             FROM channel_refs cr \
             JOIN releases rel ON rel.id = cr.release_id \
             JOIN artifacts a ON a.release_id = rel.id AND a.kind = 'oci_manifest' \
             WHERE cr.package_id = $1 AND cr.ecosystem = 'oci' AND cr.name = $2 AND rel.status = 'published' \
             LIMIT 1",
        )
        .bind(package_id)
        .bind(&tag)
        .fetch_optional(db)
        .await,
        OciReference::Digest(digest) => sqlx::query(
            "SELECT rel.id AS release_id, a.storage_key, a.content_type, a.size_bytes, a.sha256 \
             FROM releases rel \
             JOIN artifacts a ON a.release_id = rel.id AND a.kind = 'oci_manifest' \
             WHERE rel.package_id = $1 AND rel.status = 'published' AND rel.version = $2 \
             LIMIT 1",
        )
        .bind(package_id)
        .bind(&digest)
        .fetch_optional(db)
        .await,
    }
    .map_err(|_| {
        auth::with_registry_headers(auth::oci_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "UNKNOWN",
            "Failed to load OCI manifest metadata",
            None,
        ))
    })?
    .ok_or_else(|| {
        auth::with_registry_headers(auth::oci_error_response(
            StatusCode::NOT_FOUND,
            "MANIFEST_UNKNOWN",
            "OCI manifest not found",
            None,
        ))
    })?;

    let sha256: String = row.try_get("sha256").unwrap_or_default();
    Ok(ManifestArtifactContext {
        release_id: row.try_get("release_id").unwrap_or_default(),
        storage_key: row.try_get("storage_key").unwrap_or_default(),
        content_type: row
            .try_get("content_type")
            .unwrap_or_else(|_| "application/vnd.oci.image.manifest.v1+json".into()),
        size_bytes: row.try_get("size_bytes").unwrap_or(0_i64),
        digest: format!("sha256:{sha256}"),
    })
}

async fn load_referrer_descriptors(
    db: &PgPool,
    package_id: Uuid,
    subject_digest: &str,
) -> Result<Vec<ReferrerDescriptor>, sqlx::Error> {
    sqlx::query(
        "SELECT a.content_type, a.size_bytes, a.sha256, rel.provenance \
         FROM oci_manifest_references omr \
         JOIN releases rel ON rel.id = omr.release_id \
         JOIN artifacts a ON a.release_id = rel.id AND a.kind = 'oci_manifest' \
         WHERE rel.package_id = $1 \
           AND rel.status = 'published' \
           AND omr.ref_kind = 'subject' \
           AND omr.ref_digest = $2 \
         ORDER BY a.sha256 ASC",
    )
    .bind(package_id)
    .bind(subject_digest)
    .fetch_all(db)
    .await
    .map(|rows| {
        rows.into_iter()
            .map(|row| {
                let provenance = row
                    .try_get::<Option<serde_json::Value>, _>("provenance")
                    .unwrap_or(None);
                let fallback_content_type: String = row
                    .try_get("content_type")
                    .unwrap_or_else(|_| OCI_IMAGE_MANIFEST_MEDIA_TYPE.into());
                let media_type = resolve_referrer_media_type(
                    provenance.as_ref(),
                    fallback_content_type.as_str(),
                );

                ReferrerDescriptor {
                    media_type: media_type.clone(),
                    digest: format!(
                        "sha256:{}",
                        row.try_get::<String, _>("sha256").unwrap_or_default()
                    ),
                    size_bytes: row.try_get("size_bytes").unwrap_or_default(),
                    artifact_type: resolve_referrer_artifact_type(
                        media_type.as_str(),
                        provenance.as_ref(),
                    ),
                    annotations: resolve_referrer_annotations(provenance.as_ref()),
                }
            })
            .collect()
    })
}

fn resolve_referrer_media_type(
    provenance: Option<&serde_json::Value>,
    fallback_content_type: &str,
) -> String {
    provenance
        .and_then(serde_json::Value::as_object)
        .and_then(|object| object.get("mediaType"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            let fallback = fallback_content_type.trim();
            if fallback.is_empty() {
                OCI_IMAGE_MANIFEST_MEDIA_TYPE
            } else {
                fallback
            }
        })
        .to_owned()
}

fn resolve_referrer_artifact_type(
    media_type: &str,
    provenance: Option<&serde_json::Value>,
) -> Option<String> {
    let object = provenance.and_then(serde_json::Value::as_object);
    if let Some(artifact_type) = object
        .and_then(|object| object.get("artifactType"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(artifact_type.to_owned());
    }

    if media_type == OCI_IMAGE_MANIFEST_MEDIA_TYPE {
        return object
            .and_then(|object| object.get("config"))
            .and_then(serde_json::Value::as_object)
            .and_then(|config| config.get("mediaType"))
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
    }

    None
}

fn resolve_referrer_annotations(
    provenance: Option<&serde_json::Value>,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    provenance
        .and_then(serde_json::Value::as_object)
        .and_then(|object| object.get("annotations"))
        .and_then(serde_json::Value::as_object)
        .filter(|annotations| !annotations.is_empty())
        .cloned()
}

fn build_referrers_next_link(
    package_name: &str,
    subject_digest: &str,
    limit: usize,
    last_digest: &str,
    artifact_type: Option<&str>,
) -> String {
    let mut query = vec![
        format!("n={limit}"),
        format!("last={}", percent_encode_query_value(last_digest)),
    ];
    if let Some(artifact_type) = artifact_type {
        query.push(format!(
            "artifactType={}",
            percent_encode_query_value(artifact_type)
        ));
    }

    format!(
        "</oci/v2/{package_name}/referrers/{subject_digest}?{}>; rel=\"next\"",
        query.join("&")
    )
}

fn percent_encode_query_value(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(byte as char)
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }

    encoded
}

async fn blob_is_referenced_by_package(
    db: &PgPool,
    package_id: Uuid,
    digest: &str,
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT omr.ref_size \
         FROM oci_manifest_references omr \
         JOIN releases rel ON rel.id = omr.release_id \
         WHERE rel.package_id = $1 AND rel.status = 'published' AND omr.ref_digest = $2 \
         LIMIT 1",
    )
    .bind(package_id)
    .bind(digest)
    .fetch_optional(db)
    .await
}

async fn blob_is_referenced_anywhere(db: &PgPool, digest: &str) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(\
             SELECT 1 \
             FROM oci_manifest_references omr \
             JOIN releases rel ON rel.id = omr.release_id \
             WHERE omr.ref_digest = $1 AND rel.status = 'published'\
         )",
    )
    .bind(digest)
    .fetch_one(db)
    .await
}

async fn load_matching_upload_session(
    db: &PgPool,
    session_id: Uuid,
    package_name: &str,
) -> Result<UploadSessionRecord, Response> {
    let session = upload::load_upload_session(db, session_id)
        .await
        .map_err(|_| {
            auth::with_registry_headers(auth::oci_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "UNKNOWN",
                "Failed to load OCI upload session",
                None,
            ))
        })?
        .ok_or_else(|| {
            auth::with_registry_headers(auth::oci_error_response(
                StatusCode::NOT_FOUND,
                "BLOB_UPLOAD_UNKNOWN",
                "OCI upload session not found",
                None,
            ))
        })?;

    if name::normalize_repository_name(&session.package_name)
        != name::normalize_repository_name(package_name)
    {
        return Err(auth::with_registry_headers(auth::oci_error_response(
            StatusCode::NOT_FOUND,
            "BLOB_UPLOAD_UNKNOWN",
            "OCI upload session does not belong to the requested repository",
            None,
        )));
    }

    Ok(session)
}

fn digest_for_bytes(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(sha2::Sha256::digest(bytes)))
}

async fn package_readable(
    package: &PackageContext,
    db: &PgPool,
    actor_user_id: Option<Uuid>,
) -> bool {
    can_read_package(
        db,
        &package.package_visibility,
        &package.repository_visibility,
        package.package_owner_user_id,
        package.package_owner_org_id,
        package.repository_owner_user_id,
        package.repository_owner_org_id,
        actor_user_id,
    )
    .await
}

async fn has_package_write_access(
    db: &PgPool,
    package_id: Uuid,
    repository_id: Uuid,
    owner_user_id: Option<Uuid>,
    owner_org_id: Option<Uuid>,
    actor_user_id: Uuid,
) -> bool {
    if owner_user_id == Some(actor_user_id) {
        return true;
    }

    if let Some(org_id) = owner_org_id {
        let direct_roles = ORG_PACKAGE_WRITE_ROLES
            .iter()
            .map(|role| (*role).to_owned())
            .collect::<Vec<_>>();
        let direct_access = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (\
                 SELECT 1 FROM org_memberships \
                 WHERE org_id = $1 AND user_id = $2 AND role::text = ANY($3)\
             )",
        )
        .bind(org_id)
        .bind(actor_user_id)
        .bind(&direct_roles)
        .fetch_one(db)
        .await
        .unwrap_or(false);
        if direct_access {
            return true;
        }

        let package_permissions = TEAM_PACKAGE_PUBLISH_PERMISSIONS
            .iter()
            .map(|permission| (*permission).to_owned())
            .collect::<Vec<_>>();
        let package_access = sqlx::query_scalar::<_, bool>(
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
        .bind(&package_permissions)
        .fetch_one(db)
        .await
        .unwrap_or(false);
        if package_access {
            return true;
        }

        let repository_permissions = TEAM_REPOSITORY_PUBLISH_PERMISSIONS
            .iter()
            .map(|permission| (*permission).to_owned())
            .collect::<Vec<_>>();
        return sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (\
                 SELECT 1 \
                 FROM team_repository_access tra \
                 JOIN team_memberships tm ON tm.team_id = tra.team_id \
                 JOIN teams t ON t.id = tra.team_id \
                 JOIN repositories r ON r.id = tra.repository_id \
                 WHERE tra.repository_id = $1 \
                   AND tm.user_id = $2 \
                   AND t.org_id = r.owner_org_id \
                   AND tra.permission::text = ANY($3)\
             )",
        )
        .bind(repository_id)
        .bind(actor_user_id)
        .bind(&repository_permissions)
        .fetch_one(db)
        .await
        .unwrap_or(false);
    }

    false
}

async fn has_package_admin_access(
    db: &PgPool,
    package_id: Uuid,
    repository_id: Uuid,
    owner_user_id: Option<Uuid>,
    owner_org_id: Option<Uuid>,
    actor_user_id: Uuid,
) -> bool {
    if owner_user_id == Some(actor_user_id) {
        return true;
    }

    if let Some(org_id) = owner_org_id {
        let direct_roles = ORG_PACKAGE_ADMIN_ROLES
            .iter()
            .map(|role| (*role).to_owned())
            .collect::<Vec<_>>();
        let direct_access = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (\
                 SELECT 1 FROM org_memberships \
                 WHERE org_id = $1 AND user_id = $2 AND role::text = ANY($3)\
             )",
        )
        .bind(org_id)
        .bind(actor_user_id)
        .bind(&direct_roles)
        .fetch_one(db)
        .await
        .unwrap_or(false);
        if direct_access {
            return true;
        }

        let package_permissions = TEAM_PACKAGE_ADMIN_PERMISSIONS
            .iter()
            .map(|permission| (*permission).to_owned())
            .collect::<Vec<_>>();
        let package_access = sqlx::query_scalar::<_, bool>(
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
        .bind(&package_permissions)
        .fetch_one(db)
        .await
        .unwrap_or(false);
        if package_access {
            return true;
        }

        let repository_permissions = TEAM_REPOSITORY_ADMIN_PERMISSIONS
            .iter()
            .map(|permission| (*permission).to_owned())
            .collect::<Vec<_>>();
        return sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (\
                 SELECT 1 \
                 FROM team_repository_access tra \
                 JOIN team_memberships tm ON tm.team_id = tra.team_id \
                 JOIN teams t ON t.id = tra.team_id \
                 JOIN repositories r ON r.id = tra.repository_id \
                 WHERE tra.repository_id = $1 \
                   AND tm.user_id = $2 \
                   AND t.org_id = r.owner_org_id \
                   AND tra.permission::text = ANY($3)\
             )",
        )
        .bind(repository_id)
        .bind(actor_user_id)
        .bind(&repository_permissions)
        .fetch_one(db)
        .await
        .unwrap_or(false);
    }

    false
}

#[allow(clippy::too_many_arguments)]
async fn can_read_package(
    db: &PgPool,
    package_visibility: &str,
    repository_visibility: &str,
    package_owner_user_id: Option<Uuid>,
    package_owner_org_id: Option<Uuid>,
    repository_owner_user_id: Option<Uuid>,
    repository_owner_org_id: Option<Uuid>,
    actor_user_id: Option<Uuid>,
) -> bool {
    let package_anonymous = matches!(package_visibility, "public" | "unlisted");
    let repository_anonymous = matches!(repository_visibility, "public" | "unlisted");
    if package_anonymous && repository_anonymous {
        return true;
    }

    let Some(actor_user_id) = actor_user_id else {
        return false;
    };
    let package_access = is_owner_or_member(
        db,
        package_owner_user_id,
        package_owner_org_id,
        actor_user_id,
    )
    .await;
    let repository_access = is_owner_or_member(
        db,
        repository_owner_user_id,
        repository_owner_org_id,
        actor_user_id,
    )
    .await;

    (package_anonymous || package_access) && (repository_anonymous || repository_access)
}

async fn is_owner_or_member(
    db: &PgPool,
    owner_user_id: Option<Uuid>,
    owner_org_id: Option<Uuid>,
    actor_user_id: Uuid,
) -> bool {
    if owner_user_id == Some(actor_user_id) {
        return true;
    }

    if let Some(org_id) = owner_org_id {
        return sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM org_memberships WHERE org_id = $1 AND user_id = $2)",
        )
        .bind(org_id)
        .bind(actor_user_id)
        .fetch_one(db)
        .await
        .unwrap_or(false);
    }

    false
}

fn parse_tags_path(path: &str) -> Option<String> {
    path.strip_suffix("/tags/list")
        .map(|name| name.trim_matches('/').to_owned())
        .filter(|name| !name.is_empty())
}

fn parse_manifest_path(path: &str) -> Option<(String, String)> {
    let (name, reference) = path.rsplit_once("/manifests/")?;
    let name = name.trim_matches('/');
    let reference = reference.trim_matches('/');
    if name.is_empty() || reference.is_empty() {
        return None;
    }
    Some((name.to_owned(), reference.to_owned()))
}

fn parse_upload_start_path(path: &str) -> Option<String> {
    path.strip_suffix("/blobs/uploads/")
        .map(|name| name.trim_matches('/').to_owned())
        .filter(|name| !name.is_empty())
}

fn parse_upload_session_path(path: &str) -> Option<(String, Uuid)> {
    let (name, session_id) = path.rsplit_once("/blobs/uploads/")?;
    let name = name.trim_matches('/');
    let session_id = session_id.trim_matches('/');
    if name.is_empty() || session_id.is_empty() {
        return None;
    }
    let session_id = Uuid::parse_str(session_id).ok()?;
    Some((name.to_owned(), session_id))
}

fn parse_blob_path(path: &str) -> Option<(String, String)> {
    if path.contains("/blobs/uploads/") {
        return None;
    }
    let (name, digest) = path.rsplit_once("/blobs/")?;
    let name = name.trim_matches('/');
    let digest = digest.trim_matches('/');
    if name.is_empty() || digest.is_empty() {
        return None;
    }
    Some((name.to_owned(), digest.to_owned()))
}

fn parse_referrers_path(path: &str) -> Option<(String, String)> {
    let (name, digest) = path.rsplit_once("/referrers/")?;
    let name = name.trim_matches('/');
    let digest = digest.trim_matches('/');
    if name.is_empty() || digest.is_empty() {
        return None;
    }

    Some((name.to_owned(), digest.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_manifest_paths() {
        let parsed =
            parse_manifest_path("acme/widget/manifests/latest").expect("path should parse");
        assert_eq!(parsed.0, "acme/widget");
        assert_eq!(parsed.1, "latest");
    }

    #[test]
    fn parses_upload_session_paths() {
        let session_id = Uuid::new_v4();
        let path = format!("acme/widget/blobs/uploads/{session_id}");
        let parsed = parse_upload_session_path(&path).expect("path should parse");
        assert_eq!(parsed.0, "acme/widget");
        assert_eq!(parsed.1, session_id);
    }

    #[test]
    fn parses_referrers_paths() {
        let parsed = parse_referrers_path(
            "acme/widget/referrers/sha256:1111111111111111111111111111111111111111111111111111111111111111",
        )
        .expect("path should parse");
        assert_eq!(parsed.0, "acme/widget");
        assert_eq!(
            parsed.1,
            "sha256:1111111111111111111111111111111111111111111111111111111111111111"
        );
    }
}
