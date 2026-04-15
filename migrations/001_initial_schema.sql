-- ============================================================
-- Migration 001: Core schema for Publaryn
-- ============================================================

-- Enable extensions
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
CREATE EXTENSION IF NOT EXISTS "pg_trgm";
CREATE EXTENSION IF NOT EXISTS "citext";

-- ────────────────────────────────────────────────────────────
-- Enum types
-- ────────────────────────────────────────────────────────────

CREATE TYPE org_role AS ENUM (
    'owner', 'admin', 'maintainer', 'publisher',
    'security_manager', 'auditor', 'billing_manager', 'viewer'
);

CREATE TYPE ecosystem AS ENUM (
    'npm', 'bun', 'pypi', 'composer', 'nuget',
    'rubygems', 'maven', 'oci', 'cargo'
);

CREATE TYPE visibility AS ENUM (
    'public', 'private', 'internal_org', 'unlisted', 'quarantined'
);

CREATE TYPE repository_kind AS ENUM (
    'public', 'private', 'staging', 'release', 'proxy', 'virtual'
);

CREATE TYPE release_status AS ENUM (
    'quarantine', 'scanning', 'published', 'deprecated', 'yanked', 'deleted'
);

CREATE TYPE artifact_kind AS ENUM (
    'tarball', 'wheel', 'sdist', 'jar', 'pom', 'gem', 'nupkg', 'snupkg',
    'oci_manifest', 'oci_layer', 'crate', 'composer_zip',
    'checksum', 'signature', 'sbom', 'source_zip'
);

CREATE TYPE token_kind AS ENUM (
    'personal', 'org_automation', 'repository', 'package', 'ci', 'publish', 'oidc_derived'
);

CREATE TYPE audit_action AS ENUM (
    'package_create', 'package_delete', 'package_transfer', 'package_visibility_change',
    'release_publish', 'release_yank', 'release_unyank', 'release_deprecate',
    'user_login', 'user_logout', 'user_register', 'mfa_enable', 'mfa_disable',
    'token_create', 'token_revoke',
    'org_create', 'org_delete', 'org_member_add', 'org_member_remove', 'org_role_change',
    'team_create', 'team_delete', 'team_member_add', 'team_member_remove',
    'namespace_claim_create', 'namespace_claim_transfer',
    'security_finding_create', 'security_finding_resolve',
    'policy_change', 'sso_config_change'
);

CREATE TYPE security_severity AS ENUM ('info', 'low', 'medium', 'high', 'critical');

CREATE TYPE finding_kind AS ENUM (
    'vulnerability', 'malware', 'policy_violation', 'secrets_exposed',
    'suspicious_install_hook', 'archive_bomb', 'file_type_anomaly', 'dependency_confusion'
);

CREATE TYPE team_permission AS ENUM (
    'admin', 'publish', 'write_metadata', 'read_private', 'security_review', 'transfer_ownership'
);

-- ────────────────────────────────────────────────────────────
-- Users
-- ────────────────────────────────────────────────────────────

