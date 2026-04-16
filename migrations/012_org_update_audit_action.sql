-- ============================================================
-- Migration 012: Organization update audit action
-- ============================================================

ALTER TYPE audit_action ADD VALUE IF NOT EXISTS 'org_update';
