-- Migration 023: OCI blob inventory + background cleanup jobs
--
-- Publaryn stores OCI blobs content-addressably in shared object storage, but
-- unreferenced blobs need a protocol-aware inventory so background cleanup can
-- remove them safely after a grace period.

ALTER TYPE job_kind ADD VALUE IF NOT EXISTS 'cleanup_oci_blobs';

CREATE TABLE IF NOT EXISTS oci_blob_inventory (
    digest            TEXT        PRIMARY KEY,
    storage_key       TEXT        NOT NULL UNIQUE,
    size_bytes        BIGINT      NOT NULL,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_uploaded_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_oci_blob_inventory_last_uploaded_at
    ON oci_blob_inventory (last_uploaded_at);

COMMENT ON TABLE oci_blob_inventory IS
    'Inventory of OCI config/layer blobs stored in object storage for cleanup and lifecycle tracking.';
COMMENT ON COLUMN oci_blob_inventory.digest IS
    'OCI blob digest (for example sha256:...).';
COMMENT ON COLUMN oci_blob_inventory.storage_key IS
    'Content-addressed object-storage key for the blob.';
COMMENT ON COLUMN oci_blob_inventory.last_uploaded_at IS
    'Most recent successful upload timestamp for this digest, used for grace-period cleanup decisions.';

INSERT INTO oci_blob_inventory (digest, storage_key, size_bytes, created_at, last_uploaded_at)
SELECT
    omr.ref_digest,
    CONCAT('oci/blobs/sha256/', split_part(omr.ref_digest, ':', 2)) AS storage_key,
    COALESCE(MAX(omr.ref_size), 0) AS size_bytes,
    MIN(omr.created_at) AS created_at,
    MAX(omr.created_at) AS last_uploaded_at
FROM oci_manifest_references omr
WHERE omr.ref_kind IN ('config', 'layer')
GROUP BY omr.ref_digest
ON CONFLICT (digest) DO UPDATE
SET size_bytes = GREATEST(oci_blob_inventory.size_bytes, EXCLUDED.size_bytes),
    last_uploaded_at = GREATEST(oci_blob_inventory.last_uploaded_at, EXCLUDED.last_uploaded_at);