CREATE TABLE users (
    id               UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    username         CITEXT      NOT NULL UNIQUE,
    email            CITEXT      NOT NULL UNIQUE,
    password_hash    TEXT,
    display_name     TEXT,
    avatar_url       TEXT,
    bio              TEXT,
    website          TEXT,
    is_admin         BOOLEAN     NOT NULL DEFAULT FALSE,
    is_active        BOOLEAN     NOT NULL DEFAULT TRUE,
    email_verified   BOOLEAN     NOT NULL DEFAULT FALSE,
    mfa_enabled      BOOLEAN     NOT NULL DEFAULT FALSE,
    mfa_totp_secret  TEXT,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_users_username ON users (username);
CREATE INDEX idx_users_email    ON users (email);

-- ────────────────────────────────────────────────────────────
-- Organizations
-- ────────────────────────────────────────────────────────────

CREATE TABLE organizations (
    id               UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    name             TEXT        NOT NULL,
    slug             CITEXT      NOT NULL UNIQUE,
    display_name     TEXT,
    description      TEXT,
    avatar_url       TEXT,
    website          TEXT,
    email            CITEXT,
    is_verified      BOOLEAN     NOT NULL DEFAULT FALSE,
    verified_domain  TEXT,
    mfa_required     BOOLEAN     NOT NULL DEFAULT FALSE,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_orgs_slug ON organizations (slug);

-- ────────────────────────────────────────────────────────────
-- Org Memberships
-- ────────────────────────────────────────────────────────────

CREATE TABLE org_memberships (
    id           UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    org_id       UUID        NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    user_id      UUID        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    role         org_role    NOT NULL DEFAULT 'viewer',
    invited_by   UUID        REFERENCES users (id) ON DELETE SET NULL,
    joined_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (org_id, user_id)
);

CREATE INDEX idx_org_memberships_org_id  ON org_memberships (org_id);
CREATE INDEX idx_org_memberships_user_id ON org_memberships (user_id);

-- ────────────────────────────────────────────────────────────
-- Teams
-- ────────────────────────────────────────────────────────────

CREATE TABLE teams (
    id           UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    org_id       UUID        NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    name         TEXT        NOT NULL,
    slug         CITEXT      NOT NULL,
    description  TEXT,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (org_id, slug)
);

CREATE INDEX idx_teams_org_id ON teams (org_id);

CREATE TABLE team_memberships (
    id       UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    team_id  UUID        NOT NULL REFERENCES teams (id) ON DELETE CASCADE,
    user_id  UUID        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    added_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (team_id, user_id)
);

-- ────────────────────────────────────────────────────────────
-- Namespace Claims
-- ────────────────────────────────────────────────────────────

CREATE TABLE namespace_claims (
    id             UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    ecosystem      ecosystem   NOT NULL,
    namespace      TEXT        NOT NULL,
    owner_user_id  UUID        REFERENCES users (id) ON DELETE SET NULL,
    owner_org_id   UUID        REFERENCES organizations (id) ON DELETE SET NULL,
    is_verified    BOOLEAN     NOT NULL DEFAULT FALSE,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (ecosystem, namespace),
    CONSTRAINT owner_set CHECK (
        (owner_user_id IS NOT NULL)::int + (owner_org_id IS NOT NULL)::int = 1
    )
);

-- ────────────────────────────────────────────────────────────
-- Repositories
-- ────────────────────────────────────────────────────────────

CREATE TABLE repositories (
    id             UUID              PRIMARY KEY DEFAULT uuid_generate_v4(),
    name           TEXT              NOT NULL,
    slug           CITEXT            NOT NULL,
    description    TEXT,
    kind           repository_kind   NOT NULL DEFAULT 'public',
    visibility     visibility        NOT NULL DEFAULT 'public',
    owner_user_id  UUID              REFERENCES users (id) ON DELETE SET NULL,
    owner_org_id   UUID              REFERENCES organizations (id) ON DELETE SET NULL,
    upstream_url   TEXT,
    created_at     TIMESTAMPTZ       NOT NULL DEFAULT NOW(),
    updated_at     TIMESTAMPTZ       NOT NULL DEFAULT NOW(),
    CONSTRAINT repo_owner_set CHECK (
        (owner_user_id IS NOT NULL)::int + (owner_org_id IS NOT NULL)::int <= 1
    )
);

CREATE INDEX idx_repositories_slug ON repositories (slug);

-- ────────────────────────────────────────────────────────────
-- Packages
-- ────────────────────────────────────────────────────────────

CREATE TABLE packages (
    id                  UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    repository_id       UUID        NOT NULL REFERENCES repositories (id) ON DELETE CASCADE,
    ecosystem           TEXT        NOT NULL,
    name                TEXT        NOT NULL,
    normalized_name     TEXT        NOT NULL,
    display_name        TEXT,
    description         TEXT,
    readme              TEXT,
    homepage            TEXT,
    repository_url      TEXT,
    license             TEXT,
    keywords            TEXT[]      NOT NULL DEFAULT '{}',
    visibility          TEXT        NOT NULL DEFAULT 'public',
    owner_user_id       UUID        REFERENCES users (id) ON DELETE SET NULL,
    owner_org_id        UUID        REFERENCES organizations (id) ON DELETE SET NULL,
    is_deprecated       BOOLEAN     NOT NULL DEFAULT FALSE,
    deprecation_message TEXT,
    is_archived         BOOLEAN     NOT NULL DEFAULT FALSE,
    download_count      BIGINT      NOT NULL DEFAULT 0,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (ecosystem, normalized_name, repository_id)
);

CREATE INDEX idx_packages_ecosystem        ON packages (ecosystem);
CREATE INDEX idx_packages_normalized_name  ON packages (normalized_name);
CREATE INDEX idx_packages_owner_user       ON packages (owner_user_id);
CREATE INDEX idx_packages_owner_org        ON packages (owner_org_id);
CREATE INDEX idx_packages_name_trgm        ON packages USING gin (name gin_trgm_ops);
CREATE INDEX idx_packages_desc_trgm        ON packages USING gin (description gin_trgm_ops);

-- ────────────────────────────────────────────────────────────
-- Releases
-- ────────────────────────────────────────────────────────────

CREATE TABLE releases (
    id                  UUID           PRIMARY KEY DEFAULT uuid_generate_v4(),
    package_id          UUID           NOT NULL REFERENCES packages (id) ON DELETE CASCADE,
    version             TEXT           NOT NULL,
    status              release_status NOT NULL DEFAULT 'quarantine',
    published_by        UUID           NOT NULL REFERENCES users (id) ON DELETE RESTRICT,
    description         TEXT,
    changelog           TEXT,
    is_prerelease       BOOLEAN        NOT NULL DEFAULT FALSE,
    is_yanked           BOOLEAN        NOT NULL DEFAULT FALSE,
    yank_reason         TEXT,
    is_deprecated       BOOLEAN        NOT NULL DEFAULT FALSE,
    deprecation_message TEXT,
    source_ref          TEXT,
    provenance          JSONB,
    published_at        TIMESTAMPTZ    NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ    NOT NULL DEFAULT NOW(),
    UNIQUE (package_id, version)
);

CREATE INDEX idx_releases_package_id  ON releases (package_id);
CREATE INDEX idx_releases_status      ON releases (status);
CREATE INDEX idx_releases_published   ON releases (published_at DESC);

-- ────────────────────────────────────────────────────────────
-- Artifacts
-- ────────────────────────────────────────────────────────────

CREATE TABLE artifacts (
    id                UUID          PRIMARY KEY DEFAULT uuid_generate_v4(),
    release_id        UUID          NOT NULL REFERENCES releases (id) ON DELETE CASCADE,
    kind              artifact_kind NOT NULL,
    filename          TEXT          NOT NULL,
    storage_key       TEXT          NOT NULL UNIQUE,
    content_type      TEXT          NOT NULL,
    size_bytes        BIGINT        NOT NULL,
    sha256            TEXT          NOT NULL,
    sha512            TEXT,
    md5               TEXT,
    is_signed         BOOLEAN       NOT NULL DEFAULT FALSE,
    signature_key_id  TEXT,
    uploaded_at       TIMESTAMPTZ   NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_artifacts_release_id ON artifacts (release_id);

-- ────────────────────────────────────────────────────────────
-- Channel Refs (tags / aliases)
-- ────────────────────────────────────────────────────────────

CREATE TABLE channel_refs (
    id          UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    package_id  UUID        NOT NULL REFERENCES packages (id) ON DELETE CASCADE,
    ecosystem   TEXT        NOT NULL,
    name        TEXT        NOT NULL,
    release_id  UUID        NOT NULL REFERENCES releases (id) ON DELETE CASCADE,
    created_by  UUID        NOT NULL REFERENCES users (id) ON DELETE RESTRICT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (package_id, name)
);

CREATE INDEX idx_channel_refs_package_id ON channel_refs (package_id);

-- ────────────────────────────────────────────────────────────
-- Tokens
-- ────────────────────────────────────────────────────────────

CREATE TABLE tokens (
    id             UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    kind           TEXT        NOT NULL DEFAULT 'personal',
    prefix         TEXT        NOT NULL DEFAULT 'pub_',
    token_hash     TEXT        NOT NULL UNIQUE,
    name           TEXT        NOT NULL,
    user_id        UUID        REFERENCES users (id) ON DELETE CASCADE,
    org_id         UUID        REFERENCES organizations (id) ON DELETE CASCADE,
    package_id     UUID        REFERENCES packages (id) ON DELETE CASCADE,
    repository_id  UUID        REFERENCES repositories (id) ON DELETE CASCADE,
    scopes         TEXT[]      NOT NULL DEFAULT '{}',
    last_used_at   TIMESTAMPTZ,
    expires_at     TIMESTAMPTZ,
    is_revoked     BOOLEAN     NOT NULL DEFAULT FALSE,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_tokens_user_id    ON tokens (user_id);
CREATE INDEX idx_tokens_token_hash ON tokens (token_hash);
CREATE INDEX idx_tokens_org_id     ON tokens (org_id);

-- ────────────────────────────────────────────────────────────
-- Security Findings
-- ────────────────────────────────────────────────────────────

CREATE TABLE security_findings (
    id           UUID              PRIMARY KEY DEFAULT uuid_generate_v4(),
    release_id   UUID              NOT NULL REFERENCES releases (id) ON DELETE CASCADE,
    artifact_id  UUID              REFERENCES artifacts (id) ON DELETE SET NULL,
    kind         finding_kind      NOT NULL,
    severity     security_severity NOT NULL,
    title        TEXT              NOT NULL,
    description  TEXT,
    advisory_id  TEXT,
    is_resolved  BOOLEAN           NOT NULL DEFAULT FALSE,
    resolved_at  TIMESTAMPTZ,
    resolved_by  UUID              REFERENCES users (id) ON DELETE SET NULL,
    detected_at  TIMESTAMPTZ       NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_security_findings_release_id ON security_findings (release_id);
CREATE INDEX idx_security_findings_severity   ON security_findings (severity);

-- ────────────────────────────────────────────────────────────
-- Audit Log (append-only)
-- ────────────────────────────────────────────────────────────

CREATE TABLE audit_logs (
    id                UUID         PRIMARY KEY DEFAULT uuid_generate_v4(),
    action            audit_action NOT NULL,
    actor_user_id     UUID         REFERENCES users (id) ON DELETE SET NULL,
    actor_token_id    UUID         REFERENCES tokens (id) ON DELETE SET NULL,
    target_user_id    UUID         REFERENCES users (id) ON DELETE SET NULL,
    target_org_id     UUID         REFERENCES organizations (id) ON DELETE SET NULL,
    target_package_id UUID         REFERENCES packages (id) ON DELETE SET NULL,
    target_release_id UUID         REFERENCES releases (id) ON DELETE SET NULL,
    ip_address        TEXT,
    user_agent        TEXT,
    metadata          JSONB,
    occurred_at       TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_audit_logs_action        ON audit_logs (action);
CREATE INDEX idx_audit_logs_actor_user    ON audit_logs (actor_user_id);
CREATE INDEX idx_audit_logs_target_pkg    ON audit_logs (target_package_id);
CREATE INDEX idx_audit_logs_occurred_at   ON audit_logs (occurred_at DESC);

-- Prevent updates and deletes on audit_log (enforced at DB level)
CREATE RULE audit_log_no_update AS ON UPDATE TO audit_logs DO INSTEAD NOTHING;
CREATE RULE audit_log_no_delete AS ON DELETE TO audit_logs DO INSTEAD NOTHING;

-- ────────────────────────────────────────────────────────────
-- Team Package Access
-- ────────────────────────────────────────────────────────────

CREATE TABLE team_package_access (
    id          UUID            PRIMARY KEY DEFAULT uuid_generate_v4(),
    team_id     UUID            NOT NULL REFERENCES teams (id) ON DELETE CASCADE,
    package_id  UUID            NOT NULL REFERENCES packages (id) ON DELETE CASCADE,
    permission  team_permission NOT NULL,
    granted_at  TIMESTAMPTZ     NOT NULL DEFAULT NOW(),
    UNIQUE (team_id, package_id, permission)
);

-- ────────────────────────────────────────────────────────────
-- Trusted Publishing (OIDC)
-- ────────────────────────────────────────────────────────────

CREATE TABLE trusted_publishers (
    id            UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    package_id    UUID        NOT NULL REFERENCES packages (id) ON DELETE CASCADE,
    issuer        TEXT        NOT NULL,
    subject       TEXT        NOT NULL,
    repository    TEXT,
    workflow_ref  TEXT,
    environment   TEXT,
    created_by    UUID        NOT NULL REFERENCES users (id) ON DELETE RESTRICT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (package_id, issuer, subject)
);

CREATE INDEX idx_trusted_publishers_package ON trusted_publishers (package_id);
