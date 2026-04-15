-- ============================================================
-- Migration 006: Canonicalize PyPI normalized package names
-- ============================================================

UPDATE packages
SET normalized_name = LOWER(REGEXP_REPLACE(name, '[-_.]+', '-', 'g'))
WHERE ecosystem = 'pypi';
