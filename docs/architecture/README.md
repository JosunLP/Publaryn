# Architecture Overview

Publaryn is built as a Rust workspace with a modular monolith at its core and a
small set of clearly separated runtime concerns around it.

## Runtime shape

- **API server**: Axum-based HTTP surface for management routes, native adapter mounts, OpenAPI, and frontend asset serving
- **Core domain**: package lifecycle, governance, visibility, audit, and validation logic
- **Auth layer**: password login, JWT sessions, API tokens, MFA, and trusted publishing support
- **Workers and jobs**: reindexing, cleanup, scanning, and asynchronous recovery-oriented workflows
- **Frontend**: Bun-managed SvelteKit application with Tailwind CSS, built as static assets

## Backing services

- **PostgreSQL** for metadata, governance, audit, releases, and job state
- **Redis** for rate limiting, caching, and selected runtime coordination
- **S3-compatible object storage** for immutable artifacts and OCI blobs
- **Meilisearch** for discovery and visibility-aware search

## Workspace layout

```text
crates/
├── api/                HTTP server, router composition, OpenAPI
├── auth/               authentication, JWT, MFA, OIDC-related flows
├── core/               domain models, validation, policy, persistence helpers
├── search/             search integration
├── workers/            async/background job processing
├── test-utils/         integration-test support
└── adapters/
    ├── npm/
    ├── pypi/
    ├── cargo-registry/
    ├── nuget/
    ├── maven/
    ├── rubygems/
    ├── composer/
    └── oci/
```

## Design expectations for 1.0

- native protocol behavior should remain ecosystem-correct
- security-sensitive writes derive ownership from the authenticated actor
- background jobs remain observable and recoverable
- public and authenticated visibility rules stay aligned across every surface

## Detailed architecture references

- [Full product concept](/concept)
- [Architecture decision record index](/adr/README)
- [API and adapter route reference](/api-routes)
