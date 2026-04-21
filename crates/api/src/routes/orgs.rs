use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::IntoResponse,
    routing::{delete, get, patch, post, put},
    Json, Router,
};
use csv::WriterBuilder;
use serde::Deserialize;
use sqlx::{Postgres, QueryBuilder, Row};
use std::collections::{BTreeSet, HashMap};
use std::str::FromStr;
use uuid::Uuid;

use publaryn_core::{
    domain::{
        organization::{OrgRole, Organization},
        package::normalize_package_name,
        team::TeamPermission,
    },
    error::Error,
    validation,
};

use crate::{
    error::{ApiError, ApiResult},
    request_auth::{
        actor_can_access_org_member_directory_by_id,
        actor_can_transfer_package_by_id, actor_can_transfer_repository_by_id,
        ensure_org_admin_by_slug, ensure_org_audit_access_by_slug, ensure_org_member_by_slug,
        AuthenticatedIdentity, OptionalAuthenticatedIdentity,
    },
    scopes::{ensure_scope, SCOPE_ORGS_TRANSFER, SCOPE_ORGS_WRITE, SCOPE_PACKAGES_WRITE},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/orgs", post(create_org))
        .route("/v1/orgs/{slug}", get(get_org))
        .route("/v1/orgs/{slug}", patch(update_org))
        .route("/v1/orgs/{slug}/audit", get(list_org_audit_logs))
        .route(
            "/v1/orgs/{slug}/audit/export",
            get(export_org_audit_logs_csv),
        )
        .route("/v1/orgs/{slug}/members", get(list_members))
        .route("/v1/orgs/{slug}/members/search", get(search_org_members))
        .route("/v1/orgs/{slug}/members", post(add_member))
        .route("/v1/orgs/{slug}/members/{username}", delete(remove_member))
        .route(
            "/v1/orgs/{slug}/ownership-transfer",
            post(transfer_ownership),
        )
        .route("/v1/orgs/{slug}/teams", get(list_teams))
        .route("/v1/orgs/{slug}/teams", post(create_team))
        .route("/v1/orgs/{slug}/teams/{team_slug}", patch(update_team))
        .route("/v1/orgs/{slug}/teams/{team_slug}", delete(delete_team))
        .route(
            "/v1/orgs/{slug}/teams/{team_slug}/members",
            get(list_team_members).post(add_team_member),
        )
        .route(
            "/v1/orgs/{slug}/teams/{team_slug}/members/{username}",
            delete(remove_team_member),
        )
        .route(
            "/v1/orgs/{slug}/teams/{team_slug}/package-access",
            get(list_team_package_access),
        )
        .route(
            "/v1/orgs/{slug}/teams/{team_slug}/package-access/{ecosystem}/{name}",
            put(replace_team_package_access).delete(remove_team_package_access),
        )
        .route(
            "/v1/orgs/{slug}/teams/{team_slug}/repository-access",
            get(list_team_repository_access),
        )
        .route(
            "/v1/orgs/{slug}/teams/{team_slug}/repository-access/{repository_slug}",
            put(replace_team_repository_access).delete(remove_team_repository_access),
        )
        .route(
            "/v1/orgs/{slug}/teams/{team_slug}/namespace-access",
            get(list_team_namespace_access),
        )
        .route(
            "/v1/orgs/{slug}/teams/{team_slug}/namespace-access/{claim_id}",
            put(replace_team_namespace_access).delete(remove_team_namespace_access),
        )
        .route("/v1/orgs/{slug}/repositories", get(list_org_repositories))
        .route(
            "/v1/orgs/{slug}/security-findings",
            get(list_org_security_findings),
        )
        .route(
            "/v1/orgs/{slug}/security-findings/export",
            get(export_org_security_findings_csv),
        )
        .route("/v1/orgs/{slug}/packages", get(list_org_packages))
}

#[derive(Debug, Deserialize)]
struct CreateOrgRequest {
    name: String,
    slug: String,
    description: Option<String>,
    website: Option<String>,
    email: Option<String>,
}

async fn create_org(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Json(body): Json<CreateOrgRequest>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    ensure_scope(&identity, SCOPE_ORGS_WRITE)?;

    validation::validate_slug(&body.slug).map_err(ApiError::from)?;
    let org = Organization::new(body.name, body.slug);

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query(
        "INSERT INTO organizations (id, name, slug, description, website, email, \
         is_verified, mfa_required, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, false, false, $7, $7)",
    )
    .bind(org.id)
    .bind(&org.name)
    .bind(&org.slug)
    .bind(&body.description)
    .bind(&body.website)
    .bind(&body.email)
    .bind(org.created_at)
    .execute(&mut *tx)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db) if db.is_unique_violation() => ApiError(Error::AlreadyExists(
            "Organization slug already taken".into(),
        )),
        _ => ApiError(Error::Database(e)),
    })?;

    sqlx::query(
        "INSERT INTO org_memberships (id, org_id, user_id, role, invited_by, joined_at) \
         VALUES ($1, $2, $3, 'owner', NULL, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(org.id)
    .bind(identity.user_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_org_id, metadata, occurred_at) \
         VALUES ($1, 'org_create', $2, $3, $4, $5, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(org.id)
    .bind(serde_json::json!({ "name": &org.name, "slug": &org.slug }))
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    tx.commit()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "id": org.id, "slug": org.slug })),
    ))
}

async fn get_org(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let row = sqlx::query(
        "SELECT id, name, slug, description, website, email, is_verified, mfa_required, created_at \
         FROM organizations WHERE slug = $1",
    )
    .bind(&slug)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| ApiError(Error::NotFound(format!("Organization '{slug}' not found"))))?;

    Ok(Json(serde_json::json!({
        "id": row.try_get::<Uuid, _>("id").ok(),
        "name": row.try_get::<String, _>("name").ok(),
        "slug": row.try_get::<String, _>("slug").ok(),
        "description": row.try_get::<Option<String>, _>("description").ok().flatten(),
        "website": row.try_get::<Option<String>, _>("website").ok().flatten(),
        "email": row.try_get::<Option<String>, _>("email").ok().flatten(),
        "is_verified": row.try_get::<bool, _>("is_verified").ok(),
        "mfa_required": row.try_get::<bool, _>("mfa_required").ok(),
        "created_at": row.try_get::<chrono::DateTime<chrono::Utc>, _>("created_at").ok(),
    })))
}

#[derive(Debug, Deserialize)]
struct UpdateOrgRequest {
    description: Option<Option<String>>,
    website: Option<Option<String>>,
    email: Option<Option<String>>,
    mfa_required: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedOrgProfileUpdate {
    description: Option<String>,
    website: Option<String>,
    email: Option<String>,
    mfa_required: bool,
}

#[derive(Debug, Deserialize)]
struct OrgAuditQuery {
    action: Option<String>,
    actor_user_id: Option<Uuid>,
    occurred_from: Option<String>,
    occurred_until: Option<String>,
    page: Option<u32>,
    per_page: Option<u32>,
}

#[derive(Debug, Clone)]
struct ResolvedOrgAuditFilters {
    action: Option<String>,
    actor_user_id: Option<Uuid>,
    occurred_from: Option<chrono::NaiveDate>,
    occurred_until: Option<chrono::NaiveDate>,
}

#[derive(Debug, Deserialize)]
struct OrgSecurityFindingsQuery {
    severity: Option<String>,
    ecosystem: Option<String>,
    package: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct ResolvedOrgSecurityFilters {
    severities: Vec<String>,
    ecosystem: Option<String>,
    package: Option<String>,
}

async fn update_org(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path(slug): Path<String>,
    Json(body): Json<UpdateOrgRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_ORGS_WRITE)?;
    let org_id = ensure_org_admin_by_slug(&state.db, &slug, identity.user_id).await?;

    let current_org = sqlx::query(
        "SELECT name, description, website, email, mfa_required \
         FROM organizations \
         WHERE id = $1",
    )
    .bind(org_id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    let org_name: String = current_org
        .try_get("name")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let current_description = current_org
        .try_get::<Option<String>, _>("description")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let current_website = current_org
        .try_get::<Option<String>, _>("website")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let current_email = current_org
        .try_get::<Option<String>, _>("email")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let current_mfa_required = current_org
        .try_get::<bool, _>("mfa_required")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    let current_profile = ResolvedOrgProfileUpdate {
        description: current_description,
        website: current_website,
        email: current_email,
        mfa_required: current_mfa_required,
    };
    let updated_profile = resolve_org_profile_update(&current_profile, &body);
    let changes = collect_org_profile_changes(&current_profile, &updated_profile);

    if changes.is_empty() {
        return Ok(Json(
            serde_json::json!({ "message": "Organization updated" }),
        ));
    }

    let changed_fields = changes.keys().cloned().collect::<Vec<_>>();

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query(
        "UPDATE organizations \
         SET description = $1, \
              website     = $2, \
              email       = $3, \
              mfa_required = $4, \
              updated_at   = NOW() \
         WHERE id = $5",
    )
    .bind(&updated_profile.description)
    .bind(&updated_profile.website)
    .bind(&updated_profile.email)
    .bind(updated_profile.mfa_required)
    .bind(org_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_org_id, metadata, occurred_at) \
         VALUES ($1, 'org_update', $2, $3, $4, $5, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(org_id)
    .bind(serde_json::json!({
        "org_slug": slug,
        "org_name": org_name,
        "changed_fields": changed_fields,
        "changes": changes,
    }))
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    tx.commit()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(Json(
        serde_json::json!({ "message": "Organization updated" }),
    ))
}

fn normalize_optional_org_field(value: Option<String>) -> Option<String> {
    value.and_then(|raw_value| {
        let trimmed = raw_value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_owned())
        }
    })
}

fn resolve_org_profile_update(
    current: &ResolvedOrgProfileUpdate,
    body: &UpdateOrgRequest,
) -> ResolvedOrgProfileUpdate {
    ResolvedOrgProfileUpdate {
        description: body
            .description
            .clone()
            .map(normalize_optional_org_field)
            .unwrap_or_else(|| current.description.clone()),
        website: body
            .website
            .clone()
            .map(normalize_optional_org_field)
            .unwrap_or_else(|| current.website.clone()),
        email: body
            .email
            .clone()
            .map(normalize_optional_org_field)
            .unwrap_or_else(|| current.email.clone()),
        mfa_required: body.mfa_required.unwrap_or(current.mfa_required),
    }
}

fn collect_org_profile_changes(
    current: &ResolvedOrgProfileUpdate,
    updated: &ResolvedOrgProfileUpdate,
) -> serde_json::Map<String, serde_json::Value> {
    let mut changes = serde_json::Map::new();

    if current.description != updated.description {
        changes.insert(
            "description".into(),
            serde_json::json!({
                "before": current.description,
                "after": updated.description,
            }),
        );
    }

    if current.website != updated.website {
        changes.insert(
            "website".into(),
            serde_json::json!({
                "before": current.website,
                "after": updated.website,
            }),
        );
    }

    if current.email != updated.email {
        changes.insert(
            "email".into(),
            serde_json::json!({
                "before": current.email,
                "after": updated.email,
            }),
        );
    }

    if current.mfa_required != updated.mfa_required {
        changes.insert(
            "mfa_required".into(),
            serde_json::json!({
                "before": current.mfa_required,
                "after": updated.mfa_required,
            }),
        );
    }

    changes
}

async fn list_org_audit_logs(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path(slug): Path<String>,
    Query(query): Query<OrgAuditQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_ORGS_WRITE)?;

    let org_id = ensure_org_audit_access_by_slug(&state.db, &slug, identity.user_id).await?;
    let filters = resolve_org_audit_filters(&query)?;
    let page = query.page.unwrap_or(1).max(1);
    let limit = query.per_page.unwrap_or(20).clamp(1, 100) as i64;
    let offset = ((page.saturating_sub(1)) as i64) * limit;
    let fetch_limit = limit + 1;

    let mut builder = build_org_audit_query(org_id);
    apply_org_audit_filters(&mut builder, &filters)?;
    builder
        .push(" ORDER BY al.occurred_at DESC LIMIT ")
        .push_bind(fetch_limit)
        .push(" OFFSET ")
        .push_bind(offset);

