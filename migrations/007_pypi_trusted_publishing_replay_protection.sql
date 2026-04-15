-- Migration 007: replay protection for PyPI trusted publishing OIDC exchange

CREATE TABLE IF NOT EXISTS oidc_token_replays (
    issuer      TEXT        NOT NULL,
    jwt_id      TEXT        NOT NULL,
    expires_at  TIMESTAMPTZ NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (issuer, jwt_id)
);

CREATE INDEX IF NOT EXISTS idx_oidc_token_replays_expires_at
    ON oidc_token_replays (expires_at);
