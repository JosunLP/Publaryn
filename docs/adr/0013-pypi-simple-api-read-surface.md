# ADR 0013: Read-only PyPI Simple API as the second native protocol surface

- Status: Accepted
- Date: 2026-04-15

## Context

Publaryn already provides a generic control-plane release workflow and a working npm adapter.
The PyPI ecosystem is the next practical protocol surface because:

- pip and related tooling rely on a comparatively small, well-specified read API
- the existing domain model already stores the file metadata needed for the Simple Repository API
- supporting install flows for a second ecosystem validates the modular-monolith adapter architecture beyond npm

At the same time, Publaryn's control-plane package identity for PyPI previously normalized names differently from the canonical PEP 503 rules.
That mismatch would have made `/simple/<project>/` routing ambiguous and could have allowed logically equivalent PyPI names to drift apart.

## Decision

Publaryn now implements a read-only PyPI Simple Repository API under `/pypi`.
Clients configure pip with an index URL such as:

```text
https://host.example.com/pypi/simple/
```

### Current protocol surface

The current slice provides:

- `GET /pypi/simple/`
- `GET /pypi/simple/<project>/`
- `GET /pypi/files/<artifact_id>/<filename>`

The adapter supports both HTML and JSON serializations of the Simple API via HTTP content negotiation:

- `application/vnd.pypi.simple.v1+json`
- `application/vnd.pypi.simple.v1+html`
- `text/html` as the HTML compatibility alias

### Repository version

Publaryn reports Simple API version `1.1`.
That lets JSON responses include:

- `versions`
- `files[].size`
- `files[].upload-time`

while still using the standard `v1` media types because the media type only carries the major version.

### File coverage

The adapter currently exposes published PyPI distribution files backed by the shared artifact store for these artifact kinds:

- `wheel`
- `sdist`

Files from `quarantine`, `scanning`, and `deleted` releases remain hidden.
Published, deprecated, and yanked releases are visible, and yanked files are marked using the standard yank metadata.

### Authentication and visibility

Public and unlisted package reads work anonymously by direct URL.
Private and organization-internal reads require authentication using existing Publaryn credentials.

The adapter currently accepts:

- Bearer JWTs
- Bearer API tokens
- Basic authentication carrying a Publaryn API token (for pip-friendly private index usage)

This keeps the adapter stateless and horizontally safe because credential checks still resolve against shared PostgreSQL metadata.

### PyPI name canonicalization

Publaryn now uses canonical PEP 503 normalization for PyPI package identity:

- lowercase the project name
- collapse every run of `.`, `-`, and `_` into a single `-`

Existing stored PyPI package rows are updated by migration so protocol routes and control-plane lookups agree on one canonical identity model.

## Consequences

### Positive

- Publaryn now supports pip install/read flows for a second native ecosystem
- the adapter reuses the shared package, release, and artifact domain model without introducing a Python-specific microservice
- private package access remains compatible with horizontally scaled replicas because no local state is introduced
- PyPI canonical name handling is now aligned with the official specification

### Trade-offs

- this slice is read-only; Twine uploads and richer Python metadata are deferred
- root project listing may become expensive at large package counts and is a candidate for caching later
- GPG signatures, core metadata sidecars, provenance URLs, and Requires-Python fields are not exposed yet because the shared data model does not currently persist them in protocol-ready form

## Follow-up work

- add upload support compatible with Twine and the PyPI upload workflow
- persist Python-specific metadata such as `Requires-Python` and optional core metadata sidecars
- consider caching or precomputed views for the `/simple/` root index as package counts grow
- align the npm adapter's protocol-layer permission checks with the newer delegated team-governance model everywhere they still lag behind
