-- ============================================================
-- Migration 019: repository_transfer audit action
-- ============================================================

ALTER TYPE audit_action ADD VALUE IF NOT EXISTS 'repository_transfer';
