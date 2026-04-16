use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use sqlx::{Postgres, QueryBuilder, Row};
use uuid::Uuid;

use publaryn_core::error::Error;

use crate::{
    error::{ApiError, ApiResult},
    request_auth::{ensure_platform_admin, AuthenticatedIdentity},
    scopes::{ensure_scope, SCOPE_AUDIT_READ},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/audit", get(list_audit_logs))
}

#[derive(Debug, Deserialize)]
struct AuditQuery {
    action: Option<String>,
    actor_user_id: Option<Uuid>,
    target_org_id: Option<Uuid>,
    target_package_id: Option<Uuid>,
    page: Option<u32>,
    per_page: Option<u32>,
}

async fn list_audit_logs(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Query(query): Query<AuditQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_AUDIT_READ)?;
    ensure_platform_admin(&state.db, identity.user_id).await?;

    let limit = query.per_page.unwrap_or(50).min(100) as i64;
    let offset = ((query.page.unwrap_or(1).saturating_sub(1)) as i64) * limit;

    let mut builder = QueryBuilder::<Postgres>::new(
        "SELECT id, action, actor_user_id, actor_token_id, target_user_id, target_org_id, \
                target_package_id, target_release_id, ip_address, user_agent, metadata, occurred_at \
         FROM audit_logs WHERE 1 = 1",
    );

    if let Some(action) = query.action.as_deref() {
        builder.push(" AND action = ").push_bind(action);
    }
    if let Some(actor_user_id) = query.actor_user_id {
        builder
            .push(" AND actor_user_id = ")
            .push_bind(actor_user_id);
    }
    if let Some(target_org_id) = query.target_org_id {
        builder
            .push(" AND target_org_id = ")
            .push_bind(target_org_id);
    }
    if let Some(target_package_id) = query.target_package_id {
        builder
            .push(" AND target_package_id = ")
            .push_bind(target_package_id);
    }

    builder
        .push(" ORDER BY occurred_at DESC LIMIT ")
        .push_bind(limit)
        .push(" OFFSET ")
        .push_bind(offset);

    let rows = builder
        .build()
        .fetch_all(&state.db)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    let logs: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            serde_json::json!({
                "id": row.try_get::<Uuid, _>("id").ok(),
                "action": row.try_get::<String, _>("action").ok(),
                "actor_user_id": row.try_get::<Option<Uuid>, _>("actor_user_id").ok().flatten(),
                "actor_token_id": row.try_get::<Option<Uuid>, _>("actor_token_id").ok().flatten(),
                "target_user_id": row.try_get::<Option<Uuid>, _>("target_user_id").ok().flatten(),
                "target_org_id": row.try_get::<Option<Uuid>, _>("target_org_id").ok().flatten(),
                "target_package_id": row.try_get::<Option<Uuid>, _>("target_package_id").ok().flatten(),
                "target_release_id": row.try_get::<Option<Uuid>, _>("target_release_id").ok().flatten(),
                "ip_address": row.try_get::<Option<String>, _>("ip_address").ok().flatten(),
                "user_agent": row.try_get::<Option<String>, _>("user_agent").ok().flatten(),
                "metadata": row.try_get::<Option<serde_json::Value>, _>("metadata").ok().flatten(),
                "occurred_at": row.try_get::<chrono::DateTime<chrono::Utc>, _>("occurred_at").ok(),
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "page": query.page.unwrap_or(1),
        "per_page": limit,
        "logs": logs,
    })))
}
