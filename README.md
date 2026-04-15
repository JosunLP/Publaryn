# Publaryn

**Publaryn** — secure, independent package registry across ecosystems.

A self-hostable, security-first package registry platform that speaks the native protocols of all major package managers and provides a unified management API.

---

## Supported Ecosystems

| Ecosystem    | Protocol                   | Status        |
| ------------ | -------------------------- | ------------- |
| npm / Bun    | npm Registry Protocol      | 🚧 In progress |
| pip / PyPI   | Simple Index (PEP 503/691) | 🚧 In progress |
| Rust Crates  | Cargo Sparse Index         | 🚧 In progress |
| NuGet        | NuGet v3                   | 🚧 In progress |
| Apache Maven | Maven2                     | 🚧 In progress |
| RubyGems     | RubyGems / Compact Index   | 🚧 In progress |
| Composer     | Composer Repository        | 🚧 In progress |
| Containers   | OCI Distribution Spec      | 🚧 In progress |

> **Note:** Bun uses the npm adapter — no separate protocol implementation is required.

---

## Architecture

```text
                     ┌────────────────────┐
                     │     Web Portal /   │
                     │     Admin UI       │
                     └────────┬───────────┘
                              │
                              ▼
┌──────────────┐    ┌──────────────────────┐    ┌──────────────────┐
│ Native       │    │  Management REST API │    │ Auth / Identity  │
│ Clients      ├───►│  /v1/*              ├───►│ OIDC / MFA / JWT │
│ npm, pip,    │    │  OpenAPI / Swagger  │    │ Tokens / Sessions │
│ cargo, etc.  │    └──────────┬──────────┘    └────────┬─────────┘
└──────┬───────┘               │                        │
       │                       ▼                        ▼
       │          ┌─────────────────────────────────────────────┐
       │          │            Rust Core Application             │
       │          │─────────────────────────────────────────────│
       │          │ Package Domain │ Org/Teams │ Policy Engine  │
       │          │ Publish Pipeline │ Audit    │ Namespace Mgmt │
       │          │ Security Findings │ Search  │ Provenance     │
       └─────────►└──┬──────────────┬──────────┬───────────────┘
                     │              │           │
                     ▼              ▼           ▼
              ┌─────────┐  ┌──────────────┐  ┌──────────────┐
              │Protocol │  │   Background  │  │ Scan/Policy  │
              │Adapters │  │   Workers     │  │ Workers      │
              │npm/pip/ │  │   indexing    │  │ ClamAV/YARA  │
              │cargo/   │  │   gc/events   │  │ Trivy/Grype  │
              └────┬────┘  └──────┬────────┘  └──────┬───────┘
                   │              │                   │
                   └──────────────┴───────────────────┘
                                  │
          ┌───────────────────────┼──────────────────────────┐
          ▼                       ▼                          ▼
  ┌──────────────┐    ┌───────────────────────┐    ┌────────────────┐
  │  PostgreSQL  │    │  S3 / MinIO Artifacts │    │   Meilisearch  │
  │  metadata,   │    │  immutable blob store │    │   full-text    │
  │  audit, auth │    └───────────────────────┘    │   search       │
  └──────────────┘                                 └────────────────┘
          │
  ┌───────┴───────┐
  │     Redis     │
  │ cache / rate  │
  │ limit / sess. │
  └───────────────┘
```

### Crate Structure

```text
crates/
├── core/               # Domain models, errors, validation, policy
├── api/                # HTTP server (axum) — REST + OpenAPI
├── auth/               # Authentication: passwords, JWT, OIDC, MFA
├── search/             # Search index (Meilisearch adapter)
└── adapters/
    ├── npm/            # npm / Bun registry adapter
    ├── pypi/           # PyPI Simple Index adapter
    ├── cargo-registry/ # Cargo sparse index adapter
    ├── maven/          # Maven2 adapter
    ├── nuget/          # NuGet v3 adapter
    ├── rubygems/       # RubyGems compact index adapter
    ├── composer/       # Composer repository adapter
    └── oci/            # OCI Distribution API adapter
```

---

## Local Development

### Prerequisites

