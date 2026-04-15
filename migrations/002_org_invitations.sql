-- ============================================================
-- Migration 002: Organization invitations
-- ============================================================

ALTER TYPE audit_action ADD VALUE IF NOT EXISTS 'org_invitation_create';
ALTER TYPE audit_action ADD VALUE IF NOT EXISTS 'org_invitation_revoke';
ALTER TYPE audit_action ADD VALUE IF NOT EXISTS 'org_invitation_accept';
ALTER TYPE audit_action ADD VALUE IF NOT EXISTS 'org_invitation_decline';

CREATE TABLE org_invitations (
    id               UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    org_id           UUID        NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    invited_user_id  UUID        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    role             org_role    NOT NULL DEFAULT 'viewer',
    invited_by       UUID        NOT NULL REFERENCES users (id) ON DELETE RESTRICT,
    accepted_by      UUID        REFERENCES users (id) ON DELETE SET NULL,
    accepted_at      TIMESTAMPTZ,
    declined_by      UUID        REFERENCES users (id) ON DELETE SET NULL,
    declined_at      TIMESTAMPTZ,
    revoked_by       UUID        REFERENCES users (id) ON DELETE SET NULL,
    revoked_at       TIMESTAMPTZ,
    expires_at       TIMESTAMPTZ NOT NULL,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_org_invitations_org_id ON org_invitations (org_id);
CREATE INDEX idx_org_invitations_invited_user_id ON org_invitations (invited_user_id);
CREATE INDEX idx_org_invitations_expires_at ON org_invitations (expires_at);

CREATE UNIQUE INDEX idx_org_invitations_active_unique
    ON org_invitations (org_id, invited_user_id)
    WHERE accepted_at IS NULL AND declined_at IS NULL AND revoked_at IS NULL;
