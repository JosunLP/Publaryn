-- PyPI-specific per-release metadata needed for resolver-aware Simple API responses.
--
-- Publaryn already stores the raw legacy-upload core metadata in release
-- provenance, but common Python clients also need normalized, queryable fields
-- like Requires-Python on the adapter read path. Keep those fields in a
-- dedicated ecosystem table rather than widening the generic release model.

CREATE TABLE IF NOT EXISTS pypi_release_metadata (
    release_id         UUID PRIMARY KEY REFERENCES releases(id) ON DELETE CASCADE,
    requires_python    TEXT,
    requires_dist      TEXT[],
    requires_external  TEXT[],
    provides_extra     TEXT[],
    created_at         TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

COMMENT ON TABLE pypi_release_metadata IS
    'PyPI-specific per-release metadata projected from legacy upload core metadata for resolver-aware Simple API responses.';
COMMENT ON COLUMN pypi_release_metadata.requires_python IS
    'Normalized Requires-Python constraint from the uploaded distribution metadata.';
COMMENT ON COLUMN pypi_release_metadata.requires_dist IS
    'Raw Requires-Dist entries captured from the uploaded distribution metadata.';
COMMENT ON COLUMN pypi_release_metadata.requires_external IS
    'Raw Requires-External entries captured from the uploaded distribution metadata.';
COMMENT ON COLUMN pypi_release_metadata.provides_extra IS
    'Raw Provides-Extra entries captured from the uploaded distribution metadata.';
