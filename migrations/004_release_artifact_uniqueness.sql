-- ============================================================
-- Migration 004: Release artifact uniqueness for idempotent uploads
-- ============================================================

ALTER TABLE artifacts
    ADD CONSTRAINT artifacts_release_filename_unique UNIQUE (release_id, filename);
