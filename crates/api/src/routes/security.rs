use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use sqlx::Row;
use uuid::Uuid;

use publaryn_core::{
    domain::package::normalize_package_name,
    error::Error,
};

use crate::{
    error::{ApiError, ApiResult},
    routes::parse_ecosystem,
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/v1/packages/:ecosystem/:name/security-findings",
        get(list_security_findings),
    )
}

#[derive(Debug, Deserialize)]
struct SecurityQuery {
    include_resolved: Option<bool>,
}

async fn list_security_findings(
    State(state): State<AppState>,
    Path((ecosystem_str, name)): Path<(String, String)>,
    Query(query): Query<SecurityQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let ecosystem = parse_ecosystem(&ecosystem_str)?;
    let normalized_name = normalize_package_name(&name, &ecosystem);
    let include_resolved = query.include_resolved.unwrap_or(false);

    let rows = sqlx::query(
        "SELECT sf.id, sf.kind, sf.severity, sf.title, sf.description, sf.advisory_id, \
                sf.is_resolved, sf.resolved_at, sf.resolved_by, sf.detected_at, \
                r.version, a.filename \
         FROM security_findings sf \
         JOIN releases r ON r.id = sf.release_id \
         JOIN packages p ON p.id = r.package_id \
         LEFT JOIN artifacts a ON a.id = sf.artifact_id \
         WHERE p.ecosystem = $1 AND p.normalized_name = $2 \
           AND ($3::bool = true OR sf.is_resolved = false) \
         ORDER BY sf.detected_at DESC",
    )
    .bind(ecosystem.as_str())
    .bind(&normalized_name)
    .bind(include_resolved)
    .fetch_all(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    let findings: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            serde_json::json!({
                "id": row.try_get::<Uuid, _>("id").ok(),
                "kind": row.try_get::<String, _>("kind").ok(),
                "severity": row.try_get::<String, _>("severity").ok(),
                "title": row.try_get::<String, _>("title").ok(),
                "description": row.try_get::<Option<String>, _>("description").ok().flatten(),
                "advisory_id": row.try_get::<Option<String>, _>("advisory_id").ok().flatten(),
                "is_resolved": row.try_get::<bool, _>("is_resolved").ok(),
                "resolved_at": row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("resolved_at").ok().flatten(),
                "resolved_by": row.try_get::<Option<Uuid>, _>("resolved_by").ok().flatten(),
                "detected_at": row.try_get::<chrono::DateTime<chrono::Utc>, _>("detected_at").ok(),
                "release_version": row.try_get::<String, _>("version").ok(),
                "artifact_filename": row.try_get::<Option<String>, _>("filename").ok().flatten(),
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "findings": findings })))
}
