use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{delete, get, post},
    Json, Router,
};
use serde::Deserialize;
use sqlx::{Postgres, QueryBuilder, Row};
use uuid::Uuid;

use publaryn_core::{domain::namespace::NamespaceClaim, error::Error};

use crate::{
    error::{ApiError, ApiResult},
    request_auth::{ensure_org_admin_by_id, AuthenticatedIdentity},
    routes::parse_ecosystem,
    scopes::{ensure_scope, SCOPE_NAMESPACES_WRITE},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/namespaces", get(list_namespaces))
        .route("/v1/namespaces", post(create_namespace))
        .route("/v1/namespaces/{claim_id}", delete(delete_namespace))
        .route("/v1/namespaces/lookup", get(lookup_namespace))
}

#[derive(Debug, Deserialize)]
struct CreateNamespaceRequest {
    ecosystem: String,
    namespace: String,
    owner_user_id: Option<Uuid>,
    owner_org_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
struct NamespaceListQuery {
    ecosystem: Option<String>,
    owner_user_id: Option<Uuid>,
    owner_org_id: Option<Uuid>,
    verified: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct NamespaceLookupQuery {
    ecosystem: String,
    namespace: String,
}

async fn create_namespace(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Json(body): Json<CreateNamespaceRequest>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    ensure_scope(&identity, SCOPE_NAMESPACES_WRITE)?;

    if body.namespace.trim().is_empty() {
        return Err(ApiError(Error::Validation(
            "Namespace must not be empty".into(),
        )));
    }

    if body.owner_user_id.is_some() && body.owner_org_id.is_some() {
        return Err(ApiError(Error::Validation(
            "Namespace claim must have exactly one owner".into(),
        )));
    }

    let ecosystem = parse_ecosystem(&body.ecosystem)?;
    let owner_user_id = match (body.owner_user_id, body.owner_org_id) {
        (Some(owner_user_id), None) if owner_user_id == identity.user_id => Some(owner_user_id),
        (Some(_), None) => {
            return Err(ApiError(Error::Forbidden(
                "You can only create user-owned namespace claims for your own account".into(),
            )));
        }
        (None, Some(owner_org_id)) => {
            ensure_org_admin_by_id(&state.db, owner_org_id, identity.user_id).await?;
            None
        }
        (None, None) => Some(identity.user_id),
        (Some(_), Some(_)) => unreachable!("validated above"),
    };

    let claim = NamespaceClaim {
        id: Uuid::new_v4(),
        ecosystem: ecosystem.clone(),
        namespace: body.namespace,
        owner_user_id,
        owner_org_id: body.owner_org_id,
        is_verified: false,
        created_at: chrono::Utc::now(),
    };

    sqlx::query(
        "INSERT INTO namespace_claims (id, ecosystem, namespace, owner_user_id, owner_org_id, is_verified, created_at) \
            VALUES ($1, $2::ecosystem, $3, $4, $5, $6, $7)",
    )
    .bind(claim.id)
    .bind(claim.ecosystem.as_str())
    .bind(&claim.namespace)
    .bind(claim.owner_user_id)
    .bind(claim.owner_org_id)
    .bind(claim.is_verified)
    .bind(claim.created_at)
    .execute(&state.db)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db) if db.is_unique_violation() => {
            ApiError(Error::AlreadyExists("Namespace claim already exists".into()))
        }
        _ => ApiError(Error::Database(e)),
    })?;

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_user_id, target_org_id, metadata, occurred_at) \
         VALUES ($1, 'namespace_claim_create', $2, $3, $4, $5, $6, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(claim.owner_user_id)
    .bind(claim.owner_org_id)
    .bind(serde_json::json!({
        "ecosystem": claim.ecosystem.as_str(),
        "namespace": &claim.namespace,
    }))
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "id": claim.id,
            "ecosystem": claim.ecosystem.as_str(),
            "namespace": claim.namespace,
            "owner_user_id": claim.owner_user_id,
            "owner_org_id": claim.owner_org_id,
            "is_verified": claim.is_verified,
        })),
    ))
}

