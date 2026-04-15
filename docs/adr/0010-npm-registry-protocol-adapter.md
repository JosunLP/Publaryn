# ADR 0010: npm registry protocol adapter as the first ecosystem binding

- Status: Accepted
- Date: 2026-04-16

## Context

Publaryn's architecture separates the control-plane API (packages, releases, artifacts, dist-tags) from ecosystem-specific wire protocols. All eight ecosystem adapter crates were stubs that only defined a placeholder `EcosystemAdapter` trait.

Without at least one working adapter, `npm publish`, `npm install`, and `npm search` cannot target a Publaryn instance. The npm ecosystem is the highest-traffic target for an initial binding because:

- the npm registry protocol is well-documented via the public CouchDB-based registry
- the publish payload is a single JSON document with a base64-encoded tarball attachment, keeping the flow atomic
- dist-tags map directly to Publaryn's existing `channel_refs` model
- scoped packages (`@scope/name`) exercise the full package-name validation and URL-encoding path

## Decision

### Adapter crate structure

The `publaryn-adapter-npm` crate provides four modules:

| Module     | Responsibility                                                                            |
| ---------- | ----------------------------------------------------------------------------------------- |
| `name`     | npm package name validation, normalization, scope extraction, tarball filename generation |
| `metadata` | Building the npm packument (registry metadata document) from domain types                 |
| `publish`  | Parsing the npm fat-manifest publish payload and decoding the base64 tarball              |
| `routes`   | Axum route handlers exposing the npm registry protocol                                    |

### NpmAppState trait

To avoid a circular dependency between the adapter crate and the API crate, the adapter defines a trait:

```rust
pub trait NpmAppState: Clone + Send + Sync + 'static {
    fn db(&self) -> &PgPool;
    fn artifact_put(&self, key: &str, body: Vec<u8>, content_type: &str) -> ...;
    fn artifact_get(&self, key: &str) -> ...;
    fn base_url(&self) -> &str;
    fn jwt_secret(&self) -> &str;
    fn jwt_issuer(&self) -> &str;
    fn search_packages(&self, query: &str, offset: usize, limit: usize) -> ...;
}
```

The API crate implements this trait for `AppState` in a thin bridge module (`npm_bridge.rs`). This lets the adapter compile and test independently of the API crate.

### Mount point

The npm router is nested under `/npm` in the main API router:

```rust
.nest("/npm", publaryn_adapter_npm::routes::router())
```

Clients configure npm to use the registry:

```
npm config set registry http://host:3000/npm/
```

This allows all ecosystem adapters to coexist on the same API server under distinct mount points (e.g., `/npm`, `/pypi`, `/maven`).

### Endpoints

| Method   | Path                                                 | Purpose                                          |
| -------- | ---------------------------------------------------- | ------------------------------------------------ |
| `GET`    | `/:package`, `/:scope/:name`                         | Packument (full metadata document)               |
| `PUT`    | `/:package`, `/:scope/:name`                         | Publish (fat manifest with base64 tarball)       |
| `GET`    | `/:package/-/:filename`, `/:scope/:name/-/:filename` | Tarball download                                 |
| `GET`    | `/-/v1/search`                                       | npm-compatible search (delegates to Meilisearch) |
| `GET`    | `/-/package/:package/dist-tags`                      | List dist-tags                                   |
| `PUT`    | `/-/package/:package/dist-tags/:tag`                 | Set dist-tag                                     |
| `DELETE` | `/-/package/:package/dist-tags/:tag`                 | Delete dist-tag (protects "latest")              |

### Publish flow

1. Parse the fat manifest JSON, validating that exactly one version entry and one attachment exist.
2. Decode the base64-encoded tarball and compute SHA-512 integrity + SHA-1 shasum.
3. Authenticate the user via Bearer token (JWT or API token with `packages:write` scope).
4. If the package does not exist, auto-create it in the user's first writable repository with ecosystem `npm` and `public` visibility.
5. Create a release in `quarantine` state.
6. Upload the tarball to S3 under `npm/{package_id}/{version}/{filename}`.
7. Create the artifact record with size, SHA-256 digest, and content type.
8. Transition the release to `published`.
9. Set dist-tags (channel_refs) from the publish payload.
10. Re-index the package in Meilisearch.

If any step after quarantine creation fails, the release remains in quarantine for manual cleanup.

### Authentication

The adapter reuses Publaryn's existing auth infrastructure:

- **API tokens** (prefixed `pub_`): SHA-256 hashed, looked up in `api_tokens` table, scope-checked for `packages:write` on publish and `packages:read` on private package reads.
- **JWT tokens**: Validated with the same secret and issuer as the control-plane API.

### Visibility

Package read access follows the same visibility policy as the control-plane (ADR 0007):

- `public` / `unlisted`: readable without authentication
- `private`: requires auth + package owner or org membership
- `internal_org`: requires auth + membership in the owning organization

### Scoped packages

npm scoped packages (`@scope/name`) appear in URL paths as `@scope%2fname` (percent-encoded slash). The adapter provides both `/:scope/:name` and `/:package` route variants. The `/:scope/:name` variant reconstructs the full name via a `scoped_name()` helper. Internally, the full `@scope/name` string is stored as the package name.

## Consequences

- `npm publish`, `npm install`, and `npm search` work against a Publaryn instance configured with `registry=http://host:3000/npm/`.
- The `NpmAppState` trait pattern establishes the convention for all future ecosystem adapters, keeping them independently compilable and testable.
- The adapter ships with 18 unit tests covering name validation, packument construction, and publish payload parsing.
- Integration tests exercising the full publish → download flow will be added once a test harness with an in-memory database is available.
- Future adapters (PyPI, Maven, etc.) follow the same pattern: define an `XxxAppState` trait, implement routes, wire via bridge module and `.nest()`.
