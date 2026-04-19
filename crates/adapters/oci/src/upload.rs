use sqlx::{PgPool, Row};
use uuid::Uuid;

use publaryn_core::error::{Error, Result};

use crate::name::digest_hex;

const AUTO_CREATE_ALLOWED_REPOSITORY_KINDS: &[&str] = &["public", "private", "staging", "release"];
const ORG_REPOSITORY_WRITE_ROLES: &[&str] = &["owner", "admin"];
const TEAM_REPOSITORY_CREATE_PERMISSIONS: &[&str] = &["admin", "publish", "write_metadata"];

#[derive(Debug, Clone)]
pub struct RepoInfo {
    pub id: Uuid,
    pub visibility: String,
    pub owner_user_id: Option<Uuid>,
    pub owner_org_id: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub struct UploadSessionRecord {
    pub id: Uuid,
    pub repository_id: Uuid,
    pub package_name: String,
    pub created_by: Uuid,
    pub storage_key: String,
    pub received_bytes: i64,
}

pub fn blob_storage_key(digest: &str) -> String {
    let hex = digest_hex(digest).unwrap_or(digest);
    format!("oci/blobs/sha256/{hex}")
}

pub fn manifest_storage_key(release_id: Uuid, digest: &str) -> String {
    format!("oci/manifests/{release_id}/{digest}")
}

pub fn manifest_filename(digest: &str) -> String {
    format!("{}.json", digest.replace(':', "_"))
}

pub fn upload_session_storage_key(session_id: Uuid) -> String {
    format!("oci/uploads/{session_id}")
}

pub async fn select_default_repository(db: &PgPool, actor_user_id: Uuid) -> Result<RepoInfo> {
    let row = sqlx::query(
        "SELECT id, visibility::text AS visibility, owner_user_id, owner_org_id \
         FROM repositories \
         WHERE kind::text = ANY($1) \
           AND (\
               owner_user_id = $2 \
               OR EXISTS (\
                   SELECT 1 \
                   FROM org_memberships om \
                   WHERE om.org_id = repositories.owner_org_id \
                     AND om.user_id = $2 \
                     AND om.role::text = ANY($3)\
               ) \
               OR EXISTS (\
                   SELECT 1 \
                   FROM team_repository_access tra \
                   JOIN team_memberships tm ON tm.team_id = tra.team_id \
                   JOIN teams t ON t.id = tra.team_id \
                   WHERE tra.repository_id = repositories.id \
                     AND tm.user_id = $2 \
                     AND t.org_id = repositories.owner_org_id \
                     AND tra.permission::text = ANY($4)\
               )\
           ) \
         ORDER BY (owner_user_id = $2) DESC NULLS LAST, created_at ASC \
         LIMIT 1",
    )
    .bind(
        AUTO_CREATE_ALLOWED_REPOSITORY_KINDS
            .iter()
            .map(|kind| (*kind).to_owned())
            .collect::<Vec<_>>(),
    )
    .bind(actor_user_id)
    .bind(
        ORG_REPOSITORY_WRITE_ROLES
            .iter()
            .map(|role| (*role).to_owned())
            .collect::<Vec<_>>(),
    )
    .bind(
        TEAM_REPOSITORY_CREATE_PERMISSIONS
            .iter()
            .map(|permission| (*permission).to_owned())
            .collect::<Vec<_>>(),
    )
    .fetch_optional(db)
    .await
    .map_err(Error::Database)?
    .ok_or_else(|| {
        Error::Forbidden(
            "You have no repository suitable for OCI pushes. Create one via the Publaryn API or ask an administrator to delegate access first.".into(),
        )
    })?;

    Ok(RepoInfo {
        id: row.try_get("id").unwrap_or_default(),
        visibility: row
            .try_get("visibility")
            .unwrap_or_else(|_| "public".into()),
        owner_user_id: row.try_get("owner_user_id").unwrap_or(None),
        owner_org_id: row.try_get("owner_org_id").unwrap_or(None),
    })
}

pub async fn begin_upload_session(
    db: &PgPool,
    repository_id: Uuid,
    package_name: &str,
    created_by: Uuid,
) -> Result<UploadSessionRecord> {
    let session_id = Uuid::new_v4();
    let storage_key = upload_session_storage_key(session_id);

    sqlx::query(
        "INSERT INTO oci_upload_sessions \
         (id, repository_id, package_name, created_by, storage_key, received_bytes, updated_at) \
         VALUES ($1, $2, $3, $4, $5, 0, NOW())",
    )
    .bind(session_id)
    .bind(repository_id)
    .bind(package_name)
    .bind(created_by)
    .bind(&storage_key)
    .execute(db)
    .await
    .map_err(Error::Database)?;

    Ok(UploadSessionRecord {
        id: session_id,
        repository_id,
        package_name: package_name.to_owned(),
        created_by,
        storage_key,
        received_bytes: 0,
    })
}

pub async fn load_upload_session(
    db: &PgPool,
    session_id: Uuid,
) -> Result<Option<UploadSessionRecord>> {
    let row = sqlx::query(
        "SELECT id, repository_id, package_name, created_by, storage_key, received_bytes \
         FROM oci_upload_sessions \
         WHERE id = $1",
    )
    .bind(session_id)
    .fetch_optional(db)
    .await
    .map_err(Error::Database)?;

    Ok(row.map(|row| UploadSessionRecord {
        id: row.try_get("id").unwrap_or_default(),
        repository_id: row.try_get("repository_id").unwrap_or_default(),
        package_name: row.try_get("package_name").unwrap_or_default(),
        created_by: row.try_get("created_by").unwrap_or_default(),
        storage_key: row.try_get("storage_key").unwrap_or_default(),
        received_bytes: row.try_get("received_bytes").unwrap_or(0_i64),
    }))
}

pub async fn update_upload_session_received_bytes(
    db: &PgPool,
    session_id: Uuid,
    received_bytes: i64,
) -> Result<()> {
    sqlx::query(
        "UPDATE oci_upload_sessions \
         SET received_bytes = $1, updated_at = NOW() \
         WHERE id = $2",
    )
    .bind(received_bytes)
    .bind(session_id)
    .execute(db)
    .await
    .map_err(Error::Database)?;

    Ok(())
}

pub async fn delete_upload_session(db: &PgPool, session_id: Uuid) -> Result<()> {
    sqlx::query("DELETE FROM oci_upload_sessions WHERE id = $1")
        .bind(session_id)
        .execute(db)
        .await
        .map_err(Error::Database)?;

    Ok(())
}
