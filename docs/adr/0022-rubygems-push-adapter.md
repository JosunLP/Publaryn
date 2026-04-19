# ADR 0022: RubyGems Push Adapter

**Status:** Accepted
**Date:** 2026-04-19
**Decision Makers:** Architecture Team

## Context

Publaryn already serves RubyGems read endpoints (gem metadata, version
listings, `.gem` download). The remaining gap is `gem push`, which
developers use to publish gems to a registry.

The `gem push` client sends a `POST /api/v1/gems` with the raw `.gem`
file as the request body (Content-Type: `application/octet-stream`),
and authenticates with an API key in the `Authorization` header
(not `Bearer`-prefixed; just the raw key).

## Decision

### Protocol surface

```
POST   /rubygems/api/v1/gems             — push a .gem
DELETE /rubygems/api/v1/gems/yank        — yank a version (form body)
POST   /rubygems/api/v1/api_key          — echo the supplied key (compat)
```

### Auth

`gem push` sends `Authorization: <key>` (raw API key, no scheme). We
accept this form in addition to `Authorization: Bearer <key>` for
parity with the rest of the adapter set. Key resolution is the same as
other adapters: `pub_*` tokens are looked up by hash; other strings are
parsed as JWTs. `packages:write` scope is required.

The `POST /api/v1/api_key` route exists because recent `gem` CLIs
optionally probe for it during `gem signin` on third-party registries.
It simply returns the supplied key string in the response body with
`200 OK`; no state change.

### `.gem` parsing

A `.gem` file is a POSIX tar archive containing:

- `metadata.gz` — gzipped YAML gemspec with name, version, platform,
  summary, description, authors, licenses, dependencies, and Ruby/gem
  version requirements.
- `data.tar.gz` — the gem's source tree (not parsed by the registry).
- `checksums.yaml.gz` — optional integrity sidecar.

We parse only `metadata.gz` for registry metadata; `data.tar.gz` is
stored untouched as the artifact binary.

### Release lifecycle

Each `(name, version, platform)` triple is a distinct release. Because
Publaryn's release uniqueness is currently `(package, version)`, we
add a partial unique index in migration 018 scoped to the rubygems
ecosystem that includes the `platform` qualifier, stored in a new
`rubygems_release_metadata` table.

Publish flow (ADR 0009 quarantine-first):

1. Parse `metadata.gz` for coordinates.
2. Validate name against `validate_rubygems_package_name`.
3. Auto-create `Package` if missing (pusher's first repo, ADR 0008).
4. Create `Release` in `quarantine`.
5. Store `.gem` as an artifact with kind `gem`.
6. Persist gemspec metadata to `rubygems_release_metadata` (platform,
   runtime deps, development deps, required Ruby/RubyGems versions).
7. Finalize release → `published`.

### Yank

`DELETE /api/v1/gems/yank` with form fields `gem_name`, `version`, and
optional `platform` transitions the matching release to `yanked`.
Unyank is **not** supported via the RubyGems CLI; the Publaryn
control-plane API handles that case.

### Platform handling

`ruby`-platform gems are the default. Native-extension gems report
platform strings like `x86_64-linux` or `arm64-darwin`. We store the
platform verbatim; listings expose it in both the existing
`/api/v1/versions/{name}.json` response and the new
`/api/v1/gems/{name}.json` document already served by the read
surface.

## Consequences

- **Positive:** `gem push --host https://…/rubygems` works end-to-end
  with standard clients.
- **Positive:** Platform-qualified releases unblock multi-platform gem
  families (e.g. `nokogiri`).
- **Negative:** `(name, version, platform)` uniqueness requires an
  ecosystem-scoped partial unique index, which is slightly more complex
  than the plain `(package_id, version)` unique used by other
  ecosystems. Rationale: keeps the cross-ecosystem domain model simple
  and confines the RubyGems quirk to its own table.

## References

- RubyGems push API: <https://guides.rubygems.org/rubygems-org-api/>
- `.gem` tar format: <https://guides.rubygems.org/make-your-own-gem/>
- ADR 0009, ADR 0008
