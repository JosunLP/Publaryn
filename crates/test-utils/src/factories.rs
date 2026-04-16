use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

use publaryn_core::domain::namespace::Ecosystem;
use publaryn_core::domain::package::normalize_package_name;
use publaryn_core::domain::user::User;

/// Insert a test user into the database and return the domain User.
/// Generates a random username and email to avoid collisions.
pub async fn create_test_user(pool: &PgPool) -> User {
    create_test_user_with(pool, None, None).await
}

/// Insert a test user with an optional custom username and/or admin flag.
pub async fn create_test_user_with(
    pool: &PgPool,
    username: Option<&str>,
    is_admin: Option<bool>,
) -> User {
    let suffix = &Uuid::new_v4().to_string()[..8];
    let username = username
        .map(|u| u.to_owned())
        .unwrap_or_else(|| format!("testuser-{suffix}"));
    let email = format!("{username}@test.publaryn.dev");
    let password_hash =
        publaryn_auth::hash_password("TestPassword123!").expect("hash should succeed");
    let is_admin = is_admin.unwrap_or(false);

    let user = User::new(username, email, Some(password_hash));

    sqlx::query(
        "INSERT INTO users (id, username, email, password_hash, is_admin, is_active,
         email_verified, mfa_enabled, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, true, false, false, $6, $6)",
    )
    .bind(user.id)
    .bind(&user.username)
    .bind(&user.email)
    .bind(&user.password_hash)
    .bind(is_admin)
    .bind(user.created_at)
    .execute(pool)
    .await
    .expect("Failed to insert test user");

    user
}

/// Insert a test organization owned by the given user.
/// Returns `(org_id, org_slug)`.
pub async fn create_test_org(pool: &PgPool, owner_id: Uuid) -> (Uuid, String) {
    let suffix = &Uuid::new_v4().to_string()[..8];
    let slug = format!("testorg-{suffix}");
    let org_id = Uuid::new_v4();
    let now = Utc::now();

    sqlx::query(
        "INSERT INTO organizations (id, name, slug, display_name, is_verified, mfa_required, created_at, updated_at)
         VALUES ($1, $2, $2, $2, false, false, $3, $3)",
    )
    .bind(org_id)
    .bind(&slug)
    .bind(now)
    .execute(pool)
    .await
    .expect("Failed to insert test organization");

    // Add owner membership
    sqlx::query(
        "INSERT INTO org_memberships (id, organization_id, user_id, role, created_at)
         VALUES ($1, $2, $3, 'owner', $4)",
    )
    .bind(Uuid::new_v4())
    .bind(org_id)
    .bind(owner_id)
    .bind(now)
    .execute(pool)
    .await
    .expect("Failed to insert org owner membership");

    (org_id, slug)
}

/// Insert a test repository and return `(repo_id, repo_slug)`.
pub async fn create_test_repository(pool: &PgPool, org_id: Uuid) -> (Uuid, String) {
    let suffix = &Uuid::new_v4().to_string()[..8];
    let slug = format!("testrepo-{suffix}");
    let repo_id = Uuid::new_v4();
    let now = Utc::now();

    sqlx::query(
        "INSERT INTO repositories (id, organization_id, name, slug, kind, visibility, created_at, updated_at)
         VALUES ($1, $2, $3, $3, 'public', 'public', $4, $4)",
    )
    .bind(repo_id)
    .bind(org_id)
    .bind(&slug)
    .bind(now)
    .execute(pool)
    .await
    .expect("Failed to insert test repository");

    (repo_id, slug)
}

/// Insert a test package in the given repository.
/// Returns the package id.
pub async fn create_test_package(
    pool: &PgPool,
    repository_id: Uuid,
    ecosystem: Ecosystem,
    name: &str,
    owner_user_id: Option<Uuid>,
    owner_org_id: Option<Uuid>,
) -> Uuid {
    let package_id = Uuid::new_v4();
    let normalized = normalize_package_name(name, &ecosystem);
    let now = Utc::now();

    sqlx::query(
        "INSERT INTO packages (id, repository_id, ecosystem, name, normalized_name,
         visibility, owner_user_id, owner_org_id, is_deprecated, is_archived,
         download_count, keywords, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, 'public', $6, $7, false, false, 0, '{}', $8, $8)",
    )
    .bind(package_id)
    .bind(repository_id)
    .bind(&ecosystem)
    .bind(name)
    .bind(&normalized)
    .bind(owner_user_id)
    .bind(owner_org_id)
    .bind(now)
    .execute(pool)
    .await
    .expect("Failed to insert test package");

    package_id
}

/// Insert a test API token for a user. Returns `(token_id, raw_token_value)`.
/// The raw value is needed to set Bearer auth in tests.
pub async fn create_test_token(pool: &PgPool, user_id: Uuid, scopes: &[&str]) -> (Uuid, String) {
    let token_id = Uuid::new_v4();
    let raw_token = format!("pub_{}", publaryn_core::security::generate_random_token(24));
    let token_hash = publaryn_core::security::hash_token(&raw_token);
    let now = Utc::now();
    let scope_vec: Vec<String> = scopes.iter().map(|s| (*s).to_owned()).collect();

    sqlx::query(
        "INSERT INTO tokens (id, kind, prefix, token_hash, name, user_id,
         scopes, is_revoked, created_at)
         VALUES ($1, 'personal', 'pub_', $2, 'test-token', $3, $4, false, $5)",
    )
    .bind(token_id)
    .bind(&token_hash)
    .bind(user_id)
    .bind(&scope_vec)
    .bind(now)
    .execute(pool)
    .await
    .expect("Failed to insert test token");

    (token_id, raw_token)
}
