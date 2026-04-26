# Job Queue Recovery Runbook

This runbook describes the minimal 1.0 operator workflow for background job
visibility and recovery in self-hosted Publaryn deployments.

## Access requirements

- a platform administrator account
- a JWT session or API token carrying the `audit:read` scope

The queue visibility endpoint is:

```http
GET /v1/admin/jobs
```

Recovery action endpoints are:

```http
POST /v1/admin/jobs/recover-stale
POST /v1/admin/jobs/{job_id}/retry
```

Supported query parameters:

- `state=pending|running|completed|failed|dead`
- `kind=scan_artifact|index_package|deliver_webhook|cleanup_expired_tokens|cleanup_oci_blobs|reindex_search`
- `page=<n>`
- `per_page=<n>`

## What to check first

1. Inspect the queue summary:
   - `summary.by_status`
   - `summary.by_kind`
   - `summary.oldest_pending_age_minutes`
   - `summary.stale_jobs_count`
2. Confirm whether the queue is blocked in one state or one job kind.
3. Review `jobs[*].last_error`, `locked_by`, `locked_until`, and `attempts` for
   the affected jobs.
4. Use `jobs[*].is_stale`, `jobs[*].can_retry`, and `jobs[*].recovery_hint` to
  decide whether an API-level recovery action is safe.

## Typical checks

### All pending jobs

```http
GET /v1/admin/jobs?state=pending
```

Use this when publication, search, or cleanup work appears delayed.

### Stale running jobs

```http
GET /v1/admin/jobs?state=running
```

Then inspect `summary.stale_jobs_count`.

If stale jobs are present, compare `locked_until` with current time and confirm
that the corresponding worker instance is no longer healthy before intervening.

### One queue family only

```http
GET /v1/admin/jobs?kind=cleanup_oci_blobs
GET /v1/admin/jobs?kind=scan_artifact
GET /v1/admin/jobs?kind=reindex_search
```

Use job-kind filters to separate publish-path failures from maintenance-path
failures.

## Recovery guidance

### Pending backlog is growing

- verify API and worker processes are both running
- check PostgreSQL health and connection saturation
- confirm Redis availability if you expect Redis-backed features in the
  deployment
- look for repeated `dead` jobs that indicate a systematic handler failure

### Stale running jobs are reported

Publaryn workers already attempt stale-job recovery during their periodic queue
sweeps. If `summary.stale_jobs_count` remains non-zero for multiple recovery
intervals:

1. confirm the worker process responsible for the stale lock is gone or hung
2. restart or replace the worker deployment
3. call `POST /v1/admin/jobs/recover-stale` to reset abandoned running locks to
  `pending`
4. recheck `GET /v1/admin/jobs` to confirm the jobs return to `pending` and are
   claimed again

### Dead-lettered jobs accumulate

- inspect `last_error` to identify whether the failure is data-specific or
  systemic
- correct the underlying storage, database, or handler issue first
- only then call `POST /v1/admin/jobs/{job_id}/retry` for failed or dead jobs
  that are safe to replay

The retry endpoint preserves `last_error` for diagnosis, resets the job to
`pending`, clears stale lock/completion fields, and resets `attempts` so normal
worker backoff can start again from a clean operator-initiated replay.

## Notes

- `GET /v1/stats` complements this runbook with a public top-level
  `job_queue_pending` counter for quick smoke checks.
- Operator recovery actions are audited with `admin_job_retry` and
  `admin_jobs_recover_stale` audit events.
- Broad abuse, takedown, and full operator-console workflows remain outside the
  1.1.0 recovery scope.
