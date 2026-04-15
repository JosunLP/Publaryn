-- Cargo-specific per-release metadata needed for sparse index serving.
-- Kept separate from the generic release model to honour ecosystem isolation.
CREATE TABLE IF NOT EXISTS cargo_release_metadata (
    release_id  UUID PRIMARY KEY REFERENCES releases(id) ON DELETE CASCADE,
    deps        JSONB NOT NULL DEFAULT '[]',
    features    JSONB NOT NULL DEFAULT '{}',
    features2   JSONB,
    links       TEXT,
    rust_version TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

COMMENT ON TABLE  cargo_release_metadata IS 'Cargo-specific dependency and feature data per release, used to serve the sparse index.';
COMMENT ON COLUMN cargo_release_metadata.deps IS 'Dependency array in Cargo index format (name, req, features, optional, default_features, target, kind, registry, package).';
COMMENT ON COLUMN cargo_release_metadata.features IS 'Feature map (v1 format).';
COMMENT ON COLUMN cargo_release_metadata.features2 IS 'Extended feature map (v2 format with dep: prefix and weak deps).';
COMMENT ON COLUMN cargo_release_metadata.links IS 'Value of the links field from Cargo.toml.';
COMMENT ON COLUMN cargo_release_metadata.rust_version IS 'Minimum supported Rust version (MSRV).';
