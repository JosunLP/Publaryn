use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use sqlx::Row;
use uuid::Uuid;

use publaryn_core::{
    domain::{package::normalize_package_name, trusted_publisher::TrustedPublisher},
    error::Error,
};

use crate::{
    error::{ApiError, ApiResult},
    routes::parse_ecosystem,
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/v1/packages/:ecosystem/:name/trusted-publishers",
            get(list_trusted_publishers),
        )
        .route(
            "/v1/packages/:ecosystem/:name/trusted-publishers",
            post(create_trusted_publisher),
        )
}

#[derive(Debug, Deserialize)]
struct CreateTrustedPublisherRequest {
    issuer: String,
    subject: String,
    repository: Option<String>,
    workflow_ref: Option<String>,
    environment: Option<String>,
    created_by: Uuid,
}

async fn list_trusted_publishers(
    State(state): State<AppState>,
    Path((ecosystem_str, name)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let ecosystem = parse_ecosystem(&ecosystem_str)?;
    let normalized_name = normalize_package_name(&name, &ecosystem);

    let rows = sqlx::query(
        "SELECT tp.id, tp.issuer, tp.subject, tp.repository, tp.workflow_ref, tp.environment, \
                tp.created_by, tp.created_at \
         FROM trusted_publishers tp \
         JOIN packages p ON p.id = tp.package_id \
         WHERE p.ecosystem = $1 AND p.normalized_name = $2 \
         ORDER BY tp.created_at DESC",
    )
    .bind(ecosystem.as_str())
    .bind(&normalized_name)
    .fetch_all(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    let publishers: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            serde_json::json!({
                "id": row.try_get::<Uuid, _>("id").ok(),
                "issuer": row.try_get::<String, _>("issuer").ok(),
                "subject": row.try_get::<String, _>("subject").ok(),
                "repository": row.try_get::<Option<String>, _>("repository").ok().flatten(),
                "workflow_ref": row.try_get::<Option<String>, _>("workflow_ref").ok().flatten(),
                "environment": row.try_get::<Option<String>, _>("environment").ok().flatten(),
                "created_by": row.try_get::<Uuid, _>("created_by").ok(),
                "created_at": row.try_get::<chrono::DateTime<chrono::Utc>, _>("created_at").ok(),
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "trusted_publishers": publishers })))
}

async fn create_trusted_publisher(
    State(state): State<AppState>,
    Path((ecosystem_str, name)): Path<(String, String)>,
    Json(body): Json<CreateTrustedPublisherRequest>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    let ecosystem = parse_ecosystem(&ecosystem_str)?;
    let normalized_name = normalize_package_name(&name, &ecosystem);

    let package_row = sqlx::query(
        "SELECT id FROM packages WHERE ecosystem = $1 AND normalized_name = $2",
    )
    .bind(ecosystem.as_str())
    .bind(&normalized_name)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| ApiError(Error::NotFound(format!("Package '{name}' not found"))))?;

    let package_id: Uuid = package_row
        .try_get("id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    let mut publisher = TrustedPublisher::new(package_id, body.issuer, body.subject, body.created_by);
    publisher.repository = body.repository;
    publisher.workflow_ref = body.workflow_ref;
    publisher.environment = body.environment;

    sqlx::query(
        "INSERT INTO trusted_publishers (id, package_id, issuer, subject, repository, workflow_ref, \
         environment, created_by, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(publisher.id)
    .bind(publisher.package_id)
    .bind(&publisher.issuer)
    .bind(&publisher.subject)
    .bind(&publisher.repository)
    .bind(&publisher.workflow_ref)
    .bind(&publisher.environment)
    .bind(publisher.created_by)
    .bind(publisher.created_at)
    .execute(&state.db)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db) if db.is_unique_violation() => {
            ApiError(Error::AlreadyExists("Trusted publisher already exists".into()))
        }
        _ => ApiError(Error::Database(e)),
    })?;

    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "id": publisher.id,
            "issuer": publisher.issuer,
            "subject": publisher.subject,
            "repository": publisher.repository,
            "workflow_ref": publisher.workflow_ref,
            "environment": publisher.environment,
            "created_by": publisher.created_by,
        })),
    ))
}