    let mut rows = builder
        .build()
        .fetch_all(&state.db)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    let has_next = rows.len() > limit as usize;
    if has_next {
        rows.truncate(limit as usize);
    }

    let logs: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            serde_json::json!({
                "id": row.try_get::<Uuid, _>("id").ok(),
                "action": row.try_get::<String, _>("action").ok(),
                "actor_user_id": row.try_get::<Option<Uuid>, _>("actor_user_id").ok().flatten(),
                "actor_username": row.try_get::<Option<String>, _>("actor_username").ok().flatten(),
                "actor_display_name": row.try_get::<Option<String>, _>("actor_display_name").ok().flatten(),
                "actor_token_id": row.try_get::<Option<Uuid>, _>("actor_token_id").ok().flatten(),
                "target_user_id": row.try_get::<Option<Uuid>, _>("target_user_id").ok().flatten(),
                "target_username": row.try_get::<Option<String>, _>("target_username").ok().flatten(),
                "target_display_name": row.try_get::<Option<String>, _>("target_display_name").ok().flatten(),
                "target_org_id": row.try_get::<Option<Uuid>, _>("target_org_id").ok().flatten(),
                "target_package_id": row.try_get::<Option<Uuid>, _>("target_package_id").ok().flatten(),
                "target_release_id": row.try_get::<Option<Uuid>, _>("target_release_id").ok().flatten(),
                "metadata": row.try_get::<Option<serde_json::Value>, _>("metadata").ok().flatten(),
                "occurred_at": row.try_get::<chrono::DateTime<chrono::Utc>, _>("occurred_at").ok(),
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "page": page,
        "per_page": limit,
        "has_next": has_next,
        "logs": logs,
    })))
}

async fn export_org_audit_logs_csv(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path(slug): Path<String>,
    Query(query): Query<OrgAuditQuery>,
) -> ApiResult<impl IntoResponse> {
    ensure_scope(&identity, SCOPE_ORGS_WRITE)?;

    let org_id = ensure_org_audit_access_by_slug(&state.db, &slug, identity.user_id).await?;
    let filters = resolve_org_audit_filters(&query)?;

    let mut builder = build_org_audit_query(org_id);
    apply_org_audit_filters(&mut builder, &filters)?;
    builder.push(" ORDER BY al.occurred_at DESC");

    let rows = builder
        .build()
        .fetch_all(&state.db)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    let csv_body = build_org_audit_csv(&rows)?;
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/csv; charset=utf-8"),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"org-audit-{slug}.csv\""))
            .map_err(|error| ApiError(Error::Internal(error.to_string())))?,
    );

    Ok((headers, csv_body))
}

fn build_org_audit_query(org_id: Uuid) -> QueryBuilder<'static, Postgres> {
    let mut builder = QueryBuilder::<Postgres>::new(
        "SELECT al.id, al.action::text AS action, al.actor_user_id, actor.username AS actor_username, \
                actor.display_name AS actor_display_name, al.actor_token_id, \
                al.target_user_id, target_user.username AS target_username, \
                target_user.display_name AS target_display_name, al.target_org_id, \
                al.target_package_id, al.target_release_id, al.metadata, al.occurred_at \
         FROM audit_logs al \
         LEFT JOIN users actor ON actor.id = al.actor_user_id \
         LEFT JOIN users target_user ON target_user.id = al.target_user_id \
         WHERE al.target_org_id = ",
    );
    builder.push_bind(org_id);

    builder
}

fn apply_org_audit_filters(
    builder: &mut QueryBuilder<'_, Postgres>,
    filters: &ResolvedOrgAuditFilters,
) -> ApiResult<()> {
    if let Some(action) = &filters.action {
        builder
            .push(" AND al.action = ")
            .push_bind(action.clone())
            .push("::audit_action");
    }
    if let Some(actor_user_id) = filters.actor_user_id {
        builder
            .push(" AND al.actor_user_id = ")
            .push_bind(actor_user_id);
    }
    if let Some(occurred_from) = filters.occurred_from {
        builder
            .push(" AND al.occurred_at >= ")
            .push_bind(org_audit_filter_start(occurred_from)?);
    }
    if let Some(occurred_until) = filters.occurred_until {
        builder
            .push(" AND al.occurred_at < ")
            .push_bind(org_audit_filter_end_exclusive(occurred_until)?);
    }

    Ok(())
}

fn resolve_org_audit_filters(query: &OrgAuditQuery) -> ApiResult<ResolvedOrgAuditFilters> {
    let occurred_from =
        parse_org_audit_date_filter("occurred_from", query.occurred_from.as_deref())?;
    let occurred_until =
        parse_org_audit_date_filter("occurred_until", query.occurred_until.as_deref())?;

    if let (Some(occurred_from), Some(occurred_until)) = (occurred_from, occurred_until) {
        if occurred_from > occurred_until {
            return Err(ApiError(Error::Validation(
                "'occurred_from' must be on or before 'occurred_until'".into(),
            )));
        }
    }

    Ok(ResolvedOrgAuditFilters {
        action: normalize_optional_query_string(query.action.as_deref()),
        actor_user_id: query.actor_user_id,
        occurred_from,
        occurred_until,
    })
}

fn normalize_optional_query_string(value: Option<&str>) -> Option<String> {
    let trimmed = value?.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

fn build_org_audit_csv(rows: &[sqlx::postgres::PgRow]) -> ApiResult<String> {
    let mut writer = WriterBuilder::new().from_writer(Vec::new());
    writer
        .write_record([
            "id",
            "occurred_at",
            "action",
            "actor_user_id",
            "actor_username",
            "actor_display_name",
            "actor_token_id",
            "target_user_id",
            "target_username",
            "target_display_name",
            "target_org_id",
            "target_package_id",
            "target_release_id",
            "metadata_json",
        ])
        .map_err(csv_write_error)?;

    for row in rows {
        let metadata_json = row
            .try_get::<Option<serde_json::Value>, _>("metadata")
            .map_err(|error| ApiError(Error::Internal(error.to_string())))?
            .map(|metadata| metadata.to_string())
            .unwrap_or_default();

        writer
            .write_record([
                row.try_get::<Uuid, _>("id")
                    .map_err(|error| ApiError(Error::Internal(error.to_string())))?
                    .to_string(),
                row.try_get::<chrono::DateTime<chrono::Utc>, _>("occurred_at")
                    .map_err(|error| ApiError(Error::Internal(error.to_string())))?
                    .to_rfc3339(),
                row.try_get::<String, _>("action")
                    .map_err(|error| ApiError(Error::Internal(error.to_string())))?,
                row.try_get::<Option<Uuid>, _>("actor_user_id")
                    .map_err(|error| ApiError(Error::Internal(error.to_string())))?
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                row.try_get::<Option<String>, _>("actor_username")
                    .map_err(|error| ApiError(Error::Internal(error.to_string())))?
                    .unwrap_or_default(),
                row.try_get::<Option<String>, _>("actor_display_name")
                    .map_err(|error| ApiError(Error::Internal(error.to_string())))?
                    .unwrap_or_default(),
                row.try_get::<Option<Uuid>, _>("actor_token_id")
                    .map_err(|error| ApiError(Error::Internal(error.to_string())))?
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                row.try_get::<Option<Uuid>, _>("target_user_id")
                    .map_err(|error| ApiError(Error::Internal(error.to_string())))?
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                row.try_get::<Option<String>, _>("target_username")
                    .map_err(|error| ApiError(Error::Internal(error.to_string())))?
                    .unwrap_or_default(),
                row.try_get::<Option<String>, _>("target_display_name")
                    .map_err(|error| ApiError(Error::Internal(error.to_string())))?
                    .unwrap_or_default(),
                row.try_get::<Option<Uuid>, _>("target_org_id")
                    .map_err(|error| ApiError(Error::Internal(error.to_string())))?
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                row.try_get::<Option<Uuid>, _>("target_package_id")
                    .map_err(|error| ApiError(Error::Internal(error.to_string())))?
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                row.try_get::<Option<Uuid>, _>("target_release_id")
                    .map_err(|error| ApiError(Error::Internal(error.to_string())))?
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                metadata_json,
            ])
            .map_err(csv_write_error)?;
    }

    let bytes = writer
        .into_inner()
        .map_err(|error| ApiError(Error::Internal(error.to_string())))?;

    String::from_utf8(bytes).map_err(|error| ApiError(Error::Internal(error.to_string())))
}

fn csv_write_error(error: csv::Error) -> ApiError {
    ApiError(Error::Internal(error.to_string()))
}

fn parse_org_audit_date_filter(
    field_name: &str,
    value: Option<&str>,
) -> ApiResult<Option<chrono::NaiveDate>> {
    let Some(value) = value else {
        return Ok(None);
    };

    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    chrono::NaiveDate::parse_from_str(trimmed, "%Y-%m-%d")
        .map(Some)
        .map_err(|_| {
            ApiError(Error::Validation(format!(
                "'{field_name}' must use the YYYY-MM-DD format"
            )))
        })
}

fn org_audit_filter_start(date: chrono::NaiveDate) -> ApiResult<chrono::DateTime<chrono::Utc>> {
    let Some(naive_datetime) = date.and_hms_opt(0, 0, 0) else {
        return Err(ApiError(Error::Internal(
            "Failed to construct the audit start timestamp".into(),
        )));
    };

    Ok(chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(
        naive_datetime,
        chrono::Utc,
    ))
}

fn org_audit_filter_end_exclusive(
    date: chrono::NaiveDate,
) -> ApiResult<chrono::DateTime<chrono::Utc>> {
    let Some(next_day) = date.succ_opt() else {
        return Err(ApiError(Error::Validation(
            "'occurred_until' must be earlier than the maximum supported audit date".into(),
        )));
    };

    org_audit_filter_start(next_day)
}

async fn list_members(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path(slug): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_org_member_by_slug(&state.db, &slug, identity.user_id).await?;

    let rows = sqlx::query(
        "SELECT u.id AS user_id, u.username, u.display_name, om.role::text AS role, om.joined_at \
         FROM org_memberships om \
         JOIN users u ON u.id = om.user_id \
         JOIN organizations o ON o.id = om.org_id \
         WHERE o.slug = $1 \
         ORDER BY om.joined_at ASC",
    )
    .bind(&slug)
    .fetch_all(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    let members: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "user_id": r.try_get::<Uuid, _>("user_id").ok(),
                "username": r.try_get::<String, _>("username").ok(),
                "display_name": r.try_get::<Option<String>, _>("display_name").ok().flatten(),
                "role": r.try_get::<String, _>("role").ok(),
                "joined_at": r.try_get::<chrono::DateTime<chrono::Utc>, _>("joined_at").ok(),
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "members": members })))
}

async fn search_org_members(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path(slug): Path<String>,
    Query(query): Query<MemberSearchQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_org_member_by_slug(&state.db, &slug, identity.user_id).await?;

    let Some(search) = query
        .query
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.len() >= 2)
    else {
        return Ok(Json(
            serde_json::json!({ "members": Vec::<serde_json::Value>::new() }),
        ));
    };

    let limit = query.limit.unwrap_or(20).clamp(1, 50) as i64;
    let pattern = format!("{}%", search);

    let rows = sqlx::query(
        "SELECT u.id AS user_id, u.username, u.display_name, om.role::text AS role, om.joined_at \
         FROM org_memberships om \
         JOIN users u ON u.id = om.user_id \
         JOIN organizations o ON o.id = om.org_id \
         WHERE o.slug = $1 \
           AND (u.username ILIKE $2 OR u.display_name ILIKE $2) \
         ORDER BY u.username ASC \
         LIMIT $3",
    )
    .bind(&slug)
    .bind(&pattern)
    .bind(limit)
    .fetch_all(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    let members: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "user_id": r.try_get::<Uuid, _>("user_id").ok(),
                "username": r.try_get::<String, _>("username").ok(),
                "display_name": r.try_get::<Option<String>, _>("display_name").ok().flatten(),
                "role": r.try_get::<String, _>("role").ok(),
                "joined_at": r.try_get::<chrono::DateTime<chrono::Utc>, _>("joined_at").ok(),
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "members": members })))
}

#[derive(Debug, Deserialize)]
struct AddMemberRequest {
    username: String,
    role: String,
}

