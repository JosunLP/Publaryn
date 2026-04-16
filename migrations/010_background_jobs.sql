-- Background job queue schema for the worker infrastructure.
-- Pairs with the `publaryn-workers` crate (queue.rs / worker.rs).

CREATE TYPE job_status AS ENUM ('pending', 'running', 'completed', 'failed', 'dead');

CREATE TYPE job_kind AS ENUM (
    'scan_artifact',
    'index_package',
    'deliver_webhook',
    'cleanup_expired_tokens',
    'reindex_search'
);

CREATE TABLE background_jobs (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    kind            job_kind    NOT NULL,
    payload         JSONB       NOT NULL DEFAULT '{}',
    status          job_status  NOT NULL DEFAULT 'pending',
    attempts        INT         NOT NULL DEFAULT 0,
    max_attempts    INT         NOT NULL DEFAULT 5,
    last_error      TEXT,
    scheduled_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    locked_until    TIMESTAMPTZ,
    locked_by       TEXT,
    started_at      TIMESTAMPTZ,
    completed_at    TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Efficiently claim the next pending jobs ordered by schedule time.
CREATE INDEX idx_background_jobs_claimable
    ON background_jobs (scheduled_at)
    WHERE status = 'pending';

-- Look up jobs by kind (monitoring, metrics).
CREATE INDEX idx_background_jobs_kind ON background_jobs (kind);

-- Find stale running jobs whose lock has expired (recovery sweep).
CREATE INDEX idx_background_jobs_stale
    ON background_jobs (locked_until)
    WHERE status = 'running';
