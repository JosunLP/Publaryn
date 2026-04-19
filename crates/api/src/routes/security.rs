use axum::{
    extract::{Path, Query, State},
    routing::{get, patch},
    Json, Router,
};
use serde::Deserialize;
use sqlx::Row;
use uuid::Uuid;

use publaryn_core::{domain::package::normalize_package_name, error::Error};

use crate::{
    error::{ApiError, ApiResult},
    request_auth::{
        ensure_package_read_access, ensure_package_security_review_access, AuthenticatedIdentity,
        OptionalAuthenticatedIdentity,
    },
    routes::parse_ecosystem,
    scopes::{ensure_scope, SCOPE_PACKAGES_WRITE},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/v1/packages/{ecosystem}/{name}/security-findings",
            get(list_security_findings),
        )
        .route(
            "/v1/packages/{ecosystem}/{name}/security-findings/{finding_id}",
            patch(update_security_finding),
        )
}

#[derive(Debug, Deserialize)]
struct SecurityQuery {
    include_resolved: Option<bool>,
}

async fn list_security_findings(
    State(state): State<AppState>,
    identity: OptionalAuthenticatedIdentity,
    Path((ecosystem_str, name)): Path<(String, String)>,
    Query(query): Query<SecurityQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let ecosystem = parse_ecosystem(&ecosystem_str)?;
    let normalized_name = normalize_package_name(&name, &ecosystem);
    let include_resolved = query.include_resolved.unwrap_or(false);
    let package_id = ensure_package_read_access(
        &state.db,
        ecosystem.as_str(),
        &normalized_name,
        identity.user_id(),
    )
    .await?;

    let rows = sqlx::query(
        "SELECT sf.id, sf.kind, sf.severity, sf.title, sf.description, sf.advisory_id, \
                sf.is_resolved, sf.resolved_at, sf.resolved_by, sf.detected_at, \
                r.version, a.filename \
         FROM security_findings sf \
         JOIN releases r ON r.id = sf.release_id \
         LEFT JOIN artifacts a ON a.id = sf.artifact_id \
         WHERE r.package_id = $1 \
           AND ($2::bool = true OR sf.is_resolved = false) \
         ORDER BY sf.detected_at DESC",
    )
    .bind(package_id)
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

#[derive(Debug, Deserialize)]
struct UpdateSecurityFindingRequest {
    is_resolved: bool,
    #[serde(default)]
    note: Option<String>,
}

async fn update_security_finding(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path((ecosystem_str, name, finding_id)): Path<(String, String, Uuid)>,
    Json(payload): Json<UpdateSecurityFindingRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_PACKAGES_WRITE)?;

    let ecosystem = parse_ecosystem(&ecosystem_str)?;
    let normalized_name = normalize_package_name(&name, &ecosystem);
    let package_id = ensure_package_security_review_access(
        &state.db,
        ecosystem.as_str(),
        &normalized_name,
        identity.user_id,
    )
    .await?;

    let note = payload
        .note
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| value.to_owned());
    if let Some(note) = note.as_ref() {
        if note.chars().count() > 2000 {
            return Err(ApiError(Error::Validation(
                "Security finding note must be 2000 characters or fewer".into(),
            )));
        }
    }

    let existing = sqlx::query(
        "SELECT sf.id, sf.is_resolved, sf.release_id, r.package_id, r.version \
         FROM security_findings sf \
         JOIN releases r ON r.id = sf.release_id \
         WHERE sf.id = $1",
    )
    .bind(finding_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| ApiError(Error::NotFound("Security finding not found".into())))?;

    let existing_package_id: Uuid = existing
        .try_get("package_id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    if existing_package_id != package_id {
        return Err(ApiError(Error::NotFound(
            "Security finding not found".into(),
        )));
    }

    let was_resolved: bool = existing
        .try_get("is_resolved")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let release_id: Uuid = existing
        .try_get("release_id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let release_version: String = existing
        .try_get("version")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    let updated_row = if payload.is_resolved {
        sqlx::query(
            "UPDATE security_findings \
             SET is_resolved = TRUE, \
                 resolved_at = NOW(), \
                 resolved_by = $1 \
             WHERE id = $2 \
             RETURNING id, kind, severity, title, description, advisory_id, is_resolved, \
                       resolved_at, resolved_by, detected_at, release_id, artifact_id",
        )
        .bind(identity.user_id)
        .bind(finding_id)
        .fetch_one(&mut *tx)
        .await
    } else {
        sqlx::query(
            "UPDATE security_findings \
             SET is_resolved = FALSE, \
                 resolved_at = NULL, \
                 resolved_by = NULL \
             WHERE id = $1 \
             RETURNING id, kind, severity, title, description, advisory_id, is_resolved, \
                       resolved_at, resolved_by, detected_at, release_id, artifact_id",
        )
        .bind(finding_id)
        .fetch_one(&mut *tx)
        .await
    }
    .map_err(|e| ApiError(Error::Database(e)))?;

    if payload.is_resolved != was_resolved {
        let action = if payload.is_resolved {
            "security_finding_resolve"
        } else {
            "security_finding_reopen"
        };
        let metadata = serde_json::json!({
            "ecosystem": ecosystem.as_str(),
            "package_name": name,
            "release_version": release_version,
            "finding_id": finding_id,
            "previous_is_resolved": was_resolved,
            "is_resolved": payload.is_resolved,
            "note": note,
        });
        sqlx::query(
            "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, \
                                      target_package_id, target_release_id, metadata, occurred_at) \
             VALUES ($1, $2::audit_action, $3, $4, $5, $6, $7, NOW())",
        )
        .bind(Uuid::new_v4())
        .bind(action)
        .bind(identity.user_id)
        .bind(identity.audit_actor_token_id())
        .bind(package_id)
        .bind(release_id)
        .bind(&metadata)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;
    }

    tx.commit()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    let artifact_filename: Option<String> = if let Some(artifact_id) = updated_row
        .try_get::<Option<Uuid>, _>("artifact_id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?
    {
        sqlx::query_scalar::<_, String>("SELECT filename FROM artifacts WHERE id = $1")
            .bind(artifact_id)
            .fetch_optional(&state.db)
            .await
            .map_err(|e| ApiError(Error::Database(e)))?
    } else {
        None
    };

    Ok(Json(serde_json::json!({
        "id": updated_row.try_get::<Uuid, _>("id").ok(),
        "kind": updated_row.try_get::<String, _>("kind").ok(),
        "severity": updated_row.try_get::<String, _>("severity").ok(),
        "title": updated_row.try_get::<String, _>("title").ok(),
        "description": updated_row.try_get::<Option<String>, _>("description").ok().flatten(),
        "advisory_id": updated_row.try_get::<Option<String>, _>("advisory_id").ok().flatten(),
        "is_resolved": updated_row.try_get::<bool, _>("is_resolved").ok(),
        "resolved_at": updated_row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("resolved_at").ok().flatten(),
        "resolved_by": updated_row.try_get::<Option<Uuid>, _>("resolved_by").ok().flatten(),
        "detected_at": updated_row.try_get::<chrono::DateTime<chrono::Utc>, _>("detected_at").ok(),
        "release_version": release_version,
        "artifact_filename": artifact_filename,
    })))
}
