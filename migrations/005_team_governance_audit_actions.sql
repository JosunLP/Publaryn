-- ============================================================
-- Migration 005: Team governance audit actions
-- ============================================================

ALTER TYPE audit_action ADD VALUE IF NOT EXISTS 'team_update';
ALTER TYPE audit_action ADD VALUE IF NOT EXISTS 'team_package_access_update';