async fn list_namespaces(
    State(state): State<AppState>,
    Query(query): Query<NamespaceListQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let mut builder = QueryBuilder::<Postgres>::new(
        "SELECT id, ecosystem::text AS ecosystem, namespace, owner_user_id, owner_org_id, is_verified, created_at \
         FROM namespace_claims WHERE 1 = 1",
    );

    if let Some(ecosystem) = query.ecosystem.as_deref() {
        let ecosystem = parse_ecosystem(ecosystem)?;
        builder
            .push(" AND ecosystem = ")
            .push_bind(ecosystem.as_str())
            .push("::ecosystem");
    }

    if let Some(owner_user_id) = query.owner_user_id {
        builder
            .push(" AND owner_user_id = ")
            .push_bind(owner_user_id);
    }

    if let Some(owner_org_id) = query.owner_org_id {
        builder.push(" AND owner_org_id = ").push_bind(owner_org_id);
    }

    if let Some(verified) = query.verified {
        builder.push(" AND is_verified = ").push_bind(verified);
    }

    builder.push(" ORDER BY created_at DESC");

    let rows = builder
        .build()
        .fetch_all(&state.db)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    let namespaces: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            serde_json::json!({
                "id": row.try_get::<Uuid, _>("id").ok(),
                "ecosystem": row.try_get::<String, _>("ecosystem").ok(),
                "namespace": row.try_get::<String, _>("namespace").ok(),
                "owner_user_id": row.try_get::<Option<Uuid>, _>("owner_user_id").ok().flatten(),
                "owner_org_id": row.try_get::<Option<Uuid>, _>("owner_org_id").ok().flatten(),
                "is_verified": row.try_get::<bool, _>("is_verified").ok(),
                "created_at": row.try_get::<chrono::DateTime<chrono::Utc>, _>("created_at").ok(),
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "namespaces": namespaces })))
}

async fn delete_namespace(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path(claim_id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_NAMESPACES_WRITE)?;

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    let claim_row = sqlx::query(
        "SELECT ecosystem::text AS ecosystem, namespace, owner_user_id, owner_org_id, is_verified \
         FROM namespace_claims \
         WHERE id = $1 \
         FOR UPDATE",
    )
    .bind(claim_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| ApiError(Error::NotFound("Namespace claim not found".into())))?;

    let ecosystem: String = claim_row
        .try_get("ecosystem")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let namespace: String = claim_row
        .try_get("namespace")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let owner_user_id: Option<Uuid> = claim_row
        .try_get("owner_user_id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let owner_org_id: Option<Uuid> = claim_row
        .try_get("owner_org_id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let is_verified: bool = claim_row
        .try_get("is_verified")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    match (owner_user_id, owner_org_id) {
        (Some(owner_user_id), None) if owner_user_id == identity.user_id => {}
        (Some(_), None) => {
            return Err(ApiError(Error::Forbidden(
                "You can only delete user-owned namespace claims for your own account".into(),
            )));
        }
        (None, Some(owner_org_id)) => {
            ensure_org_admin_by_id(&state.db, owner_org_id, identity.user_id).await?;
        }
        (None, None) => {
            return Err(ApiError(Error::Internal(
                "Namespace claim is missing an owner".into(),
            )));
        }
        (Some(_), Some(_)) => {
            return Err(ApiError(Error::Internal(
                "Namespace claim has multiple owners".into(),
            )));
        }
    }

    sqlx::query("DELETE FROM namespace_claims WHERE id = $1")
        .bind(claim_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_user_id, target_org_id, metadata, occurred_at) \
         VALUES ($1, 'namespace_claim_delete', $2, $3, $4, $5, $6, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(owner_user_id)
    .bind(owner_org_id)
    .bind(serde_json::json!({
        "ecosystem": ecosystem,
        "namespace": namespace,
        "namespace_claim_id": claim_id,
        "was_verified": is_verified,
    }))
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    tx.commit()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(Json(
        serde_json::json!({ "message": "Namespace claim deleted" }),
    ))
}

async fn lookup_namespace(
    State(state): State<AppState>,
    Query(query): Query<NamespaceLookupQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let ecosystem = parse_ecosystem(&query.ecosystem)?;

    let row = sqlx::query(
        "SELECT id, ecosystem::text AS ecosystem, namespace, owner_user_id, owner_org_id, is_verified, created_at \
            FROM namespace_claims WHERE ecosystem = $1::ecosystem AND namespace = $2",
    )
    .bind(ecosystem.as_str())
    .bind(&query.namespace)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| {
        ApiError(Error::NotFound(format!(
            "Namespace '{}' not found for ecosystem '{}'",
            query.namespace, query.ecosystem
        )))
    })?;

    Ok(Json(serde_json::json!({
        "id": row.try_get::<Uuid, _>("id").ok(),
        "ecosystem": row.try_get::<String, _>("ecosystem").ok(),
        "namespace": row.try_get::<String, _>("namespace").ok(),
        "owner_user_id": row.try_get::<Option<Uuid>, _>("owner_user_id").ok().flatten(),
        "owner_org_id": row.try_get::<Option<Uuid>, _>("owner_org_id").ok().flatten(),
        "is_verified": row.try_get::<bool, _>("is_verified").ok(),
        "created_at": row.try_get::<chrono::DateTime<chrono::Utc>, _>("created_at").ok(),
    })))
}
