# ADR 0021: Composer Publish Adapter

**Status:** Accepted
**Date:** 2026-04-19
**Decision Makers:** Architecture Team

## Context

Publaryn already serves a Packagist-style read surface for Composer
(`packages.json`, `p/{vendor}/{package}`, `files/{id}/{filename}`).
Unlike npm or RubyGems, Composer has no standardized push API — the
public Packagist mirrors a Git host for source of truth and serves
pre-computed dists. For a private registry, clients typically rely on
a custom `composer-plugin` or a manual upload endpoint.

To complete the adapter matrix we define a small, Publaryn-native
publish surface that any CI pipeline can target with a plain HTTP
request.

## Decision

### Protocol surface

```
PUT    /composer/packages/{vendor}/{package}          — publish a version
DELETE /composer/packages/{vendor}/{package}/versions/{version}  — yank
```

`PUT` accepts `multipart/form-data` with two parts:

- `composer.json` — the package manifest (required). The `version`,
  `name`, `type`, `require`, `autoload`, `license`, etc. are extracted.
- `dist.zip` — the packaged source archive (required). Stored as an
  `Artifact` with kind `composer_zip`.

The `version` field from `composer.json` is authoritative. The URL
path must match the `composer.json` `name` (`{vendor}/{package}`).

### Auth

Bearer token with `packages:write` scope. Same token resolution as npm
and NuGet adapters (supports `pub_*` API tokens and JWTs; rejects
OIDC-derived tokens).

### Release lifecycle

Each `(vendor/package, version)` maps to a `Release`. Publish flow
follows ADR 0009 quarantine-first:

1. Parse `composer.json`, validate name/version match URL.
2. Auto-create `Package` if missing (ADR 0008, pusher's first repo).
3. Create `Release` in `quarantine`.
4. Store `dist.zip` as the sole primary `Artifact`; record a content-
   addressable storage key `releases/{release_id}/artifacts/{sha256}/{name}-{version}.zip`.
5. Persist the parsed `composer.json` as `provenance` on the release
   (JSON column, already present on `releases`).
6. Finalize release → `published`. Reindexing bumps the
   `packages.json` projection.

### Yank semantics

`DELETE …/versions/{version}` transitions the release to `yanked`,
mirroring npm/cargo semantics. Yanked versions disappear from
`p/{vendor}/{package}` metadata but remain downloadable by exact URL
for reproducibility.

### Non-goals

- No Git-backed `repositories.type = vcs` integration — purely
  dist-first.
- No support for `ref`-level dist requests — clients that want a Git
  SHA must use the Git host directly.

## Consequences

- **Positive:** Composer users can publish private packages into
  Publaryn from CI with a single `curl` command.
- **Positive:** Reuses the existing read surface without changes.
- **Negative:** Clients need `composer config repositories.publaryn
  composer https://…/composer` set up; no automatic discovery.

## References

- Composer repository schema: <https://getcomposer.org/doc/05-repositories.md#composer>
- ADR 0009, ADR 0008