- [Rust](https://rustup.rs/) 1.77+
- [Docker](https://docs.docker.com/get-docker/) + Docker Compose

### Quick Start

```bash
# 1. Start infrastructure (Postgres, Redis, MinIO, Meilisearch)
docker compose up -d postgres redis minio meilisearch

# 2. Copy env config
cp .env.example .env

# Optional: allow a separate frontend origin during local development
# SERVER__CORS_ALLOWED_ORIGINS=http://localhost:5173

# 3. Run the API server
cargo run --bin publaryn
```

The API is available at `http://localhost:3000`.
Swagger UI at `http://localhost:3000/swagger-ui`.

### Full Stack (includes API container)

```bash
docker compose up --build
```

---

## API Overview

### Authentication

```http
POST /v1/auth/register
POST /v1/auth/login
POST /v1/auth/logout
```

## Control-plane Authentication

Mutable management endpoints under `/v1/*` require an `Authorization: Bearer ...` header.

Supported bearer credentials:

- JWT access tokens returned by `POST /v1/auth/login`
- Opaque API tokens created via `POST /v1/tokens`

Ownership-sensitive fields are derived from the authenticated actor instead of trusting request payload values.
For example, user-owned namespaces and repositories are created for the authenticated user, and organization-owned mutations require owner or admin membership in the owning organization.

Initial control-plane scopes:

| Scope                | Purpose                                                        |
| -------------------- | -------------------------------------------------------------- |
| `profile:write`      | Update the authenticated user's profile                        |
| `tokens:read`        | List the authenticated user's API tokens                       |
| `tokens:write`       | Create and revoke API tokens                                   |
| `orgs:write`         | Create organizations and mutate organization governance data   |
| `orgs:join`          | Review, accept, and decline invitations for the current user   |
| `orgs:transfer`      | Transfer organization ownership to another active member       |
| `namespaces:write`   | Create namespace claims                                        |
| `repositories:write` | Create and update repositories                                 |
| `packages:write`     | Update packages, releases, tags, and trusted publishers        |
| `packages:transfer`  | Transfer package ownership into an organization you administer |
| `audit:read`         | Read the platform audit log (platform administrators only)     |

JWT login sessions receive a default interactive scope set for standard self-service control-plane actions.
Opaque API tokens must request one or more supported scopes, and unsupported scope strings are rejected.

The first invitation slice supports invitations for existing active user accounts. Invited users discover pending invitations through authenticated control-plane endpoints and can accept or decline them in product.

The first ownership-transfer slice allows a current organization owner to hand off their owner role to another existing active member. The transfer is applied atomically, the initiating owner is demoted to `admin`, and the action is written to the audit log.

The first package-transfer slice allows a package owner to move a package into an organization they already administer. This supports personal-to-organization handoff and organization-to-organization transfer when the authenticated actor controls both sides. Direct transfer to another user account is intentionally deferred until an acceptance-based flow exists.

### Users

```http
GET    /v1/users/:username
PATCH  /v1/users/:username
GET    /v1/users/:username/packages
```

### Organizations & Teams

```http
POST   /v1/orgs
GET    /v1/orgs/:slug
PATCH  /v1/orgs/:slug
GET    /v1/orgs/:slug/members
POST   /v1/orgs/:slug/members
DELETE /v1/orgs/:slug/members/:username
POST   /v1/orgs/:slug/ownership-transfer
GET    /v1/orgs/:slug/invitations
POST   /v1/orgs/:slug/invitations
DELETE /v1/orgs/:slug/invitations/:id
GET    /v1/orgs/:slug/teams
POST   /v1/orgs/:slug/teams
GET    /v1/orgs/:slug/packages
GET    /v1/org-invitations
POST   /v1/org-invitations/:id/accept
POST   /v1/org-invitations/:id/decline
```

### Namespace Claims

```http
GET    /v1/namespaces
POST   /v1/namespaces
GET    /v1/namespaces/lookup?ecosystem=<eco>&namespace=<claim>
```

### Repositories

```http
POST   /v1/repositories
GET    /v1/repositories/:slug
PATCH  /v1/repositories/:slug
GET    /v1/repositories/:slug/packages
```

### Packages & Releases

```http
POST   /v1/packages
GET    /v1/packages/:ecosystem/:name
PATCH  /v1/packages/:ecosystem/:name
DELETE /v1/packages/:ecosystem/:name
POST   /v1/packages/:ecosystem/:name/ownership-transfer
POST   /v1/packages/:ecosystem/:name/releases
GET    /v1/packages/:ecosystem/:name/releases
GET    /v1/packages/:ecosystem/:name/releases/:version
POST   /v1/packages/:ecosystem/:name/releases/:version/publish
GET    /v1/packages/:ecosystem/:name/releases/:version/artifacts
PUT    /v1/packages/:ecosystem/:name/releases/:version/artifacts/:filename?kind=<kind>
GET    /v1/packages/:ecosystem/:name/releases/:version/artifacts/:filename
PUT    /v1/packages/:ecosystem/:name/releases/:version/yank
PUT    /v1/packages/:ecosystem/:name/releases/:version/unyank
PUT    /v1/packages/:ecosystem/:name/releases/:version/deprecate
GET    /v1/packages/:ecosystem/:name/tags
PUT    /v1/packages/:ecosystem/:name/tags/:tag
GET    /v1/packages/:ecosystem/:name/security-findings
GET    /v1/packages/:ecosystem/:name/trusted-publishers
POST   /v1/packages/:ecosystem/:name/trusted-publishers
```

Release history responses include published, deprecated, and yanked versions so maintainers and consumers can inspect full version state. Yanked releases can be restored with the dedicated unyank endpoint.

The control-plane publish workflow is now explicit and quarantine-first:

1. create the release in `quarantine`
2. upload one or more immutable artifacts into shared object storage
3. publish the release once artifact metadata and blobs are present consistently

Quarantined and scanning releases are intentionally hidden from public direct reads and artifact downloads. They remain visible only to actors who already have package write access.

Artifact uploads are idempotent by filename and content. Repeating the same upload for the same release and filename returns the existing artifact metadata instead of creating duplicates.

Package and repository read endpoints enforce explicit visibility semantics.
`public` resources are readable and discoverable by everyone.
`unlisted` resources remain readable through direct URLs but are intentionally excluded from search and package listing surfaces.
`private`, `internal_org`, and `quarantined` resources require an authenticated owner or organization member.

Control-plane package creation derives package ownership from the target repository instead of trusting caller-supplied owner fields.
For the current slice, package names are also enforced as globally unique within an ecosystem so the existing `/v1/packages/:ecosystem/:name` control-plane paths remain unambiguous.
If a matching namespace claim exists for an extracted namespace (currently npm/Bun scopes, Composer vendors, and Maven group IDs), the claim owner must match the repository owner.

### Search

```http
GET /v1/search?q=<query>&ecosystem=<eco>&page=1&per_page=20
```

The current search endpoint returns only publicly discoverable packages.
Authenticated discovery for private and organization-internal packages will be added in a later slice with actor-aware indexing.

### Tokens

```http
POST   /v1/tokens
GET    /v1/tokens
DELETE /v1/tokens/:id
```

### Audit

```http
GET /v1/audit
```

### Health

```http
GET /health
GET /readiness
```

`/health` is a liveness probe and returns `200 OK` while the process is running.
`/readiness` is a readiness probe and returns `200 OK` only when the instance can reach PostgreSQL; it returns `503 Service Unavailable` otherwise so orchestrators can stop routing new traffic to that replica.

The API server handles `SIGTERM` and `Ctrl+C` gracefully.
During shutdown it stops accepting new work, lets in-flight requests drain within the orchestrator grace period, and then exits cleanly.
This is the expected lifecycle for rolling updates and horizontal scale-down events.

---

## Security Features

- **Argon2id** password hashing
- **JWT** access tokens with configurable TTL
- **MFA/TOTP** ready (configurable per user and org)
- **OIDC Trusted Publishing** — no long-lived CI secrets needed
- **Immutable releases** — artifact content is never overwritten
- **Append-only Audit Log** — enforced at database rule level
- **Namespace claims** — prevent typosquatting, reserve namespaces
- **Name similarity checks** — Levenshtein distance on new package names
- **Reserved names** — block common abuse patterns
- **Granular tokens** — personal, org, repo-scoped, package-scoped, CI
- **Publish pipeline** — quarantine → scan → publish (never skippable)
- **Dependency confusion protection** — explicit namespace ownership

---

## Domain Model

| Entity             | Description                                                           |
| ------------------ | --------------------------------------------------------------------- |
| `User`             | A registered user account with MFA support                            |
| `Organization`     | Group of users with teams, namespace claims, policies                 |
| `Team`             | Sub-group of an org with fine-grained permissions                     |
| `NamespaceClaim`   | Ecosystem-specific namespace owned by user/org                        |
| `Repository`       | Logical collection of packages (public/private/staging/proxy/virtual) |
| `Package`          | Ecosystem-specific package identity                                   |
| `Release`          | Immutable versioned release                                           |
| `Artifact`         | A file associated with a release (tarball, wheel, jar, gem, …)        |
| `ChannelRef`       | Mutable tag/alias pointing to a release (npm dist-tag, OCI tag)       |
| `Token`            | Granular API token with expiry and scopes                             |
| `AuditLog`         | Append-only record of all significant actions                         |
| `SecurityFinding`  | CVE, malware, or policy violation found in a release                  |
| `TrustedPublisher` | OIDC trusted publishing configuration                                 |

---

## Configuration

All configuration is provided via environment variables (double-underscore separator).
See [`.env.example`](.env.example) for the full reference.

Key variables:

| Variable                       | Description                                                         | Default                              |
| ------------------------------ | ------------------------------------------------------------------- | ------------------------------------ |
| `DATABASE__URL`                | PostgreSQL connection string                                        | —                                    |
| `AUTH__JWT_SECRET`             | JWT signing secret (min 32 chars)                                   | —                                    |
| `AUTH__ISSUER`                 | JWT issuer URL                                                      | `http://localhost:3000`              |
| `SERVER__CORS_ALLOWED_ORIGINS` | Comma-separated browser origins allowed for cross-origin API access | empty (deny cross-origin by default) |
| `STORAGE__ENDPOINT`            | S3/MinIO endpoint                                                   | —                                    |
| `STORAGE__BUCKET`              | Artifact storage bucket                                             | —                                    |
| `SEARCH__URL`                  | Meilisearch base URL                                                | `http://localhost:7700`              |
| `REDIS__URL`                   | Redis URL                                                           | `redis://localhost:6379`             |
| `SERVER__BIND_ADDRESS`         | HTTP bind address                                                   | `0.0.0.0:3000`                       |

The API does not emit permissive CORS headers by default.
If the frontend runs on a different origin in development or production, configure an explicit allowlist with `SERVER__CORS_ALLOWED_ORIGINS`.
Wildcard origins are intentionally rejected so browser-based token usage cannot be exposed accidentally.

---

## Contributing

Contributions are welcome. Please open an issue first to discuss major changes.

---

## License

Apache License 2.0. See [LICENSE](LICENSE) for details.
