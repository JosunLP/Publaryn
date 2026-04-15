# Publaryn

**Publaryn** — secure, independent package registry across ecosystems.

A self-hostable, security-first package registry platform that speaks the native protocols of all major package managers and provides a unified management API.

---

## Supported Ecosystems

| Ecosystem | Protocol | Status |
|---|---|---|
| npm / Bun | npm Registry Protocol | 🚧 In progress |
| pip / PyPI | Simple Index (PEP 503/691) | 🚧 In progress |
| Rust Crates | Cargo Sparse Index | 🚧 In progress |
| NuGet | NuGet v3 | 🚧 In progress |
| Apache Maven | Maven2 | 🚧 In progress |
| RubyGems | RubyGems / Compact Index | 🚧 In progress |
| Composer | Composer Repository | 🚧 In progress |
| Containers | OCI Distribution Spec | 🚧 In progress |

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

```
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
GET    /v1/orgs/:slug/teams
POST   /v1/orgs/:slug/teams
GET    /v1/orgs/:slug/packages
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
GET    /v1/packages/:ecosystem/:name
PATCH  /v1/packages/:ecosystem/:name
DELETE /v1/packages/:ecosystem/:name
GET    /v1/packages/:ecosystem/:name/releases
GET    /v1/packages/:ecosystem/:name/releases/:version
PUT    /v1/packages/:ecosystem/:name/releases/:version/yank
PUT    /v1/packages/:ecosystem/:name/releases/:version/deprecate
GET    /v1/packages/:ecosystem/:name/tags
PUT    /v1/packages/:ecosystem/:name/tags/:tag
GET    /v1/packages/:ecosystem/:name/security-findings
GET    /v1/packages/:ecosystem/:name/trusted-publishers
POST   /v1/packages/:ecosystem/:name/trusted-publishers
```

### Search

```http
GET /v1/search?q=<query>&ecosystem=<eco>&page=1&per_page=20
```

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

| Entity | Description |
|---|---|
| `User` | A registered user account with MFA support |
| `Organization` | Group of users with teams, namespace claims, policies |
| `Team` | Sub-group of an org with fine-grained permissions |
| `NamespaceClaim` | Ecosystem-specific namespace owned by user/org |
| `Repository` | Logical collection of packages (public/private/staging/proxy/virtual) |
| `Package` | Ecosystem-specific package identity |
| `Release` | Immutable versioned release |
| `Artifact` | A file associated with a release (tarball, wheel, jar, gem, …) |
| `ChannelRef` | Mutable tag/alias pointing to a release (npm dist-tag, OCI tag) |
| `Token` | Granular API token with expiry and scopes |
| `AuditLog` | Append-only record of all significant actions |
| `SecurityFinding` | CVE, malware, or policy violation found in a release |
| `TrustedPublisher` | OIDC trusted publishing configuration |

---

## Configuration

All configuration is provided via environment variables (double-underscore separator).
See [`.env.example`](.env.example) for the full reference.

Key variables:

| Variable | Description | Default |
|---|---|---|
| `DATABASE__URL` | PostgreSQL connection string | — |
| `AUTH__JWT_SECRET` | JWT signing secret (min 32 chars) | — |
| `AUTH__ISSUER` | JWT issuer URL | `http://localhost:3000` |
| `STORAGE__ENDPOINT` | S3/MinIO endpoint | — |
| `STORAGE__BUCKET` | Artifact storage bucket | — |
| `SEARCH__URL` | Meilisearch base URL | `http://localhost:7700` |
| `REDIS__URL` | Redis URL | `redis://localhost:6379` |
| `SERVER__BIND_ADDRESS` | HTTP bind address | `0.0.0.0:3000` |

---

## Contributing

Contributions are welcome. Please open an issue first to discuss major changes.

---

## License

Apache License 2.0. See [LICENSE](LICENSE) for details.
