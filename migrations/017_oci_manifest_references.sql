-- Migration 017: OCI manifest references
--
-- Records the set of blob digests (config + layers + optional subject) that
-- an OCI manifest release references. Enables referential integrity on push
-- and unreferenced-blob garbage collection later.

CREATE TABLE IF NOT EXISTS oci_manifest_references (
    release_id   UUID        NOT NULL REFERENCES releases (id) ON DELETE CASCADE,
    ref_digest   TEXT        NOT NULL,
    ref_kind     TEXT        NOT NULL CHECK (ref_kind IN ('config', 'layer', 'subject')),
    ref_size     BIGINT,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (release_id, ref_digest, ref_kind)
);

CREATE INDEX IF NOT EXISTS idx_oci_manifest_refs_digest ON oci_manifest_references (ref_digest);

COMMENT ON TABLE oci_manifest_references IS 'OCI manifest → referenced blob digests.';
COMMENT ON COLUMN oci_manifest_references.ref_kind IS 'One of: config | layer | subject.';

-- Tracks in-progress OCI blob upload sessions. Sessions are resumable via
-- PATCH / finalize with PUT ?digest=.
CREATE TABLE IF NOT EXISTS oci_upload_sessions (
    id            UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    repository_id UUID        NOT NULL REFERENCES repositories (id) ON DELETE CASCADE,
    package_name  TEXT        NOT NULL,
    created_by    UUID        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    storage_key   TEXT        NOT NULL,
    received_bytes BIGINT     NOT NULL DEFAULT 0,
    sha256_state   BYTEA,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_oci_upload_sessions_pkg ON oci_upload_sessions (package_name);

COMMENT ON TABLE oci_upload_sessions IS 'Resumable OCI blob upload state.';
