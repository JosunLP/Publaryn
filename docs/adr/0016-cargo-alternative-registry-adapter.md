# ADR 0016: Cargo alternative registry adapter

- Status: Accepted
- Date: 2026-05-10

## Context

After shipping the npm (ADR 0010) and PyPI (ADR 0013/0014) ecosystem adapters, the Cargo/Rust ecosystem is the next high-value binding. The Cargo alternative registry protocol is well-specified via RFCs 2141 (alternative registries) and 2789 (sparse index), and the Rust community increasingly needs private registries for internal crates.

Key protocol characteristics:

- **Sparse index**: An HTTP-served directory of NDJSON files, one per crate, with tiered path layout (`1/`, `2/`, `3/{c}/`, `{ab}/{cd}/`). Supports ETag/If-None-Match for conditional requests.
- **Binary publish**: `PUT /api/v1/crates/new` sends a binary payload: `[u32 LE json_len][json_metadata][u32 LE crate_len][.crate_bytes]`.
- **Yank/unyank**: `DELETE /api/v1/crates/{name}/{version}/yank`, `PUT .../unyank`.
- **Download**: `GET /api/v1/crates/{name}/{version}/download` returns the `.crate` gzip tarball.
- **Owners**: `GET/PUT/DELETE /api/v1/crates/{name}/owners` for co-owner management.
- **Search**: `GET /api/v1/crates?q=...&per_page=...` returning a JSON response with `crates` and `meta`.

## Decision

### Adapter crate structure

The `publaryn-adapter-cargo-registry` crate provides five modules:

| Module     | Responsibility                                                                   |
| ---------- | -------------------------------------------------------------------------------- |
| `name`     | Crate name validation, normalization (lowercase + hyphen→underscore), index path |
| `publish`  | Binary wire-format parser, SHA-256 computation, dependency mapping               |
| `metadata` | NDJSON index content builder with v:2 schema and ETag generation                 |
| `routes`   | Axum route handlers for both the sparse index and the Web API                    |
| `lib`      | Module re-exports                                                                |

### CargoAppState trait

Following the established pattern (ADR 0010), the adapter defines:

```rust
pub trait CargoAppState: Clone + Send + Sync + 'static {
    fn db(&self) -> &PgPool;
    fn artifact_put(...) -> ...;
    fn artifact_get(...) -> ...;
    fn base_url(&self) -> &str;
    fn jwt_secret(&self) -> &str;
    fn jwt_issuer(&self) -> &str;
    fn search_crates(&self, query: &str, per_page: u32, offset: u32) -> ...;
}
```

The API crate implements this via `cargo_bridge.rs`.

### Mount points

The Cargo adapter requires two mount points — one for the sparse index and one for the Web API:

```rust
.nest("/cargo/index", publaryn_adapter_cargo_registry::routes::index_router())
.nest("/cargo/api/v1", publaryn_adapter_cargo_registry::routes::api_router())
```

Clients configure Cargo to use the registry in `.cargo/config.toml`:

```toml
[registries.publaryn]
index = "sparse+http://host:3000/cargo/index/"
```

### Database: Cargo-specific metadata

A new `cargo_release_metadata` table stores per-release data needed to reconstruct the sparse index without re-parsing `.crate` files:

| Column         | Type                  | Purpose                            |
| -------------- | --------------------- | ---------------------------------- |
| `release_id`   | `UUID PK FK→releases` | Links to the domain release record |
| `deps`         | `JSONB`               | Dependency array in index format   |
| `features`     | `JSONB`               | Feature map                        |
| `features2`    | `JSONB NULL`          | v:2 features (with `dep:` syntax)  |
| `links`        | `TEXT NULL`           | `links` field (C library name)     |
| `rust_version` | `TEXT NULL`           | MSRV                               |

### Sparse index endpoints

| Method | Path               | Purpose                              |
| ------ | ------------------ | ------------------------------------ |
| `GET`  | `/config.json`     | Registry configuration (`dl`, `api`) |
| `GET`  | `/1/:name`         | Index entry for 1-char crate names   |
| `GET`  | `/2/:name`         | Index entry for 2-char crate names   |
| `GET`  | `/3/:prefix/:name` | Index entry for 3-char crate names   |
| `GET`  | `/:ab/:cd/:name`   | Index entry for 4+ char crate names  |

Index responses use `text/plain` NDJSON with v:2 schema, ETag headers, and `304 Not Modified` support.

### Web API endpoints

| Method   | Path                              | Purpose           |
| -------- | --------------------------------- | ----------------- |
| `PUT`    | `/crates/new`                     | Publish a crate   |
| `DELETE` | `/crates/:name/:version/yank`     | Yank a version    |
| `PUT`    | `/crates/:name/:version/unyank`   | Unyank a version  |
| `GET`    | `/crates/:name/owners`            | List owners       |
| `PUT`    | `/crates/:name/owners`            | Add owners        |
| `DELETE` | `/crates/:name/owners`            | Remove owners     |
| `GET`    | `/crates`                         | Search crates     |
| `GET`    | `/crates/:name/:version/download` | Download `.crate` |

### Publish flow

1. Parse the binary wire-format payload (length-prefixed JSON + `.crate` bytes).
2. Validate crate name (ASCII, max 64 chars, no Windows reserved names) and compute SHA-256.
3. Authenticate via Bearer token (API token or JWT, requiring `packages:write` scope).
4. If the package does not exist, auto-create it in the user's first writable repository.
5. Reject if the version (with build metadata stripped) already exists.
6. Create a release in `quarantine` state.
7. Upload `.crate` to S3 under `releases/{release_id}/artifacts/{sha256}/{name}-{version}.crate`.
8. Create the artifact record.
9. Insert `cargo_release_metadata` row with deps, features, links, rust_version.
10. Transition the release to `published`.
11. Audit log the publish event.
12. Re-index in Meilisearch.

### Crate name normalization

Cargo treats `My-Crate`, `my_crate`, and `MY_CRATE` as the same crate. Normalization lowercases and replaces hyphens with underscores. This uses the existing `normalize_package_name()` in `publaryn-core` for the Cargo ecosystem.

### Error format

All error responses use Cargo's expected format: `{"errors": [{"detail": "..."}]}`.

### Authentication & visibility

Same model as npm/PyPI (ADRs 0001, 0007): API tokens (`pub_` prefix, scope-checked), JWT tokens. Public/unlisted crates are readable without auth. Private crates require auth + ownership/membership.

## Consequences

- `cargo publish --registry publaryn` and `cargo install --registry publaryn` work against a Publaryn instance.
- The sparse index eliminates the need for a Git repository, keeping the deployment stateless.
- The `CargoAppState` trait follows the established adapter pattern, making the adapter independently compilable and testable.
- The adapter ships with 36+ unit tests across `name`, `publish`, and `metadata` modules.
- Co-owner management (add/remove owners) is acknowledged but deferred to the Publaryn control-plane API; the Cargo endpoints return success with an informational message.
- Integration tests exercising the full publish → index → download flow will be added once the test harness supports the binary wire format.
- The `cargo_release_metadata` table makes index serving an efficient single query with no `.crate` file re-parsing.
