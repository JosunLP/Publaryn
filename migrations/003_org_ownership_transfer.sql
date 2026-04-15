-- ============================================================
-- Migration 003: Organization ownership transfer audit action
-- ============================================================

ALTER TYPE audit_action ADD VALUE IF NOT EXISTS 'org_ownership_transfer';
