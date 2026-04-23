# Operations Guide

This guide is the starting point for running, validating, and releasing
Publaryn in a self-hosted environment.

## Deployment baseline

Publaryn 1.0 expects the following backing services:

- PostgreSQL
- Redis for rate limiting and optional runtime coordination
- S3-compatible object storage such as MinIO
- Meilisearch

The API exposes:

- `GET /health` for liveness
- `GET /readiness` for readiness based on PostgreSQL and optional Redis connectivity
- `GET /v1/admin/jobs` for filtered background-job queue visibility

## Local validation

Backend checks:

- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test -p publaryn-core`
- `cargo test -p publaryn-auth --lib`
- `cargo test -p publaryn-api --lib`
- `cargo test -p publaryn-api --test integration_tests`
- `cargo test -p publaryn-auth --test auth_tests`

Frontend checks:

- `bun install --frozen-lockfile`
- `bun run typecheck`
- `bun test`
- `bun run build`

## Release operations

- [Release checklist](/release-checklist)
- [Release notes index](/releases/README)
- [Publaryn 1.0.0 release notes](/releases/1.0.0)

## Runbooks

- [Operator job queue recovery](/operator/job-queue-recovery)

## Additional reference

- [1.0 release contract](/1.0)
- [API and adapter route reference](/api-routes)
