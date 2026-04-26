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

#[cfg(test)]
mod tests {
    use super::{actor_has_any_team_package_access, actor_has_any_team_repository_access};
    use sqlx::PgPool;
    use uuid::Uuid;

    #[sqlx::test(migrations = "../../migrations")]
    async fn package_access_requires_team_to_belong_to_package_owner_org(pool: PgPool) {
        let actor_same_org = insert_user(&pool, "actor-same-org").await;
        let actor_other_org = insert_user(&pool, "actor-other-org").await;
        let owner_org = insert_org(&pool, "owner-org").await;
        let other_org = insert_org(&pool, "other-org").await;
        let owner_team = insert_team(&pool, owner_org, "owner-team").await;
        let outside_team = insert_team(&pool, other_org, "outside-team").await;
        let repository_id = insert_repository(&pool, owner_org, "owner-repository").await;
        let package_id = insert_package(&pool, repository_id, owner_org, "owner-package").await;

        insert_team_membership(&pool, owner_team, actor_same_org).await;
        insert_team_membership(&pool, outside_team, actor_other_org).await;
        insert_team_package_access(&pool, owner_team, package_id).await;
        insert_team_package_access(&pool, outside_team, package_id).await;

        assert!(
            actor_has_any_team_package_access(&pool, package_id, actor_same_org)
                .await
                .expect("same-org access query should succeed")
        );
        assert!(
            !actor_has_any_team_package_access(&pool, package_id, actor_other_org)
                .await
                .expect("cross-org access query should succeed")
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn repository_access_requires_team_to_belong_to_repository_owner_org(pool: PgPool) {
        let actor_same_org = insert_user(&pool, "repo-actor-same-org").await;
        let actor_other_org = insert_user(&pool, "repo-actor-other-org").await;
        let owner_org = insert_org(&pool, "repo-owner-org").await;
        let other_org = insert_org(&pool, "repo-other-org").await;
        let owner_team = insert_team(&pool, owner_org, "repo-owner-team").await;
        let outside_team = insert_team(&pool, other_org, "repo-outside-team").await;
        let repository_id = insert_repository(&pool, owner_org, "delegated-repository").await;

        insert_team_membership(&pool, owner_team, actor_same_org).await;
        insert_team_membership(&pool, outside_team, actor_other_org).await;
        insert_team_repository_access(&pool, owner_team, repository_id).await;
        insert_team_repository_access(&pool, outside_team, repository_id).await;

        assert!(
            actor_has_any_team_repository_access(&pool, repository_id, actor_same_org)
                .await
                .expect("same-org repository access query should succeed")
        );
        assert!(
            !actor_has_any_team_repository_access(&pool, repository_id, actor_other_org)
                .await
                .expect("cross-org repository access query should succeed")
        );
    }

    async fn insert_user(pool: &PgPool, username: &str) -> Uuid {
        let user_id = Uuid::new_v4();
        let email = format!("{username}@example.test");

        sqlx::query(
            "INSERT INTO users (id, username, email, password_hash, created_at, updated_at) \
             VALUES ($1, $2, $3, 'test-password-hash', NOW(), NOW())",
        )
        .bind(user_id)
        .bind(username)
        .bind(email)
        .execute(pool)
        .await
        .expect("user insert should succeed");

        user_id
    }

    async fn insert_org(pool: &PgPool, slug: &str) -> Uuid {
        let org_id = Uuid::new_v4();

        sqlx::query(
            "INSERT INTO organizations (id, name, slug, display_name, created_at, updated_at) \
             VALUES ($1, $2, $3, $2, NOW(), NOW())",
        )
        .bind(org_id)
        .bind(slug)
        .bind(slug)
        .execute(pool)
        .await
        .expect("organization insert should succeed");

        org_id
    }

    async fn insert_team(pool: &PgPool, org_id: Uuid, slug: &str) -> Uuid {
        let team_id = Uuid::new_v4();

        sqlx::query(
            "INSERT INTO teams (id, org_id, name, slug, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, NOW(), NOW())",
        )
        .bind(team_id)
        .bind(org_id)
        .bind(slug)
        .bind(slug)
        .execute(pool)
        .await
        .expect("team insert should succeed");

        team_id
    }

    async fn insert_team_membership(pool: &PgPool, team_id: Uuid, user_id: Uuid) {
        sqlx::query(
            "INSERT INTO team_memberships (id, team_id, user_id, added_at) \
             VALUES ($1, $2, $3, NOW())",
        )
        .bind(Uuid::new_v4())
        .bind(team_id)
        .bind(user_id)
        .execute(pool)
        .await
        .expect("team membership insert should succeed");
    }

    async fn insert_repository(pool: &PgPool, owner_org_id: Uuid, slug: &str) -> Uuid {
        let repository_id = Uuid::new_v4();

        sqlx::query(
            "INSERT INTO repositories (id, name, slug, kind, visibility, owner_org_id, created_at, updated_at) \
             VALUES ($1, $2, $3, 'public', 'private', $4, NOW(), NOW())",
        )
        .bind(repository_id)
        .bind(slug)
        .bind(slug)
        .bind(owner_org_id)
        .execute(pool)
        .await
        .expect("repository insert should succeed");

        repository_id
    }

    async fn insert_package(
        pool: &PgPool,
        repository_id: Uuid,
        owner_org_id: Uuid,
        name: &str,
    ) -> Uuid {
        let package_id = Uuid::new_v4();

        sqlx::query(
            "INSERT INTO packages (id, repository_id, ecosystem, name, normalized_name, visibility, owner_org_id, created_at, updated_at) \
             VALUES ($1, $2, 'npm', $3, $4, 'private', $5, NOW(), NOW())",
        )
        .bind(package_id)
        .bind(repository_id)
        .bind(name)
        .bind(name)
        .bind(owner_org_id)
        .execute(pool)
        .await
        .expect("package insert should succeed");

        package_id
    }

    async fn insert_team_package_access(pool: &PgPool, team_id: Uuid, package_id: Uuid) {
        sqlx::query(
            "INSERT INTO team_package_access (id, team_id, package_id, permission, granted_at) \
             VALUES ($1, $2, $3, 'read_private', NOW())",
        )
        .bind(Uuid::new_v4())
        .bind(team_id)
        .bind(package_id)
        .execute(pool)
        .await
        .expect("team package access insert should succeed");
    }

    async fn insert_team_repository_access(pool: &PgPool, team_id: Uuid, repository_id: Uuid) {
        sqlx::query(
            "INSERT INTO team_repository_access (id, team_id, repository_id, permission, granted_at) \
             VALUES ($1, $2, $3, 'read_private', NOW())",
        )
        .bind(Uuid::new_v4())
        .bind(team_id)
        .bind(repository_id)
        .execute(pool)
        .await
        .expect("team repository access insert should succeed");
    }
}
