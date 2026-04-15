use sqlx::PgPool;

/// Run all pending migrations on the given pool.
/// Intended for per-test databases provisioned via `sqlx::test`.
pub async fn run_migrations(pool: &PgPool) {
    sqlx::migrate!("../../migrations")
        .run(pool)
        .await
        .expect("Failed to run database migrations");
}

/// Truncate all data tables (not schema/migration tables) for test isolation.
/// Uses TRUNCATE ... CASCADE for speed instead of per-row DELETE.
pub async fn truncate_all(pool: &PgPool) {
    sqlx::query(
        "TRUNCATE
            users,
            organizations,
            org_memberships,
            teams,
            team_members,
            team_package_access,
            org_invitations,
            namespace_claims,
            repositories,
            packages,
            releases,
            artifacts,
            channel_refs,
            tokens,
            trusted_publishers,
            security_findings,
            audit_logs
        CASCADE",
    )
    .execute(pool)
    .await
    .expect("Failed to truncate test tables");
}
