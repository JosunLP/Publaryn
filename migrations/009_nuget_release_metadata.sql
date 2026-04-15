-- NuGet-specific per-release metadata, mirroring the nuspec fields that are
-- not captured by the generic release model.
--
-- Follows the same pattern as cargo_release_metadata (migration 008).

CREATE TABLE nuget_release_metadata (
    release_id   UUID PRIMARY KEY REFERENCES releases (id) ON DELETE CASCADE,
    authors      TEXT,
    title        TEXT,
    icon_url     TEXT,
    license_url  TEXT,
    license_expression TEXT,
    project_url  TEXT,
    require_license_acceptance BOOLEAN NOT NULL DEFAULT FALSE,
    min_client_version TEXT,
    summary      TEXT,
    tags         TEXT[] NOT NULL DEFAULT '{}',
    -- Dependency groups stored as JSON array of objects:
    -- [{"targetFramework": ".NETStandard2.0", "dependencies": [{"id": "...", "range": "..."}]}]
    dependency_groups JSONB NOT NULL DEFAULT '[]',
    -- Package types as JSON array: [{"name": "Dependency"}]
    package_types JSONB NOT NULL DEFAULT '[{"name":"Dependency"}]',
    -- Whether the version is listed (visible in search / default resolution).
    -- Unlisting sets this to false without removing the release.
    is_listed    BOOLEAN NOT NULL DEFAULT TRUE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