#[derive(Debug, Deserialize)]
struct MemberSearchQuery {
    query: Option<String>,
    limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct TransferOwnershipRequest {
    username: String,
}

async fn add_member(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path(slug): Path<String>,
    Json(body): Json<AddMemberRequest>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    ensure_scope(&identity, SCOPE_ORGS_WRITE)?;
    let org_id = ensure_org_admin_by_slug(&state.db, &slug, identity.user_id).await?;
    let role = OrgRole::from_str(&body.role).map_err(ApiError::from)?;

    if role.is_owner() {
        return Err(ApiError(Error::Validation(
            "Owner assignment is not supported through member management; use a dedicated ownership transfer flow".into(),
        )));
    }

    let user_row = sqlx::query("SELECT id FROM users WHERE username = $1")
        .bind(&body.username)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?
        .ok_or_else(|| {
            ApiError(Error::NotFound(format!(
                "User '{}' not found",
                body.username
            )))
        })?;

    let user_id: Uuid = user_row
        .try_get("id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    let previous_role = sqlx::query_scalar::<_, String>(
        "SELECT role::text AS role FROM org_memberships WHERE org_id = $1 AND user_id = $2",
    )
    .bind(org_id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query(
        "INSERT INTO org_memberships (id, org_id, user_id, role, invited_by, joined_at) \
            VALUES ($1, $2, $3, $4::org_role, $5, NOW()) \
            ON CONFLICT (org_id, user_id) DO UPDATE \
            SET role = EXCLUDED.role, invited_by = EXCLUDED.invited_by",
    )
    .bind(Uuid::new_v4())
    .bind(org_id)
    .bind(user_id)
    .bind(role.as_str())
    .bind(identity.user_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    let (status, message) = match previous_role.as_deref() {
        None => {
            sqlx::query(
                "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_user_id, target_org_id, metadata, occurred_at) \
                 VALUES ($1, 'org_member_add', $2, $3, $4, $5, $6, NOW())",
            )
            .bind(Uuid::new_v4())
            .bind(identity.user_id)
            .bind(identity.audit_actor_token_id())
            .bind(user_id)
            .bind(org_id)
            .bind(serde_json::json!({
                "username": body.username,
                "role": role.as_str(),
            }))
            .execute(&mut *tx)
            .await
            .map_err(|e| ApiError(Error::Database(e)))?;

            (StatusCode::CREATED, "Member added")
        }
        Some(existing_role) if existing_role != role.as_str() => {
            sqlx::query(
                "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_user_id, target_org_id, metadata, occurred_at) \
                 VALUES ($1, 'org_role_change', $2, $3, $4, $5, $6, NOW())",
            )
            .bind(Uuid::new_v4())
            .bind(identity.user_id)
            .bind(identity.audit_actor_token_id())
            .bind(user_id)
            .bind(org_id)
            .bind(serde_json::json!({
                "username": body.username,
                "previous_role": existing_role,
                "role": role.as_str(),
            }))
            .execute(&mut *tx)
            .await
            .map_err(|e| ApiError(Error::Database(e)))?;

            (StatusCode::OK, "Member role updated")
        }
        Some(_) => (StatusCode::OK, "Member role unchanged"),
    };

    tx.commit()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    Ok((status, Json(serde_json::json!({ "message": message }))))
}

async fn remove_member(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path((slug, username)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_ORGS_WRITE)?;
    let org_id = ensure_org_admin_by_slug(&state.db, &slug, identity.user_id).await?;

    let user_row = sqlx::query("SELECT id FROM users WHERE username = $1")
        .bind(&username)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?
        .ok_or_else(|| ApiError(Error::NotFound(format!("User '{username}' not found"))))?;

    let user_id: Uuid = user_row
        .try_get("id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    let membership_row = sqlx::query(
        "SELECT role::text AS role FROM org_memberships WHERE org_id = $1 AND user_id = $2 FOR UPDATE",
    )
        .bind(org_id)
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?
        .ok_or_else(|| ApiError(Error::NotFound("Membership not found".into())))?;

    let role: String = membership_row
        .try_get("role")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    if OrgRole::from_str(&role).map_err(ApiError::from)?.is_owner() {
        return Err(ApiError(Error::Forbidden(
            "Owner membership cannot be removed through this endpoint".into(),
        )));
    }

    let result = sqlx::query("DELETE FROM org_memberships WHERE org_id = $1 AND user_id = $2")
        .bind(org_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    if result.rows_affected() == 0 {
        return Err(ApiError(Error::NotFound("Membership not found".into())));
    }

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_user_id, target_org_id, metadata, occurred_at) \
         VALUES ($1, 'org_member_remove', $2, $3, $4, $5, $6, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(user_id)
    .bind(org_id)
    .bind(serde_json::json!({
        "username": username,
        "role": role,
    }))
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    tx.commit()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(Json(serde_json::json!({ "message": "Member removed" })))
}

async fn transfer_ownership(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path(slug): Path<String>,
    Json(body): Json<TransferOwnershipRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_ORGS_TRANSFER)?;

    let target_username = body.username.trim();
    if target_username.is_empty() {
        return Err(ApiError(Error::Validation(
            "Ownership transfer target must not be empty".into(),
        )));
    }

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    let org_row = sqlx::query(
        "SELECT id, name \
         FROM organizations \
         WHERE slug = $1",
    )
    .bind(&slug)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| ApiError(Error::NotFound(format!("Organization '{slug}' not found"))))?;

    let org_id: Uuid = org_row
        .try_get("id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let org_name: String = org_row
        .try_get("name")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    let actor_membership_row = sqlx::query(
        "SELECT role::text AS role \
         FROM org_memberships \
         WHERE org_id = $1 AND user_id = $2 \
         FOR UPDATE",
    )
    .bind(org_id)
    .bind(identity.user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| {
        ApiError(Error::Forbidden(
            "Transferring organization ownership requires an owner membership".into(),
        ))
    })?;

    let actor_role = actor_membership_row
        .try_get::<String, _>("role")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let actor_role = OrgRole::from_str(&actor_role).map_err(ApiError::from)?;

    let target_membership_row = sqlx::query(
        "SELECT om.user_id, om.role::text AS role, u.username \
         FROM org_memberships om \
         JOIN users u ON u.id = om.user_id \
         WHERE om.org_id = $1 AND u.username = $2 AND u.is_active = true \
         FOR UPDATE OF om",
    )
    .bind(org_id)
    .bind(target_username)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| {
        ApiError(Error::NotFound(
            "The target user must already be an active organization member".into(),
        ))
    })?;

    let target_user_id: Uuid = target_membership_row
        .try_get("user_id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let target_username: String = target_membership_row
        .try_get("username")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let target_role = target_membership_row
        .try_get::<String, _>("role")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let target_role = OrgRole::from_str(&target_role).map_err(ApiError::from)?;

    validate_ownership_transfer(identity.user_id, &actor_role, target_user_id, &target_role)?;

    sqlx::query(
        "UPDATE org_memberships \
         SET role = 'owner' \
         WHERE org_id = $1 AND user_id = $2",
    )
    .bind(org_id)
    .bind(target_user_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query(
        "UPDATE org_memberships \
         SET role = 'admin' \
         WHERE org_id = $1 AND user_id = $2",
    )
    .bind(org_id)
    .bind(identity.user_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_user_id, target_org_id, metadata, occurred_at) \
         VALUES ($1, 'org_ownership_transfer', $2, $3, $4, $5, $6, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(target_user_id)
    .bind(org_id)
    .bind(serde_json::json!({
        "org_slug": slug,
        "org_name": org_name,
        "former_owner_user_id": identity.user_id,
        "former_owner_new_role": OrgRole::Admin.as_str(),
        "new_owner_user_id": target_user_id,
        "new_owner_username": target_username,
        "new_owner_previous_role": target_role.as_str(),
    }))
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    tx.commit()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(Json(serde_json::json!({
        "message": "Organization ownership transferred",
        "org": {
            "id": org_id,
            "slug": slug,
            "name": org_name,
        },
        "previous_owner": {
            "id": identity.user_id,
            "new_role": OrgRole::Admin.as_str(),
        },
        "new_owner": {
            "id": target_user_id,
            "username": target_username,
            "role": OrgRole::Owner.as_str(),
        },
    })))
}

fn validate_ownership_transfer(
    actor_user_id: Uuid,
    actor_role: &OrgRole,
    target_user_id: Uuid,
    target_role: &OrgRole,
) -> ApiResult<()> {
    if !actor_role.is_owner() {
        return Err(ApiError(Error::Forbidden(
            "Transferring organization ownership requires an owner membership".into(),
        )));
    }

    if actor_user_id == target_user_id {
        return Err(ApiError(Error::Validation(
            "Ownership transfer target must be a different organization member".into(),
        )));
    }

    if target_role.is_owner() {
        return Err(ApiError(Error::Conflict(
            "The selected member is already an organization owner".into(),
        )));
    }

    Ok(())
}

async fn list_teams(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path(slug): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_org_member_by_slug(&state.db, &slug, identity.user_id).await?;

    let rows = sqlx::query(
        "SELECT t.id, t.name, t.slug, t.description, t.created_at \
         FROM teams t \
         JOIN organizations o ON o.id = t.org_id \
         WHERE o.slug = $1 \
         ORDER BY t.name ASC",
    )
    .bind(&slug)
    .fetch_all(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    let teams: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.try_get::<Uuid, _>("id").ok(),
                "name": r.try_get::<String, _>("name").ok(),
                "slug": r.try_get::<String, _>("slug").ok(),
                "description": r.try_get::<Option<String>, _>("description").ok().flatten(),
                "created_at": r.try_get::<chrono::DateTime<chrono::Utc>, _>("created_at").ok(),
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "teams": teams })))
}

#[derive(Debug, Deserialize)]
struct CreateTeamRequest {
    name: String,
    slug: String,
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UpdateTeamRequest {
    name: Option<String>,
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AddTeamMemberRequest {
    username: String,
}

#[derive(Debug, Deserialize)]
struct ReplaceTeamPackageAccessRequest {
    permissions: Vec<String>,
}

#[derive(Debug, Clone)]
struct TeamRecord {
    id: Uuid,
    org_id: Uuid,
    name: String,
    slug: String,
    description: Option<String>,
}

#[derive(Debug, Clone)]
struct TeamPackageAccessTarget {
    id: Uuid,
    ecosystem: String,
    name: String,
    normalized_name: String,
}

#[derive(Debug, Clone)]
struct TeamRepositoryAccessTarget {
    id: Uuid,
    name: String,
    slug: String,
    kind: String,
    visibility: String,
}

#[derive(Debug, Clone)]
struct TeamNamespaceAccessTarget {
    id: Uuid,
    ecosystem: String,
    namespace: String,
    is_verified: bool,
}

async fn create_team(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path(slug): Path<String>,
    Json(body): Json<CreateTeamRequest>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    ensure_scope(&identity, SCOPE_ORGS_WRITE)?;
    validation::validate_slug(&body.slug).map_err(ApiError::from)?;

    if body.name.trim().is_empty() {
        return Err(ApiError(Error::Validation(
            "Team name must not be empty".into(),
        )));
    }

    let org_id = ensure_org_admin_by_slug(&state.db, &slug, identity.user_id).await?;
    let team_id = Uuid::new_v4();

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query(
        "INSERT INTO teams (id, org_id, name, slug, description, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, NOW(), NOW())",
    )
    .bind(team_id)
    .bind(org_id)
    .bind(body.name.trim())
    .bind(&body.slug)
    .bind(&body.description)
    .execute(&mut *tx)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db) if db.is_unique_violation() => ApiError(Error::AlreadyExists(
            "Team slug already exists in this organization".into(),
        )),
        _ => ApiError(Error::Database(e)),
    })?;

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_org_id, metadata, occurred_at) \
         VALUES ($1, 'team_create', $2, $3, $4, $5, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(org_id)
    .bind(serde_json::json!({
        "team_id": team_id,
        "team_slug": body.slug,
        "team_name": body.name.trim(),
        "description": body.description,
    }))
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    tx.commit()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "id": team_id,
            "slug": body.slug,
            "name": body.name.trim(),
        })),
    ))
}

