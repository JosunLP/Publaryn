-- Migration 018: RubyGems release metadata
--
-- Gems carry per-release fields that don't map cleanly onto the generic
-- release model: platform, runtime/development dependencies, required
-- Ruby/RubyGems versions, and licenses.
--
-- Platform is stored here. For non-`ruby` platforms the app encodes the
-- platform as a suffix on `releases.version` (e.g. `1.2.3-x86_64-linux`)
-- to keep the existing `(package_id, version)` uniqueness constraint
-- working without ecosystem-specific partial indexes.

CREATE TABLE IF NOT EXISTS rubygems_release_metadata (
    release_id          UUID        PRIMARY KEY REFERENCES releases (id) ON DELETE CASCADE,
    platform            TEXT        NOT NULL DEFAULT 'ruby',
    summary             TEXT,
    authors             TEXT[]      NOT NULL DEFAULT '{}',
    licenses            TEXT[]      NOT NULL DEFAULT '{}',
    required_ruby_version      TEXT,
    required_rubygems_version  TEXT,
    runtime_dependencies       JSONB NOT NULL DEFAULT '[]',
    development_dependencies   JSONB NOT NULL DEFAULT '[]',
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_rubygems_release_platform
    ON rubygems_release_metadata (platform);

COMMENT ON TABLE rubygems_release_metadata IS
    'RubyGems-specific per-release metadata from the .gem gemspec.';
COMMENT ON COLUMN rubygems_release_metadata.runtime_dependencies IS
    'Array of {name, requirement} objects (requirement is the Gem::Requirement string).';
