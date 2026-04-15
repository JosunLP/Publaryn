# ADR 0011: Stateless API runtime and graceful lifecycle for horizontal scaling

- Status: Accepted
- Date: 2026-04-15

## Context

Publaryn is intended to scale horizontally across multiple API replicas and independently scaled worker pools.
The current architecture already externalizes durable state into PostgreSQL, S3-compatible object storage, Redis, and a dedicated search service.

However, horizontal scaling also depends on explicit runtime lifecycle behavior:

- replicas must be safe to add and remove at any time
- orchestrators must have a reliable way to determine liveness versus readiness
- rolling updates must not cut off in-flight publish or management requests abruptly
- no critical workflow may depend on local container filesystem state

Without these guarantees, Kubernetes or other container platforms can route traffic to instances that are not ready, or terminate replicas mid-request during scale-down and deployment rollouts.

## Decision

### API runtime model

Publaryn API instances are stateless application replicas.
Durable state remains external to the process:

- PostgreSQL for metadata, governance state, and audit data
- S3-compatible object storage for immutable artifacts and blobs
- Redis for shared cache, coordination helpers, and future session/rate-limit data
- a dedicated search service for derived discovery views

No critical runtime state may be stored only on the local container filesystem.

### Probe semantics

Publaryn distinguishes liveness from readiness:

- `/health` is the process liveness probe and returns `200 OK` while the process is alive.
- `/readiness` is the traffic readiness probe for database-backed requests and returns `200 OK` only when PostgreSQL connectivity succeeds; otherwise it returns `503 Service Unavailable`.

This allows orchestrators to keep a live process running while removing an unready replica from service.

### Graceful shutdown

The API server must handle `SIGTERM` and `Ctrl+C` with graceful shutdown.
When a shutdown signal is received, the server stops accepting new connections and allows in-flight requests to finish before exiting.

Container deployments should provide a grace period long enough for ordinary management and publish requests to complete.
The local Compose baseline uses a `30s` stop grace period.

## Consequences

- API replicas can participate in rolling updates and horizontal scale-down without introducing avoidable request loss.
- Readiness failures now communicate operational state correctly to load balancers and orchestrators.
- The runtime contract stays aligned with the modular-monolith architecture: strong transactional consistency in shared stores, but no single-node dependency in the API tier.
- Future worker processes should follow the same lifecycle rules, including signal-aware draining and idempotent retry behavior.
- Broader dependency readiness checks for object storage, Redis, or search can be added later if a specific endpoint class requires them to be treated as hard readiness dependencies.
