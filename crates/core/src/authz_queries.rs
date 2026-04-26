use sqlx::PgPool;
use uuid::Uuid;

pub async fn actor_has_any_team_package_access(
    db: &PgPool,
    package_id: Uuid,
    actor_user_id: Uuid,
) -> sqlx::Result<bool> {
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
}

pub async fn actor_has_any_team_repository_access(
    db: &PgPool,
    repository_id: Uuid,
    actor_user_id: Uuid,
) -> sqlx::Result<bool> {
    sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (\
             SELECT 1 \
             FROM team_repository_access tra \
             JOIN team_memberships tm ON tm.team_id = tra.team_id \
             JOIN teams t ON t.id = tra.team_id \
             JOIN repositories r ON r.id = tra.repository_id \
             WHERE tra.repository_id = $1 \
               AND tm.user_id = $2 \
               AND t.org_id = r.owner_org_id\
         )",
    )
    .bind(repository_id)
    .bind(actor_user_id)
    .fetch_one(db)
    .await
}
