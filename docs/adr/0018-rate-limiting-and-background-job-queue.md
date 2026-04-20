# ADR 0018: Redis-backed rate limiting and PostgreSQL-backed background job queue

- Status: Accepted
- Date: 2026-04-16

## Context

Publaryn needs two foundational infrastructure capabilities before the platform
can harden security, implement scanning pipelines, or deliver webhooks:

1. **Rate limiting** — protect authentication, publish, and read endpoints from
   abuse, credential stuffing, and denial-of-service. Rate limiting must work
   correctly across multiple horizontal replicas sharing no local state.

2. **Background job processing** — decouple long-running work such as artifact
   scanning, search reindexing, webhook delivery, and token cleanup from the
   synchronous request path. Workers must be independently scalable and safe
   for multi-instance deployment.

Redis is already provisioned in the deployment baseline (`docker-compose.yml`,
configuration in `config.rs`) but was not yet wired into the application. A
background job queue was entirely absent.

## Decision

### Redis client integration

The `fred` Redis client is added to `AppState` as an optional dependency.
If Redis is unreachable at startup the application starts without it and logs
a warning. The `/readiness` probe reports Redis health alongside PostgreSQL.

Environment configuration remains environment-variable driven:

```ini
REDIS__URL=redis://localhost:6379
```

### Rate limiting middleware

A custom Tower middleware (`rate_limit.rs`) uses Redis `INCR` + `EXPIRE` for
fixed-window per-minute rate limiting. Requests are classified into four tiers:

| Tier     | Scope                                               | Default limit |
| -------- | --------------------------------------------------- | ------------- |
| Auth     | `/v1/auth/*` (register, login)                      | 10 req/min    |
| Write    | POST, PUT, PATCH, DELETE mutations                  | 60 req/min    |
| Read     | GET/HEAD control-plane reads                        | 300 req/min   |
| Protocol | GET/HEAD on native adapter reads such as `/npm/`, `/pypi/`, `/cargo/`, `/nuget/`, `/oci/`, `/rubygems/`, `/maven/`, and `/composer/` | 1000 req/min  |

Keys are derived from:

- **Authenticated requests**: SHA-256 prefix of the `Authorization` header
- **Anonymous requests**: client IP address

When Redis is unavailable the middleware is permissive — requests pass through
rather than causing a hard outage. Rate limit headers are always returned:
`X-RateLimit-Limit`, `X-RateLimit-Remaining`, and `Retry-After` on 429.

All tier limits are configurable via environment variables:

```ini
RATE_LIMIT__ENABLED=true
RATE_LIMIT__AUTH_REQUESTS_PER_MINUTE=10
RATE_LIMIT__WRITE_REQUESTS_PER_MINUTE=60
RATE_LIMIT__READ_REQUESTS_PER_MINUTE=300
RATE_LIMIT__PROTOCOL_REQUESTS_PER_MINUTE=1000
```

### Background job queue

A new `crates/workers/` crate provides a PostgreSQL-backed job queue using
`SELECT ... FOR UPDATE SKIP LOCKED` for safe concurrent claiming. This avoids
introducing a new infrastructure dependency — only PostgreSQL is required.

**Database schema** (migration 010):

- `background_jobs` table with columns: `id`, `kind`, `payload` (JSONB),
  `status`, `attempts`, `max_attempts`, `last_error`, `scheduled_at`,
  `locked_until`, `locked_by`, `started_at`, `completed_at`, `created_at`
- Custom enums: `job_status` (pending, running, completed, failed, dead),
  `job_kind` (scan_artifact, index_package, deliver_webhook,
  cleanup_expired_tokens, reindex_search)
- Indexes optimized for the claim query, stale recovery, and cleanup

**Worker lifecycle**:

- Configurable poll interval, batch size, and lock duration
- Exponential backoff on failure (10s × 2^attempt)
- Dead-lettering after `max_attempts` (default 3)
- Automatic stale job recovery for crashed workers
- Automatic cleanup of completed/dead jobs after retention period
- Graceful shutdown via `tokio::sync::watch` channel
- `JobHandler` trait for pluggable processing logic

**Integration**:

- The worker runs as a background `tokio::spawn` task in the main binary
- It shares the PostgreSQL pool with the API server
- The shutdown signal cascade ensures the worker drains before exit
- Workers can also be deployed as standalone replicas using the same binary
  with different entry points in the future

## Consequences

### Rate limiting

- Authentication endpoints are protected against credential stuffing and brute force
- Publish endpoints are rate-limited to prevent abuse from compromised tokens
- Protocol adapter reads have generous limits to avoid breaking native client workflows
- Redis failure degrades gracefully — the platform remains functional
- Multiple API replicas share the same rate limit counters via Redis
- Operators can tune limits per deployment without code changes
- Fixed-window approach has known edge-case burst behavior at window boundaries;
  a sliding-window upgrade can be added later if needed

### Background workers

- Artifact scanning, search indexing, webhook delivery, and cleanup can be
  dispatched asynchronously without blocking request handlers
- Multiple worker instances can run concurrently thanks to `SKIP LOCKED`
- No additional infrastructure beyond PostgreSQL is required
- Job processing is idempotent and retry-safe by design
- Queue depth metrics are available for autoscaling decisions
- The job queue does not compete with high-throughput protocol reads because
  it uses advisory-style locking rather than table-level locks

### Scalability

- Rate limiting is inherently horizontal — Redis is the single coordination
  point, which is architecturally appropriate for this role
- Workers scale independently; adding replicas increases job throughput
- The PostgreSQL job queue becomes a bottleneck only at very high enqueue rates;
  a Redis-stream or dedicated queue can replace it later without changing the
  `JobHandler` interface

### Security

- Rate limiting reduces the attack surface for credential stuffing, enumeration,
  and denial of service
- Job payload validation must be performed in handlers to prevent injection
- Worker processes use the same database credentials and audit logging as the API
