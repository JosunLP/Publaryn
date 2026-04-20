use sqlx::{PgPool, Row};
use uuid::Uuid;

use publaryn_core::error::Error;
use publaryn_search::{PackageDocument, SearchIndex};

const RELEASE_HISTORY_VISIBLE_STATUSES: &[&str] = &["published", "deprecated", "yanked"];

pub async fn reindex_package_document(
    db: &PgPool,
    search: &(dyn SearchIndex + Send + Sync),
    package_id: Uuid,
) -> publaryn_core::Result<()> {
    let row = sqlx::query(
        "SELECT p.id, p.name, p.normalized_name, p.display_name, p.description, p.ecosystem, \
                p.keywords, p.download_count, p.is_deprecated, p.visibility, p.updated_at, \
                u.username AS owner_username, o.slug AS owner_org_slug, \
                r.name AS repository_name, r.slug AS repository_slug, \
                latest_release.version AS latest_version \
          FROM packages p \
          JOIN repositories r ON r.id = p.repository_id \
          LEFT JOIN users u ON u.id = p.owner_user_id \
          LEFT JOIN organizations o ON o.id = p.owner_org_id \
          LEFT JOIN LATERAL ( \
             SELECT version \
             FROM releases \
             WHERE package_id = p.id AND status::text = ANY($2) \
             ORDER BY published_at DESC \
             LIMIT 1 \
         ) latest_release ON true \
         WHERE p.id = $1",
    )
    .bind(package_id)
    .bind(RELEASE_HISTORY_VISIBLE_STATUSES)
    .fetch_optional(db)
    .await
    .map_err(Error::Database)?
    .ok_or_else(|| Error::NotFound(format!("Package '{package_id}' not found for indexing")))?;

    let owner_name = row
        .try_get::<Option<String>, _>("owner_org_slug")
        .map_err(|e| Error::Internal(e.to_string()))?
        .or_else(|| {
            row.try_get::<Option<String>, _>("owner_username")
                .ok()
                .flatten()
        });

    let document = PackageDocument {
        id: row
            .try_get::<Uuid, _>("id")
            .map_err(|e| Error::Internal(e.to_string()))?
            .to_string(),
        name: row
            .try_get("name")
            .map_err(|e| Error::Internal(e.to_string()))?,
        normalized_name: row
            .try_get("normalized_name")
            .map_err(|e| Error::Internal(e.to_string()))?,
        display_name: row
            .try_get::<Option<String>, _>("display_name")
            .map_err(|e| Error::Internal(e.to_string()))?,
        description: row
            .try_get::<Option<String>, _>("description")
            .map_err(|e| Error::Internal(e.to_string()))?,
        ecosystem: row
            .try_get::<String, _>("ecosystem")
            .map_err(|e| Error::Internal(e.to_string()))?,
        keywords: row
            .try_get::<Vec<String>, _>("keywords")
            .map_err(|e| Error::Internal(e.to_string()))?,
        latest_version: row
            .try_get::<Option<String>, _>("latest_version")
            .map_err(|e| Error::Internal(e.to_string()))?,
        download_count: row
            .try_get::<i64, _>("download_count")
            .map_err(|e| Error::Internal(e.to_string()))?,
        is_deprecated: row
            .try_get::<bool, _>("is_deprecated")
            .map_err(|e| Error::Internal(e.to_string()))?,
        visibility: row
            .try_get::<String, _>("visibility")
            .map_err(|e| Error::Internal(e.to_string()))?,
        owner_name,
        repository_name: row
            .try_get::<Option<String>, _>("repository_name")
            .map_err(|e| Error::Internal(e.to_string()))?,
        repository_slug: row
            .try_get::<Option<String>, _>("repository_slug")
            .map_err(|e| Error::Internal(e.to_string()))?,
        updated_at: row
            .try_get::<chrono::DateTime<chrono::Utc>, _>("updated_at")
            .map_err(|e| Error::Internal(e.to_string()))?
            .to_rfc3339(),
    };

    search.index_package(document).await
}
