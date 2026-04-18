ALTER TYPE audit_action ADD VALUE IF NOT EXISTS 'team_repository_access_update';

CREATE TABLE IF NOT EXISTS team_repository_access (
    id UUID PRIMARY KEY,
    team_id UUID NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    repository_id UUID NOT NULL REFERENCES repositories(id) ON DELETE CASCADE,
    permission team_permission NOT NULL,
    granted_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (team_id, repository_id, permission)
);

CREATE INDEX IF NOT EXISTS idx_team_repository_access_team_id
    ON team_repository_access (team_id);

CREATE INDEX IF NOT EXISTS idx_team_repository_access_repository_id
    ON team_repository_access (repository_id);