async fn update_team(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path((slug, team_slug)): Path<(String, String)>,
    Json(body): Json<UpdateTeamRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_ORGS_WRITE)?;

    if body.name.is_none() && body.description.is_none() {
        return Err(ApiError(Error::Validation(
            "At least one team field must be provided".into(),
        )));
    }

    if body
        .name
        .as_ref()
        .is_some_and(|name| name.trim().is_empty())
    {
        return Err(ApiError(Error::Validation(
            "Team name must not be empty".into(),
        )));
    }

    let org_id = ensure_org_admin_by_slug(&state.db, &slug, identity.user_id).await?;
    let team = load_team_record(&state.db, org_id, &team_slug).await?;
    let updated_name = body.name.as_deref().map(str::trim);

    sqlx::query(
        "UPDATE teams \
         SET name = COALESCE($1, name), \
             description = COALESCE($2, description), \
             updated_at = NOW() \
         WHERE id = $3",
    )
    .bind(updated_name)
    .bind(&body.description)
    .bind(team.id)
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_org_id, metadata, occurred_at) \
         VALUES ($1, 'team_update', $2, $3, $4, $5, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(org_id)
    .bind(serde_json::json!({
        "team_id": team.id,
        "team_slug": team.slug,
        "previous_name": team.name,
        "previous_description": team.description,
        "name": updated_name.unwrap_or(team.name.as_str()),
        "description": body.description,
    }))
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(Json(serde_json::json!({ "message": "Team updated" })))
}

async fn delete_team(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path((slug, team_slug)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_ORGS_WRITE)?;

    let org_id = ensure_org_admin_by_slug(&state.db, &slug, identity.user_id).await?;
    let team = load_team_record(&state.db, org_id, &team_slug).await?;

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    let member_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*)::BIGINT FROM team_memberships WHERE team_id = $1")
            .bind(team.id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| ApiError(Error::Database(e)))?;

    let package_access_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*)::BIGINT FROM team_package_access WHERE team_id = $1")
            .bind(team.id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| ApiError(Error::Database(e)))?;

    let repository_access_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM team_repository_access WHERE team_id = $1",
    )
    .bind(team.id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    let namespace_access_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*)::BIGINT FROM team_namespace_access WHERE team_id = $1")
            .bind(team.id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query("DELETE FROM teams WHERE id = $1")
        .bind(team.id)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_org_id, metadata, occurred_at) \
         VALUES ($1, 'team_delete', $2, $3, $4, $5, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(org_id)
    .bind(serde_json::json!({
        "team_id": team.id,
        "team_slug": team.slug,
        "team_name": team.name,
        "removed_member_count": member_count,
        "removed_package_access_count": package_access_count,
        "removed_repository_access_count": repository_access_count,
        "removed_namespace_access_count": namespace_access_count,
    }))
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    tx.commit()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(Json(serde_json::json!({ "message": "Team deleted" })))
}

async fn list_team_members(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path((slug, team_slug)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_ORGS_WRITE)?;

    let org_id = ensure_org_admin_by_slug(&state.db, &slug, identity.user_id).await?;
    let team = load_team_record(&state.db, org_id, &team_slug).await?;

    let rows = sqlx::query(
        "SELECT u.id, u.username, u.display_name, tm.added_at \
         FROM team_memberships tm \
         JOIN users u ON u.id = tm.user_id \
         WHERE tm.team_id = $1 \
         ORDER BY tm.added_at ASC",
    )
    .bind(team.id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    let members = rows
        .iter()
        .map(|row| {
            serde_json::json!({
                "id": row.try_get::<Uuid, _>("id").ok(),
                "username": row.try_get::<String, _>("username").ok(),
                "display_name": row.try_get::<Option<String>, _>("display_name").ok().flatten(),
                "added_at": row.try_get::<chrono::DateTime<chrono::Utc>, _>("added_at").ok(),
            })
        })
        .collect::<Vec<_>>();

    Ok(Json(serde_json::json!({
        "team": {
            "id": team.id,
            "slug": team.slug,
            "name": team.name,
        },
        "members": members,
    })))
}

async fn add_team_member(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path((slug, team_slug)): Path<(String, String)>,
    Json(body): Json<AddTeamMemberRequest>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    ensure_scope(&identity, SCOPE_ORGS_WRITE)?;

    let org_id = ensure_org_admin_by_slug(&state.db, &slug, identity.user_id).await?;
    let team = load_team_record(&state.db, org_id, &team_slug).await?;

    let user_row = sqlx::query(
        "SELECT u.id, EXISTS (\
             SELECT 1 \
             FROM org_memberships om \
             WHERE om.org_id = $1 AND om.user_id = u.id\
         ) AS is_org_member \
         FROM users u \
         WHERE u.username = $2",
    )
    .bind(org_id)
    .bind(&body.username)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| {
        ApiError(Error::NotFound(format!(
            "User '{}' not found",
            body.username
        )))
    })?;

    let user_id: Uuid = user_row
        .try_get("id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let is_org_member = user_row
        .try_get::<bool, _>("is_org_member")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    if !is_org_member {
        return Err(ApiError(Error::Conflict(
            "Team members must already belong to the organization".into(),
        )));
    }

    let result = sqlx::query(
        "INSERT INTO team_memberships (id, team_id, user_id, added_at) \
         VALUES ($1, $2, $3, NOW()) \
         ON CONFLICT (team_id, user_id) DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind(team.id)
    .bind(user_id)
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    if result.rows_affected() == 0 {
        return Ok((
            StatusCode::OK,
            Json(serde_json::json!({ "message": "Team member already present" })),
        ));
    }

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_user_id, target_org_id, metadata, occurred_at) \
         VALUES ($1, 'team_member_add', $2, $3, $4, $5, $6, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(user_id)
    .bind(org_id)
    .bind(serde_json::json!({
        "team_id": team.id,
        "team_slug": team.slug,
        "team_name": team.name,
        "username": body.username,
    }))
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "message": "Team member added" })),
    ))
}

