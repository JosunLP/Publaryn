-- ============================================================
-- Migration 016: security_finding_reopen audit action
-- ============================================================

ALTER TYPE audit_action ADD VALUE IF NOT EXISTS 'security_finding_reopen';