async fn remove_team_member(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path((slug, team_slug, username)): Path<(String, String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_ORGS_WRITE)?;

    let org_id = ensure_org_admin_by_slug(&state.db, &slug, identity.user_id).await?;
    let team = load_team_record(&state.db, org_id, &team_slug).await?;

    let user_row = sqlx::query("SELECT id FROM users WHERE username = $1")
        .bind(&username)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?
        .ok_or_else(|| ApiError(Error::NotFound(format!("User '{username}' not found"))))?;

    let user_id: Uuid = user_row
        .try_get("id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    let result = sqlx::query("DELETE FROM team_memberships WHERE team_id = $1 AND user_id = $2")
        .bind(team.id)
        .bind(user_id)
        .execute(&state.db)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    if result.rows_affected() == 0 {
        return Err(ApiError(Error::NotFound(
            "Team membership not found".into(),
        )));
    }

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_user_id, target_org_id, metadata, occurred_at) \
         VALUES ($1, 'team_member_remove', $2, $3, $4, $5, $6, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(user_id)
    .bind(org_id)
    .bind(serde_json::json!({
        "team_id": team.id,
        "team_slug": team.slug,
        "team_name": team.name,
        "username": username,
    }))
    .execute(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(Json(
        serde_json::json!({ "message": "Team member removed" }),
    ))
}

async fn list_team_package_access(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path((slug, team_slug)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_ORGS_WRITE)?;

    let org_id = ensure_org_admin_by_slug(&state.db, &slug, identity.user_id).await?;
    let team = load_team_record(&state.db, org_id, &team_slug).await?;

    let rows = sqlx::query(
        "SELECT p.id, p.name, p.normalized_name, p.ecosystem, \
                ARRAY_AGG(tpa.permission::text ORDER BY tpa.permission::text) AS permissions, \
                MAX(tpa.granted_at) AS granted_at \
         FROM team_package_access tpa \
         JOIN packages p ON p.id = tpa.package_id \
         WHERE tpa.team_id = $1 AND p.owner_org_id = $2 \
         GROUP BY p.id, p.name, p.normalized_name, p.ecosystem \
         ORDER BY p.ecosystem ASC, p.name ASC",
    )
    .bind(team.id)
    .bind(org_id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    let package_access = rows
        .iter()
        .map(|row| {
            serde_json::json!({
                "package_id": row.try_get::<Uuid, _>("id").ok(),
                "name": row.try_get::<String, _>("name").ok(),
                "normalized_name": row.try_get::<String, _>("normalized_name").ok(),
                "ecosystem": row.try_get::<String, _>("ecosystem").ok(),
                "permissions": row.try_get::<Vec<String>, _>("permissions").ok(),
                "granted_at": row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("granted_at").ok().flatten(),
            })
        })
        .collect::<Vec<_>>();

    Ok(Json(serde_json::json!({
        "team": {
            "id": team.id,
            "slug": team.slug,
            "name": team.name,
        },
        "package_access": package_access,
    })))
}

async fn replace_team_package_access(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path((slug, team_slug, ecosystem_str, name)): Path<(String, String, String, String)>,
    Json(body): Json<ReplaceTeamPackageAccessRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_ORGS_WRITE)?;

    let org_id = ensure_org_admin_by_slug(&state.db, &slug, identity.user_id).await?;
    let team = load_team_record(&state.db, org_id, &team_slug).await?;
    let ecosystem = crate::routes::parse_ecosystem(&ecosystem_str)?;
    let package =
        load_org_owned_package_for_team_access(&state.db, org_id, &ecosystem, &name).await?;
    let permissions = normalize_team_permissions(&body.permissions)?;
    let permission_strings = team_permission_strings(&permissions);

    let previous_permissions = sqlx::query(
        "SELECT permission::text AS permission \
         FROM team_package_access \
         WHERE team_id = $1 AND package_id = $2 \
         ORDER BY permission::text ASC",
    )
    .bind(team.id)
    .bind(package.id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .into_iter()
    .filter_map(|row| row.try_get::<String, _>("permission").ok())
    .collect::<Vec<_>>();

    if previous_permissions == permission_strings {
        return Ok(Json(serde_json::json!({
            "message": "Team package access unchanged",
            "package": {
                "id": package.id,
                "ecosystem": package.ecosystem,
                "name": package.name,
                "normalized_name": package.normalized_name,
            },
            "permissions": permission_strings,
        })));
    }

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query("DELETE FROM team_package_access WHERE team_id = $1 AND package_id = $2")
        .bind(team.id)
        .bind(package.id)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    for permission in &permissions {
        sqlx::query(
            "INSERT INTO team_package_access (id, team_id, package_id, permission, granted_at) \
             VALUES ($1, $2, $3, $4, NOW())",
        )
        .bind(Uuid::new_v4())
        .bind(team.id)
        .bind(package.id)
        .bind(permission)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;
    }

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_org_id, target_package_id, metadata, occurred_at) \
         VALUES ($1, 'team_package_access_update', $2, $3, $4, $5, $6, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(team.org_id)
    .bind(package.id)
    .bind(serde_json::json!({
        "team_id": team.id,
        "team_slug": team.slug,
        "team_name": team.name,
        "ecosystem": package.ecosystem,
        "package_name": package.name,
        "package_normalized_name": package.normalized_name,
        "previous_permissions": previous_permissions,
        "permissions": permission_strings,
    }))
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    tx.commit()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(Json(serde_json::json!({
        "message": "Team package access updated",
        "package": {
            "id": package.id,
            "ecosystem": package.ecosystem,
            "name": package.name,
            "normalized_name": package.normalized_name,
        },
        "permissions": permission_strings,
    })))
}

async fn remove_team_package_access(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path((slug, team_slug, ecosystem_str, name)): Path<(String, String, String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_ORGS_WRITE)?;

    let org_id = ensure_org_admin_by_slug(&state.db, &slug, identity.user_id).await?;
    let team = load_team_record(&state.db, org_id, &team_slug).await?;
    let ecosystem = crate::routes::parse_ecosystem(&ecosystem_str)?;
    let package =
        load_org_owned_package_for_team_access(&state.db, org_id, &ecosystem, &name).await?;

    let previous_permissions = sqlx::query(
        "SELECT permission::text AS permission \
         FROM team_package_access \
         WHERE team_id = $1 AND package_id = $2 \
         ORDER BY permission::text ASC",
    )
    .bind(team.id)
    .bind(package.id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .into_iter()
    .filter_map(|row| row.try_get::<String, _>("permission").ok())
    .collect::<Vec<_>>();

    if previous_permissions.is_empty() {
        return Err(ApiError(Error::NotFound(
            "Team package access not found".into(),
        )));
    }

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query("DELETE FROM team_package_access WHERE team_id = $1 AND package_id = $2")
        .bind(team.id)
        .bind(package.id)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_org_id, target_package_id, metadata, occurred_at) \
         VALUES ($1, 'team_package_access_update', $2, $3, $4, $5, $6, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(team.org_id)
    .bind(package.id)
    .bind(serde_json::json!({
        "team_id": team.id,
        "team_slug": team.slug,
        "team_name": team.name,
        "ecosystem": package.ecosystem,
        "package_name": package.name,
        "package_normalized_name": package.normalized_name,
        "previous_permissions": previous_permissions,
        "permissions": Vec::<String>::new(),
    }))
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    tx.commit()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(Json(
        serde_json::json!({ "message": "Team package access removed" }),
    ))
}

async fn list_team_repository_access(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path((slug, team_slug)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_ORGS_WRITE)?;

    let org_id = ensure_org_admin_by_slug(&state.db, &slug, identity.user_id).await?;
    let team = load_team_record(&state.db, org_id, &team_slug).await?;

    let rows = sqlx::query(
        "SELECT r.id, r.name, r.slug, r.kind::text AS kind, r.visibility::text AS visibility, \
                ARRAY_AGG(tra.permission::text ORDER BY tra.permission::text) AS permissions, \
                MAX(tra.granted_at) AS granted_at \
         FROM team_repository_access tra \
         JOIN repositories r ON r.id = tra.repository_id \
         WHERE tra.team_id = $1 AND r.owner_org_id = $2 \
         GROUP BY r.id, r.name, r.slug, r.kind, r.visibility \
         ORDER BY r.name ASC, r.slug ASC",
    )
    .bind(team.id)
    .bind(org_id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    let repository_access = rows
        .iter()
        .map(|row| {
            serde_json::json!({
                "repository_id": row.try_get::<Uuid, _>("id").ok(),
                "name": row.try_get::<String, _>("name").ok(),
                "slug": row.try_get::<String, _>("slug").ok(),
                "kind": row.try_get::<String, _>("kind").ok(),
                "visibility": row.try_get::<String, _>("visibility").ok(),
                "permissions": row.try_get::<Vec<String>, _>("permissions").ok(),
                "granted_at": row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("granted_at").ok().flatten(),
            })
        })
        .collect::<Vec<_>>();

    Ok(Json(serde_json::json!({
        "team": {
            "id": team.id,
            "slug": team.slug,
            "name": team.name,
        },
        "repository_access": repository_access,
    })))
}

async fn replace_team_repository_access(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path((slug, team_slug, repository_slug)): Path<(String, String, String)>,
    Json(body): Json<ReplaceTeamPackageAccessRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_ORGS_WRITE)?;

    let org_id = ensure_org_admin_by_slug(&state.db, &slug, identity.user_id).await?;
    let team = load_team_record(&state.db, org_id, &team_slug).await?;
    let repository =
        load_org_owned_repository_for_team_access(&state.db, org_id, &repository_slug).await?;
    let permissions = normalize_team_permissions(&body.permissions)?;
    let permission_strings = team_permission_strings(&permissions);

    let previous_permissions = sqlx::query(
        "SELECT permission::text AS permission \
         FROM team_repository_access \
         WHERE team_id = $1 AND repository_id = $2 \
         ORDER BY permission::text ASC",
    )
    .bind(team.id)
    .bind(repository.id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .into_iter()
    .filter_map(|row| row.try_get::<String, _>("permission").ok())
    .collect::<Vec<_>>();

    if previous_permissions == permission_strings {
        return Ok(Json(serde_json::json!({
            "message": "Team repository access unchanged",
            "repository": {
                "id": repository.id,
                "name": repository.name,
                "slug": repository.slug,
                "kind": repository.kind,
                "visibility": repository.visibility,
            },
            "permissions": permission_strings,
        })));
    }

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query("DELETE FROM team_repository_access WHERE team_id = $1 AND repository_id = $2")
        .bind(team.id)
        .bind(repository.id)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    for permission in &permissions {
        sqlx::query(
            "INSERT INTO team_repository_access (id, team_id, repository_id, permission, granted_at) \
             VALUES ($1, $2, $3, $4, NOW())",
        )
        .bind(Uuid::new_v4())
        .bind(team.id)
        .bind(repository.id)
        .bind(permission)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;
    }

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_org_id, metadata, occurred_at) \
         VALUES ($1, 'team_repository_access_update', $2, $3, $4, $5, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(team.org_id)
    .bind(serde_json::json!({
        "team_id": team.id,
        "team_slug": team.slug,
        "team_name": team.name,
        "repository_id": repository.id,
        "repository_slug": repository.slug,
        "repository_name": repository.name,
        "repository_kind": repository.kind,
        "repository_visibility": repository.visibility,
        "previous_permissions": previous_permissions,
        "permissions": permission_strings,
    }))
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    tx.commit()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(Json(serde_json::json!({
        "message": "Team repository access updated",
        "repository": {
            "id": repository.id,
            "name": repository.name,
            "slug": repository.slug,
            "kind": repository.kind,
            "visibility": repository.visibility,
        },
        "permissions": permission_strings,
    })))
}

async fn remove_team_repository_access(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path((slug, team_slug, repository_slug)): Path<(String, String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_ORGS_WRITE)?;

    let org_id = ensure_org_admin_by_slug(&state.db, &slug, identity.user_id).await?;
    let team = load_team_record(&state.db, org_id, &team_slug).await?;
    let repository =
        load_org_owned_repository_for_team_access(&state.db, org_id, &repository_slug).await?;

    let previous_permissions = sqlx::query(
        "SELECT permission::text AS permission \
         FROM team_repository_access \
         WHERE team_id = $1 AND repository_id = $2 \
         ORDER BY permission::text ASC",
    )
    .bind(team.id)
    .bind(repository.id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .into_iter()
    .filter_map(|row| row.try_get::<String, _>("permission").ok())
    .collect::<Vec<_>>();

    if previous_permissions.is_empty() {
        return Err(ApiError(Error::NotFound(
            "Team repository access not found".into(),
        )));
    }

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query("DELETE FROM team_repository_access WHERE team_id = $1 AND repository_id = $2")
        .bind(team.id)
        .bind(repository.id)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_org_id, metadata, occurred_at) \
         VALUES ($1, 'team_repository_access_update', $2, $3, $4, $5, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(team.org_id)
    .bind(serde_json::json!({
        "team_id": team.id,
        "team_slug": team.slug,
        "team_name": team.name,
        "repository_id": repository.id,
        "repository_slug": repository.slug,
        "repository_name": repository.name,
        "repository_kind": repository.kind,
        "repository_visibility": repository.visibility,
        "previous_permissions": previous_permissions,
        "permissions": Vec::<String>::new(),
    }))
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    tx.commit()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(Json(
        serde_json::json!({ "message": "Team repository access removed" }),
    ))
}

async fn list_team_namespace_access(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path((slug, team_slug)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_ORGS_WRITE)?;

    let org_id = ensure_org_admin_by_slug(&state.db, &slug, identity.user_id).await?;
    let team = load_team_record(&state.db, org_id, &team_slug).await?;

    let rows = sqlx::query(
        "SELECT nc.id, nc.ecosystem::text AS ecosystem, nc.namespace, nc.is_verified, \
                ARRAY_AGG(tna.permission::text ORDER BY tna.permission::text) AS permissions, \
                MAX(tna.granted_at) AS granted_at \
         FROM team_namespace_access tna \
         JOIN namespace_claims nc ON nc.id = tna.namespace_claim_id \
         WHERE tna.team_id = $1 AND nc.owner_org_id = $2 \
         GROUP BY nc.id, nc.ecosystem, nc.namespace, nc.is_verified \
         ORDER BY nc.ecosystem ASC, nc.namespace ASC",
    )
    .bind(team.id)
    .bind(org_id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    let namespace_access = rows
        .iter()
        .map(|row| {
            serde_json::json!({
                "namespace_claim_id": row.try_get::<Uuid, _>("id").ok(),
                "ecosystem": row.try_get::<String, _>("ecosystem").ok(),
                "namespace": row.try_get::<String, _>("namespace").ok(),
                "is_verified": row.try_get::<bool, _>("is_verified").ok(),
                "permissions": row.try_get::<Vec<String>, _>("permissions").ok(),
                "granted_at": row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("granted_at").ok().flatten(),
            })
        })
        .collect::<Vec<_>>();

    Ok(Json(serde_json::json!({
        "team": {
            "id": team.id,
            "slug": team.slug,
            "name": team.name,
        },
        "namespace_access": namespace_access,
    })))
}

async fn replace_team_namespace_access(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path((slug, team_slug, claim_id)): Path<(String, String, Uuid)>,
    Json(body): Json<ReplaceTeamPackageAccessRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_ORGS_WRITE)?;

    let org_id = ensure_org_admin_by_slug(&state.db, &slug, identity.user_id).await?;
    let team = load_team_record(&state.db, org_id, &team_slug).await?;
    let claim = load_org_owned_namespace_claim_for_team_access(&state.db, org_id, claim_id).await?;
    let permissions = normalize_namespace_team_permissions(&body.permissions)?;
    let permission_strings = team_permission_strings(&permissions);

    let previous_permissions = sqlx::query(
        "SELECT permission::text AS permission \
         FROM team_namespace_access \
         WHERE team_id = $1 AND namespace_claim_id = $2 \
         ORDER BY permission::text ASC",
    )
    .bind(team.id)
    .bind(claim.id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .into_iter()
    .filter_map(|row| row.try_get::<String, _>("permission").ok())
    .collect::<Vec<_>>();

    if previous_permissions == permission_strings {
        return Ok(Json(serde_json::json!({
            "message": "Team namespace access unchanged",
            "namespace_claim": {
                "id": claim.id,
                "ecosystem": claim.ecosystem,
                "namespace": claim.namespace,
                "is_verified": claim.is_verified,
            },
            "permissions": permission_strings,
        })));
    }

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query("DELETE FROM team_namespace_access WHERE team_id = $1 AND namespace_claim_id = $2")
        .bind(team.id)
        .bind(claim.id)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    for permission in &permissions {
        sqlx::query(
            "INSERT INTO team_namespace_access (id, team_id, namespace_claim_id, permission, granted_at) \
             VALUES ($1, $2, $3, $4, NOW())",
        )
        .bind(Uuid::new_v4())
        .bind(team.id)
        .bind(claim.id)
        .bind(permission)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;
    }

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_org_id, metadata, occurred_at) \
         VALUES ($1, 'team_namespace_access_update', $2, $3, $4, $5, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(team.org_id)
    .bind(serde_json::json!({
        "team_id": team.id,
        "team_slug": team.slug,
        "team_name": team.name,
        "namespace_claim_id": claim.id,
        "ecosystem": claim.ecosystem,
        "namespace": claim.namespace,
        "is_verified": claim.is_verified,
        "previous_permissions": previous_permissions,
        "permissions": permission_strings,
    }))
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    tx.commit()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(Json(serde_json::json!({
        "message": "Team namespace access updated",
        "namespace_claim": {
            "id": claim.id,
            "ecosystem": claim.ecosystem,
            "namespace": claim.namespace,
            "is_verified": claim.is_verified,
        },
        "permissions": permission_strings,
    })))
}

async fn remove_team_namespace_access(
    State(state): State<AppState>,
    identity: AuthenticatedIdentity,
    Path((slug, team_slug, claim_id)): Path<(String, String, Uuid)>,
) -> ApiResult<Json<serde_json::Value>> {
    ensure_scope(&identity, SCOPE_ORGS_WRITE)?;

    let org_id = ensure_org_admin_by_slug(&state.db, &slug, identity.user_id).await?;
    let team = load_team_record(&state.db, org_id, &team_slug).await?;
    let claim = load_org_owned_namespace_claim_for_team_access(&state.db, org_id, claim_id).await?;

    let previous_permissions = sqlx::query(
        "SELECT permission::text AS permission \
         FROM team_namespace_access \
         WHERE team_id = $1 AND namespace_claim_id = $2 \
         ORDER BY permission::text ASC",
    )
    .bind(team.id)
    .bind(claim.id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .into_iter()
    .filter_map(|row| row.try_get::<String, _>("permission").ok())
    .collect::<Vec<_>>();

    if previous_permissions.is_empty() {
        return Err(ApiError(Error::NotFound(
            "Team namespace access not found".into(),
        )));
    }

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query("DELETE FROM team_namespace_access WHERE team_id = $1 AND namespace_claim_id = $2")
        .bind(team.id)
        .bind(claim.id)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    sqlx::query(
        "INSERT INTO audit_logs (id, action, actor_user_id, actor_token_id, target_org_id, metadata, occurred_at) \
         VALUES ($1, 'team_namespace_access_update', $2, $3, $4, $5, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(identity.user_id)
    .bind(identity.audit_actor_token_id())
    .bind(team.org_id)
    .bind(serde_json::json!({
        "team_id": team.id,
        "team_slug": team.slug,
        "team_name": team.name,
        "namespace_claim_id": claim.id,
        "ecosystem": claim.ecosystem,
        "namespace": claim.namespace,
        "is_verified": claim.is_verified,
        "previous_permissions": previous_permissions,
        "permissions": Vec::<String>::new(),
    }))
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    tx.commit()
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    Ok(Json(
        serde_json::json!({ "message": "Team namespace access removed" }),
    ))
}

async fn list_org_packages(
    State(state): State<AppState>,
    identity: OptionalAuthenticatedIdentity,
    Path(slug): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Json<serde_json::Value>> {
    let limit: i64 = q
        .get("per_page")
        .and_then(|s| s.parse().ok())
        .unwrap_or(20_i64)
        .min(100);
    let page: i64 = q.get("page").and_then(|s| s.parse().ok()).unwrap_or(1_i64);
    let offset = (page - 1) * limit;
    let org_row = sqlx::query("SELECT id FROM organizations WHERE slug = $1")
        .bind(&slug)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?
        .ok_or_else(|| ApiError(Error::NotFound(format!("Organization '{slug}' not found"))))?;

    let org_id: Uuid = org_row
        .try_get("id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let actor_user_id = identity.user_id();
    let can_view_non_public =
        actor_can_access_org_member_directory_by_id(&state.db, org_id, actor_user_id).await?;

    let rows = sqlx::query(
        "SELECT p.id, p.name, p.ecosystem, p.description, p.download_count, p.created_at \
         FROM packages p \
         JOIN repositories r ON r.id = p.repository_id \
         WHERE p.owner_org_id = $1 \
           AND ($2::bool = true OR (p.visibility = 'public' AND r.visibility = 'public')) \
         ORDER BY p.download_count DESC \
         LIMIT $3 OFFSET $4",
    )
    .bind(org_id)
    .bind(can_view_non_public)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    let mut packages = Vec::with_capacity(rows.len());
    for row in rows {
        let package_id: Uuid = row
            .try_get("id")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
        let can_transfer =
            actor_can_transfer_package_by_id(&state.db, package_id, actor_user_id).await?;

        packages.push(serde_json::json!({
            "id": package_id,
            "name": row.try_get::<String, _>("name").ok(),
            "ecosystem": row.try_get::<String, _>("ecosystem").ok(),
            "description": row.try_get::<Option<String>, _>("description").ok().flatten(),
            "download_count": row.try_get::<i64, _>("download_count").ok(),
            "created_at": row.try_get::<chrono::DateTime<chrono::Utc>, _>("created_at").ok(),
            "can_transfer": can_transfer,
        }));
    }

    Ok(Json(serde_json::json!({ "packages": packages })))
}

async fn list_org_repositories(
    State(state): State<AppState>,
    identity: OptionalAuthenticatedIdentity,
    Path(slug): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Json<serde_json::Value>> {
    let limit: i64 = q
        .get("per_page")
        .and_then(|s| s.parse().ok())
        .unwrap_or(20_i64)
        .min(100);
    let page: i64 = q.get("page").and_then(|s| s.parse().ok()).unwrap_or(1_i64);
    let offset = (page - 1) * limit;
    let org_row = sqlx::query("SELECT id FROM organizations WHERE slug = $1")
        .bind(&slug)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?
        .ok_or_else(|| ApiError(Error::NotFound(format!("Organization '{slug}' not found"))))?;

    let org_id: Uuid = org_row
        .try_get("id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let actor_user_id = identity.user_id();
    let can_view_non_public =
        actor_can_access_org_member_directory_by_id(&state.db, org_id, actor_user_id).await?;

    let rows = sqlx::query(
        "SELECT r.id, r.name, r.slug, r.description, r.kind::text AS kind, \
                r.visibility::text AS visibility, r.upstream_url, r.created_at, \
                COUNT(p.id) FILTER (WHERE $2::bool = true OR p.visibility = 'public')::BIGINT AS package_count \
         FROM repositories r \
         LEFT JOIN packages p ON p.repository_id = r.id \
         WHERE r.owner_org_id = $1 \
           AND ($2::bool = true OR r.visibility = 'public') \
         GROUP BY r.id, r.name, r.slug, r.description, r.kind, r.visibility, r.upstream_url, r.created_at \
         ORDER BY LOWER(r.name), LOWER(r.slug) \
         LIMIT $3 OFFSET $4",
    )
    .bind(org_id)
    .bind(can_view_non_public)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    let mut repositories = Vec::with_capacity(rows.len());
    for row in rows {
        let repository_id: Uuid = row
            .try_get("id")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
        let can_transfer =
            actor_can_transfer_repository_by_id(&state.db, repository_id, actor_user_id).await?;

        repositories.push(serde_json::json!({
            "id": repository_id,
            "name": row.try_get::<String, _>("name").ok(),
            "slug": row.try_get::<String, _>("slug").ok(),
            "description": row.try_get::<Option<String>, _>("description").ok().flatten(),
            "kind": row.try_get::<String, _>("kind").ok(),
            "visibility": row.try_get::<String, _>("visibility").ok(),
            "upstream_url": row.try_get::<Option<String>, _>("upstream_url").ok().flatten(),
            "package_count": row.try_get::<i64, _>("package_count").ok(),
            "created_at": row.try_get::<chrono::DateTime<chrono::Utc>, _>("created_at").ok(),
            "can_transfer": can_transfer,
        }));
    }

    Ok(Json(serde_json::json!({ "repositories": repositories })))
}

const ORG_SECURITY_SEVERITY_VALUES: [&str; 5] = ["critical", "high", "medium", "low", "info"];

async fn resolve_org_security_scope(
    db: &sqlx::PgPool,
    slug: &str,
    actor_user_id: Option<Uuid>,
) -> ApiResult<(Uuid, bool)> {
    let org_row = sqlx::query("SELECT id FROM organizations WHERE slug = $1")
        .bind(slug)
        .fetch_optional(db)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?
        .ok_or_else(|| ApiError(Error::NotFound(format!("Organization '{slug}' not found"))))?;

    let org_id: Uuid = org_row
        .try_get("id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    let can_view_non_public =
        actor_can_access_org_member_directory_by_id(db, org_id, actor_user_id).await?;

    Ok((org_id, can_view_non_public))
}

fn resolve_org_security_filters(
    query: &OrgSecurityFindingsQuery,
) -> ApiResult<ResolvedOrgSecurityFilters> {
    let severities = parse_org_security_severity_filters(query.severity.as_deref())?;
    let ecosystem = match normalize_optional_query_string(query.ecosystem.as_deref()) {
        Some(value) => Some(crate::routes::parse_ecosystem(&value)?.as_str().to_owned()),
        None => None,
    };

    Ok(ResolvedOrgSecurityFilters {
        severities,
        ecosystem,
        package: normalize_optional_query_string(query.package.as_deref()),
    })
}

fn parse_org_security_severity_filters(value: Option<&str>) -> ApiResult<Vec<String>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };

    let mut selected = BTreeSet::new();
    for segment in value.split(',') {
        let trimmed = segment.trim();
        if trimmed.is_empty() {
            continue;
        }

        selected.insert(normalize_org_security_severity(trimmed)?);
    }

    Ok(ORG_SECURITY_SEVERITY_VALUES
        .iter()
        .filter(|severity| {
            selected
                .iter()
                .any(|selected_severity| selected_severity == *severity)
        })
        .map(|severity| (*severity).to_owned())
        .collect())
}

fn normalize_org_security_severity(value: &str) -> ApiResult<String> {
    let normalized = value.trim().to_ascii_lowercase();
    if ORG_SECURITY_SEVERITY_VALUES.contains(&normalized.as_str()) {
        Ok(normalized)
    } else {
        Err(ApiError(Error::Validation(format!(
            "Unknown security severity filter: {value}"
        ))))
    }
}

fn build_org_security_query(
    org_id: Uuid,
    can_view_non_public: bool,
) -> QueryBuilder<'static, Postgres> {
    let mut builder = QueryBuilder::<Postgres>::new(
        "SELECT p.id, p.ecosystem, p.name, p.description, p.visibility::text AS visibility, \
                MAX(sf.detected_at) AS latest_detected_at, \
                COUNT(sf.id) FILTER (WHERE sf.severity = 'critical'::security_severity)::BIGINT AS critical_count, \
                COUNT(sf.id) FILTER (WHERE sf.severity = 'high'::security_severity)::BIGINT AS high_count, \
                COUNT(sf.id) FILTER (WHERE sf.severity = 'medium'::security_severity)::BIGINT AS medium_count, \
                COUNT(sf.id) FILTER (WHERE sf.severity = 'low'::security_severity)::BIGINT AS low_count, \
                COUNT(sf.id) FILTER (WHERE sf.severity = 'info'::security_severity)::BIGINT AS info_count \
         FROM packages p \
         JOIN repositories r ON r.id = p.repository_id \
         JOIN releases rel ON rel.package_id = p.id \
         JOIN security_findings sf ON sf.release_id = rel.id \
         WHERE p.owner_org_id = ",
    );
    builder.push_bind(org_id);
    builder
        .push(" AND sf.is_resolved = false AND (")
        .push_bind(can_view_non_public)
        .push("::bool = true OR (p.visibility = 'public' AND r.visibility = 'public'))");

    builder
}

fn apply_org_security_filters(
    builder: &mut QueryBuilder<'_, Postgres>,
    filters: &ResolvedOrgSecurityFilters,
) {
    if !filters.severities.is_empty() {
        builder.push(" AND sf.severity::text IN (");
        let mut is_first = true;
        for severity in &filters.severities {
            if !is_first {
                builder.push(", ");
            }
            is_first = false;
            builder.push_bind(severity.clone());
        }
        builder.push(")");
    }

    if let Some(ecosystem) = &filters.ecosystem {
        builder
            .push(" AND p.ecosystem = ")
            .push_bind(ecosystem.clone());
    }

    if let Some(package) = &filters.package {
        let pattern = format!("%{package}%");
        builder
            .push(" AND (p.name ILIKE ")
            .push_bind(pattern.clone())
            .push(" OR p.normalized_name ILIKE ")
            .push_bind(pattern)
            .push(")");
    }
}

async fn load_org_security_packages(
    db: &sqlx::PgPool,
    org_id: Uuid,
    can_view_non_public: bool,
    filters: &ResolvedOrgSecurityFilters,
) -> ApiResult<Vec<OrgSecurityPackageSummary>> {
    let mut builder = build_org_security_query(org_id, can_view_non_public);
    apply_org_security_filters(&mut builder, filters);
    builder.push(" GROUP BY p.id, p.ecosystem, p.name, p.description, p.visibility");

    let rows = builder
        .build()
        .fetch_all(db)
        .await
        .map_err(|e| ApiError(Error::Database(e)))?;

    let mut packages = rows
        .iter()
        .map(build_org_security_package_summary)
        .collect::<ApiResult<Vec<_>>>()?;

    packages.sort_by(|left, right| {
        right
            .worst_rank()
            .cmp(&left.worst_rank())
            .then_with(|| right.open_findings().cmp(&left.open_findings()))
            .then_with(|| left.ecosystem.cmp(&right.ecosystem))
            .then_with(|| left.name.cmp(&right.name))
    });

    Ok(packages)
}

fn summarize_org_security_packages(
    packages: &[OrgSecurityPackageSummary],
) -> SecuritySeverityCounts {
    let mut summary = SecuritySeverityCounts::default();
    for package in packages {
        summary.merge_from(&package.severities);
    }

    summary
}

fn build_org_security_csv(packages: &[OrgSecurityPackageSummary]) -> ApiResult<String> {
    let mut writer = WriterBuilder::new().from_writer(Vec::new());
    writer
        .write_record([
            "package_id",
            "ecosystem",
            "name",
            "description",
            "visibility",
            "open_findings",
            "worst_severity",
            "latest_detected_at",
            "critical_count",
            "high_count",
            "medium_count",
            "low_count",
            "info_count",
        ])
        .map_err(csv_write_error)?;

    for package in packages {
        writer
            .write_record([
                package.package_id.to_string(),
                package.ecosystem.clone(),
                package.name.clone(),
                package.description.clone().unwrap_or_default(),
                package.visibility.clone(),
                package.open_findings().to_string(),
                package.severities.worst_severity().to_owned(),
                package
                    .latest_detected_at
                    .map(|value| value.to_rfc3339())
                    .unwrap_or_default(),
                package.severities.critical.to_string(),
                package.severities.high.to_string(),
                package.severities.medium.to_string(),
                package.severities.low.to_string(),
                package.severities.info.to_string(),
            ])
            .map_err(csv_write_error)?;
    }

    let bytes = writer
        .into_inner()
        .map_err(|error| ApiError(Error::Internal(error.to_string())))?;

    String::from_utf8(bytes).map_err(|error| ApiError(Error::Internal(error.to_string())))
}

#[derive(Debug, Clone, Default)]
struct SecuritySeverityCounts {
    critical: i64,
    high: i64,
    medium: i64,
    low: i64,
    info: i64,
}

impl SecuritySeverityCounts {
    fn total(&self) -> i64 {
        self.critical + self.high + self.medium + self.low + self.info
    }

    fn worst_severity(&self) -> &'static str {
        if self.critical > 0 {
            "critical"
        } else if self.high > 0 {
            "high"
        } else if self.medium > 0 {
            "medium"
        } else if self.low > 0 {
            "low"
        } else {
            "info"
        }
    }

    fn worst_rank(&self) -> i32 {
        match self.worst_severity() {
            "critical" => 4,
            "high" => 3,
            "medium" => 2,
            "low" => 1,
            _ => 0,
        }
    }

    fn merge_from(&mut self, other: &Self) {
        self.critical += other.critical;
        self.high += other.high;
        self.medium += other.medium;
        self.low += other.low;
        self.info += other.info;
    }

    fn as_json(&self) -> serde_json::Value {
        serde_json::json!({
            "critical": self.critical,
            "high": self.high,
            "medium": self.medium,
            "low": self.low,
            "info": self.info,
        })
    }
}

#[derive(Debug, Clone)]
struct SecurityReviewerTeamSummary {
    team_id: Uuid,
    team_slug: String,
    team_name: String,
}

impl SecurityReviewerTeamSummary {
    fn as_json(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.team_id,
            "slug": self.team_slug,
            "name": self.team_name,
        })
    }
}

#[derive(Debug, Clone)]
struct OrgSecurityPackageSummary {
    package_id: Uuid,
    ecosystem: String,
    name: String,
    description: Option<String>,
    visibility: String,
    latest_detected_at: Option<chrono::DateTime<chrono::Utc>>,
    severities: SecuritySeverityCounts,
    reviewer_teams: Vec<SecurityReviewerTeamSummary>,
    can_manage_security: bool,
}

impl OrgSecurityPackageSummary {
    fn open_findings(&self) -> i64 {
        self.severities.total()
    }

    fn worst_rank(&self) -> i32 {
        self.severities.worst_rank()
    }

    fn as_json(&self) -> serde_json::Value {
        serde_json::json!({
            "package_id": self.package_id,
            "ecosystem": self.ecosystem,
            "name": self.name,
            "description": self.description,
            "visibility": self.visibility,
            "open_findings": self.open_findings(),
            "worst_severity": self.severities.worst_severity(),
            "latest_detected_at": self.latest_detected_at,
            "severities": self.severities.as_json(),
            "reviewer_teams": self.reviewer_teams.iter().map(SecurityReviewerTeamSummary::as_json).collect::<Vec<_>>(),
            "can_manage_security": self.can_manage_security,
        })
    }
}

fn identity_can_attempt_security_review(identity: &OptionalAuthenticatedIdentity) -> Option<Uuid> {
    identity
        .0
        .as_ref()
        .filter(|identity| {
            identity
                .scopes()
                .iter()
                .any(|scope| scope == SCOPE_PACKAGES_WRITE)
        })
        .map(|identity| identity.user_id)
}

async fn load_org_security_reviewer_teams(
    db: &sqlx::PgPool,
    org_id: Uuid,
    package_ids: &[Uuid],
) -> ApiResult<HashMap<Uuid, Vec<SecurityReviewerTeamSummary>>> {
    if package_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let rows = sqlx::query(
        "SELECT reviewer_grants.package_id, t.id AS team_id, t.slug AS team_slug, t.name AS team_name \
         FROM ( \
             SELECT DISTINCT tpa.package_id, tpa.team_id \
             FROM team_package_access tpa \
             WHERE tpa.package_id = ANY($1) \
               AND tpa.permission::text IN ('admin', 'security_review') \
             UNION \
             SELECT DISTINCT p.id AS package_id, tra.team_id \
             FROM packages p \
             JOIN team_repository_access tra ON tra.repository_id = p.repository_id \
             WHERE p.id = ANY($1) \
               AND tra.permission::text IN ('admin', 'security_review') \
         ) reviewer_grants \
         JOIN teams t ON t.id = reviewer_grants.team_id \
         WHERE t.org_id = $2 \
         ORDER BY reviewer_grants.package_id, LOWER(t.name), LOWER(t.slug)",
    )
    .bind(package_ids)
    .bind(org_id)
    .fetch_all(db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    let mut reviewer_teams = HashMap::<Uuid, Vec<SecurityReviewerTeamSummary>>::new();
    for row in rows {
        let package_id: Uuid = row
            .try_get("package_id")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?;
        let team = SecurityReviewerTeamSummary {
            team_id: row
                .try_get("team_id")
                .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
            team_slug: row
                .try_get("team_slug")
                .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
            team_name: row
                .try_get("team_name")
                .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        };
        reviewer_teams.entry(package_id).or_default().push(team);
    }

    Ok(reviewer_teams)
}

async fn load_actor_security_manageable_packages(
    db: &sqlx::PgPool,
    actor_user_id: Option<Uuid>,
    package_ids: &[Uuid],
) -> ApiResult<BTreeSet<Uuid>> {
    let Some(actor_user_id) = actor_user_id else {
        return Ok(BTreeSet::new());
    };
    if package_ids.is_empty() {
        return Ok(BTreeSet::new());
    }

    let rows = sqlx::query(
        "SELECT DISTINCT manageable.package_id \
         FROM ( \
             SELECT p.id AS package_id \
             FROM packages p \
             WHERE p.id = ANY($1) AND p.owner_user_id = $2 \
             UNION \
             SELECT p.id AS package_id \
             FROM packages p \
             JOIN org_memberships om ON om.org_id = p.owner_org_id \
             WHERE p.id = ANY($1) \
               AND om.user_id = $2 \
               AND om.role::text IN ('owner', 'admin') \
             UNION \
             SELECT tpa.package_id \
             FROM team_package_access tpa \
             JOIN team_memberships tm ON tm.team_id = tpa.team_id \
             WHERE tpa.package_id = ANY($1) \
               AND tm.user_id = $2 \
               AND tpa.permission::text IN ('admin', 'security_review') \
             UNION \
             SELECT p.id AS package_id \
             FROM packages p \
             JOIN team_repository_access tra ON tra.repository_id = p.repository_id \
             JOIN team_memberships tm ON tm.team_id = tra.team_id \
             WHERE p.id = ANY($1) \
               AND tm.user_id = $2 \
               AND tra.permission::text IN ('admin', 'security_review') \
         ) manageable",
    )
    .bind(package_ids)
    .bind(actor_user_id)
    .fetch_all(db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?;

    rows.into_iter()
        .map(|row| {
            row.try_get("package_id")
                .map_err(|e| ApiError(Error::Internal(e.to_string())))
        })
        .collect()
}

async fn enrich_org_security_packages(
    db: &sqlx::PgPool,
    org_id: Uuid,
    identity: &OptionalAuthenticatedIdentity,
    include_reviewer_teams: bool,
    packages: &mut [OrgSecurityPackageSummary],
) -> ApiResult<()> {
    let package_ids = packages
        .iter()
        .map(|package| package.package_id)
        .collect::<Vec<_>>();
    let reviewer_teams = if include_reviewer_teams {
        load_org_security_reviewer_teams(db, org_id, &package_ids).await?
    } else {
        HashMap::new()
    };
    let manageable_packages = load_actor_security_manageable_packages(
        db,
        identity_can_attempt_security_review(identity),
        &package_ids,
    )
    .await?;

    for package in packages {
        package.reviewer_teams = reviewer_teams
            .get(&package.package_id)
            .cloned()
            .unwrap_or_default();
        package.can_manage_security = manageable_packages.contains(&package.package_id);
    }

    Ok(())
}

async fn list_org_security_findings(
    State(state): State<AppState>,
    identity: OptionalAuthenticatedIdentity,
    Path(slug): Path<String>,
    Query(query): Query<OrgSecurityFindingsQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let (org_id, can_view_non_public) =
        resolve_org_security_scope(&state.db, &slug, identity.user_id()).await?;
    let filters = resolve_org_security_filters(&query)?;
    let mut packages =
        load_org_security_packages(&state.db, org_id, can_view_non_public, &filters).await?;
    enrich_org_security_packages(
        &state.db,
        org_id,
        &identity,
        can_view_non_public,
        &mut packages,
    )
    .await?;
    let summary_severities = summarize_org_security_packages(&packages);

    let package_count = packages.len();
    let packages = packages
        .iter()
        .map(OrgSecurityPackageSummary::as_json)
        .collect::<Vec<_>>();

    Ok(Json(serde_json::json!({
        "summary": {
            "open_findings": summary_severities.total(),
            "affected_packages": package_count,
            "severities": summary_severities.as_json(),
        },
        "packages": packages,
    })))
}

async fn export_org_security_findings_csv(
    State(state): State<AppState>,
    identity: OptionalAuthenticatedIdentity,
    Path(slug): Path<String>,
    Query(query): Query<OrgSecurityFindingsQuery>,
) -> ApiResult<impl IntoResponse> {
    let (org_id, can_view_non_public) =
        resolve_org_security_scope(&state.db, &slug, identity.user_id()).await?;
    let filters = resolve_org_security_filters(&query)?;
    let packages =
        load_org_security_packages(&state.db, org_id, can_view_non_public, &filters).await?;
    let csv_body = build_org_security_csv(&packages)?;

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/csv; charset=utf-8"),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(
            "attachment; filename=\"org-security-findings-{slug}.csv\""
        ))
        .map_err(|error| ApiError(Error::Internal(error.to_string())))?,
    );

    Ok((headers, csv_body))
}

fn build_org_security_package_summary(
    row: &sqlx::postgres::PgRow,
) -> ApiResult<OrgSecurityPackageSummary> {
    Ok(OrgSecurityPackageSummary {
        package_id: row
            .try_get("id")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        ecosystem: row
            .try_get("ecosystem")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        name: row
            .try_get("name")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        description: row
            .try_get::<Option<String>, _>("description")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        visibility: row
            .try_get("visibility")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        latest_detected_at: row
            .try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("latest_detected_at")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        severities: security_severity_counts_from_row(row)?,
        reviewer_teams: Vec::new(),
        can_manage_security: false,
    })
}

fn security_severity_counts_from_row(
    row: &sqlx::postgres::PgRow,
) -> ApiResult<SecuritySeverityCounts> {
    Ok(SecuritySeverityCounts {
        critical: row
            .try_get("critical_count")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        high: row
            .try_get("high_count")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        medium: row
            .try_get("medium_count")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        low: row
            .try_get("low_count")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        info: row
            .try_get("info_count")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
    })
}

async fn load_team_record(
    db: &sqlx::PgPool,
    org_id: Uuid,
    team_slug: &str,
) -> ApiResult<TeamRecord> {
    let row = sqlx::query(
        "SELECT id, org_id, name, slug, description \
         FROM teams \
         WHERE org_id = $1 AND slug = $2",
    )
    .bind(org_id)
    .bind(team_slug)
    .fetch_optional(db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| ApiError(Error::NotFound(format!("Team '{}' not found", team_slug))))?;

    Ok(TeamRecord {
        id: row
            .try_get("id")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        org_id: row
            .try_get("org_id")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        name: row
            .try_get("name")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        slug: row
            .try_get("slug")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        description: row
            .try_get::<Option<String>, _>("description")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
    })
}

async fn load_org_owned_package_for_team_access(
    db: &sqlx::PgPool,
    org_id: Uuid,
    ecosystem: &publaryn_core::domain::namespace::Ecosystem,
    package_name: &str,
) -> ApiResult<TeamPackageAccessTarget> {
    let normalized_name = normalize_package_name(package_name, ecosystem);
    let row = sqlx::query(
        "SELECT id, ecosystem, name, normalized_name, owner_org_id \
         FROM packages \
         WHERE ecosystem = $1 AND normalized_name = $2",
    )
    .bind(ecosystem.as_str())
    .bind(&normalized_name)
    .fetch_optional(db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| {
        ApiError(Error::NotFound(format!(
            "Package '{}' not found in ecosystem '{}'",
            package_name,
            ecosystem.as_str()
        )))
    })?;

    let owner_org_id = row
        .try_get::<Option<Uuid>, _>("owner_org_id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    if owner_org_id != Some(org_id) {
        return Err(ApiError(Error::Forbidden(
            "Teams can only be granted access to packages owned by the same organization".into(),
        )));
    }

    Ok(TeamPackageAccessTarget {
        id: row
            .try_get("id")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        ecosystem: row
            .try_get("ecosystem")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        name: row
            .try_get("name")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        normalized_name: row
            .try_get("normalized_name")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
    })
}

async fn load_org_owned_repository_for_team_access(
    db: &sqlx::PgPool,
    org_id: Uuid,
    repository_slug: &str,
) -> ApiResult<TeamRepositoryAccessTarget> {
    let row = sqlx::query(
        "SELECT id, name, slug, kind::text AS kind, visibility::text AS visibility, owner_org_id \
         FROM repositories \
         WHERE slug = $1",
    )
    .bind(repository_slug)
    .fetch_optional(db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| {
        ApiError(Error::NotFound(format!(
            "Repository '{}' not found",
            repository_slug
        )))
    })?;

    let owner_org_id = row
        .try_get::<Option<Uuid>, _>("owner_org_id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    if owner_org_id != Some(org_id) {
        return Err(ApiError(Error::Forbidden(
            "Teams can only be granted access to repositories owned by the same organization"
                .into(),
        )));
    }

    Ok(TeamRepositoryAccessTarget {
        id: row
            .try_get("id")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        name: row
            .try_get("name")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        slug: row
            .try_get("slug")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        kind: row
            .try_get("kind")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        visibility: row
            .try_get("visibility")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
    })
}

async fn load_org_owned_namespace_claim_for_team_access(
    db: &sqlx::PgPool,
    org_id: Uuid,
    claim_id: Uuid,
) -> ApiResult<TeamNamespaceAccessTarget> {
    let row = sqlx::query(
        "SELECT id, ecosystem::text AS ecosystem, namespace, is_verified, owner_org_id \
         FROM namespace_claims \
         WHERE id = $1",
    )
    .bind(claim_id)
    .fetch_optional(db)
    .await
    .map_err(|e| ApiError(Error::Database(e)))?
    .ok_or_else(|| {
        ApiError(Error::NotFound(format!(
            "Namespace claim '{claim_id}' not found"
        )))
    })?;

    let owner_org_id = row
        .try_get::<Option<Uuid>, _>("owner_org_id")
        .map_err(|e| ApiError(Error::Internal(e.to_string())))?;

    if owner_org_id != Some(org_id) {
        return Err(ApiError(Error::Forbidden(
            "Teams can only be granted access to namespace claims owned by the same organization"
                .into(),
        )));
    }

    Ok(TeamNamespaceAccessTarget {
        id: row
            .try_get("id")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        ecosystem: row
            .try_get("ecosystem")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        namespace: row
            .try_get("namespace")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
        is_verified: row
            .try_get("is_verified")
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?,
    })
}

fn normalize_team_permissions(input: &[String]) -> ApiResult<Vec<TeamPermission>> {
    if input.is_empty() {
        return Err(ApiError(Error::Validation(
            "At least one team permission is required".into(),
        )));
    }

    let mut seen = BTreeSet::new();
    let mut normalized = Vec::new();

    for raw_permission in input {
        let permission = TeamPermission::from_str(raw_permission).map_err(ApiError::from)?;
        if seen.insert(permission.as_str()) {
            normalized.push(permission);
        }
    }

    normalized.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    Ok(normalized)
}

fn normalize_namespace_team_permissions(input: &[String]) -> ApiResult<Vec<TeamPermission>> {
    let normalized = normalize_team_permissions(input)?;

    if normalized.iter().all(|permission| {
        matches!(
            permission,
            TeamPermission::Admin | TeamPermission::TransferOwnership
        )
    }) {
        Ok(normalized)
    } else {
        Err(ApiError(Error::Validation(
            "Namespace delegation only supports the 'admin' and 'transfer_ownership' permissions"
                .into(),
        )))
    }
}

fn team_permission_strings(permissions: &[TeamPermission]) -> Vec<String> {
    permissions
        .iter()
        .map(|permission| permission.as_str().to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use publaryn_core::domain::{organization::OrgRole, team::TeamPermission};

    use super::{
        collect_org_profile_changes, normalize_namespace_team_permissions,
        normalize_optional_org_field, normalize_team_permissions, resolve_org_profile_update,
        resolve_org_security_filters, validate_ownership_transfer, OrgSecurityFindingsQuery,
        ResolvedOrgProfileUpdate, UpdateOrgRequest,
    };

    #[test]
    fn normalize_optional_org_field_trims_and_clears_blank_values() {
        assert_eq!(
            normalize_optional_org_field(Some("  https://packages.example.com  ".into())),
            Some("https://packages.example.com".into())
        );
        assert_eq!(normalize_optional_org_field(Some("   ".into())), None);
        assert_eq!(normalize_optional_org_field(None), None);
    }

    #[test]
    fn resolve_org_profile_update_preserves_existing_fields_and_normalizes_mfa_policy() {
        let current = ResolvedOrgProfileUpdate {
            description: Some("Current description".into()),
            website: Some("https://packages.example.com".into()),
            email: Some("team@example.com".into()),
            mfa_required: false,
        };

        let updated = resolve_org_profile_update(
            &current,
            &UpdateOrgRequest {
                description: Some(Some("  Updated description  ".into())),
                website: None,
                email: Some(Some("   ".into())),
                mfa_required: Some(true),
            },
        );

        assert_eq!(
            updated,
            ResolvedOrgProfileUpdate {
                description: Some("Updated description".into()),
                website: Some("https://packages.example.com".into()),
                email: None,
                mfa_required: true,
            }
        );
    }

    #[test]
    fn collect_org_profile_changes_tracks_mfa_policy_changes() {
        let current = ResolvedOrgProfileUpdate {
            description: Some("Current description".into()),
            website: Some("https://packages.example.com".into()),
            email: Some("team@example.com".into()),
            mfa_required: false,
        };
        let updated = ResolvedOrgProfileUpdate {
            description: Some("Current description".into()),
            website: Some("https://packages.example.com".into()),
            email: Some("team@example.com".into()),
            mfa_required: true,
        };

        let changes = collect_org_profile_changes(&current, &updated);

        assert_eq!(
            changes.get("mfa_required"),
            Some(&serde_json::json!({
                "before": false,
                "after": true,
            }))
        );
    }

    #[test]
    fn ownership_transfer_requires_current_owner() {
        let error = validate_ownership_transfer(
            Uuid::new_v4(),
            &OrgRole::Admin,
            Uuid::new_v4(),
            &OrgRole::Viewer,
        )
        .expect_err("non-owners must not transfer ownership");

        assert_eq!(
            error.0.to_string(),
            "Forbidden: Transferring organization ownership requires an owner membership"
        );
    }

    #[test]
    fn ownership_transfer_rejects_self_transfer() {
        let actor_id = Uuid::new_v4();
        let error =
            validate_ownership_transfer(actor_id, &OrgRole::Owner, actor_id, &OrgRole::Owner)
                .expect_err("self-transfer must be rejected");

        assert_eq!(
            error.0.to_string(),
            "Validation error: Ownership transfer target must be a different organization member"
        );
    }

    #[test]
    fn ownership_transfer_rejects_existing_owner_target() {
        let error = validate_ownership_transfer(
            Uuid::new_v4(),
            &OrgRole::Owner,
            Uuid::new_v4(),
            &OrgRole::Owner,
        )
        .expect_err("transferring to another owner should fail");

        assert_eq!(
            error.0.to_string(),
            "Conflict: The selected member is already an organization owner"
        );
    }

    #[test]
    fn ownership_transfer_accepts_non_owner_target() {
        validate_ownership_transfer(
            Uuid::new_v4(),
            &OrgRole::Owner,
            Uuid::new_v4(),
            &OrgRole::Admin,
        )
        .expect("owner should be able to transfer to an existing non-owner member");
    }

    #[test]
    fn normalize_team_permissions_sorts_and_deduplicates() {
        let permissions = normalize_team_permissions(&[
            "publish".to_owned(),
            "admin".to_owned(),
            "publish".to_owned(),
        ])
        .expect("permissions should normalize");

        assert_eq!(
            permissions,
            vec![TeamPermission::Admin, TeamPermission::Publish]
        );
    }

    #[test]
    fn normalize_team_permissions_rejects_empty_inputs() {
        let error =
            normalize_team_permissions(&[]).expect_err("empty permission lists must be rejected");

        assert_eq!(
            error.0.to_string(),
            "Validation error: At least one team permission is required"
        );
    }

    #[test]
    fn normalize_team_permissions_rejects_unknown_values() {
        let error = normalize_team_permissions(&["superpowers".to_owned()])
            .expect_err("unknown permissions must be rejected");

        assert_eq!(
            error.0.to_string(),
            "Validation error: Unknown team permission: superpowers"
        );
    }

    #[test]
    fn normalize_namespace_team_permissions_rejects_publish() {
        let error = normalize_namespace_team_permissions(&["publish".to_owned()])
            .expect_err("namespace grants should reject package-only permissions");

        assert_eq!(
            error.0.to_string(),
            "Validation error: Namespace delegation only supports the 'admin' and 'transfer_ownership' permissions"
        );
    }

    #[test]
    fn resolve_org_security_filters_normalizes_supported_values() {
        let filters = resolve_org_security_filters(&OrgSecurityFindingsQuery {
            severity: Some("low, critical, low".into()),
            ecosystem: Some("bun".into()),
            package: Some("  widget  ".into()),
        })
        .expect("security filters should normalize");

        assert_eq!(filters.severities, vec!["critical", "low"]);
        assert_eq!(filters.ecosystem.as_deref(), Some("npm"));
        assert_eq!(filters.package.as_deref(), Some("widget"));
    }

    #[test]
    fn resolve_org_security_filters_rejects_unknown_severity_values() {
        let error = resolve_org_security_filters(&OrgSecurityFindingsQuery {
            severity: Some("catastrophic".into()),
            ecosystem: None,
            package: None,
        })
        .expect_err("unknown severities must be rejected");

        assert_eq!(
            error.0.to_string(),
            "Validation error: Unknown security severity filter: catastrophic"
        );
    }
}
