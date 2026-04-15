# ADR 0014: Bridge PyPI legacy uploads onto the shared publish pipeline

- Status: Accepted
- Date: 2026-04-15

## Context

Publaryn now serves read-only PyPI Simple API responses, but real pip ecosystem support also requires a native publish path that works with Twine and the legacy upload API used by Warehouse.

That protocol has a few constraints that do not map one-to-one onto Publaryn's existing control-plane workflow:

- uploads arrive as `multipart/form-data` at `/legacy/`
- files are uploaded one at a time
- the first file for a version creates the release and sets its metadata
- subsequent files for the same version may arrive later
- the upload request does not include an explicit Publaryn repository slug

Publaryn must preserve its existing architectural principles while still being usable from standard Python tooling:

- package and release metadata remain in PostgreSQL
- artifact bytes remain in shared object storage
- the API runtime stays stateless and horizontally scalable
- publish-critical metadata must become visible only after the uploaded file is durably stored and recorded consistently

## Decision

Publaryn now accepts Twine-compatible uploads at `POST /pypi/legacy/`.

### Authentication

The route accepts the existing Publaryn credentials already used by the read adapter:

- Basic authentication carrying a Publaryn API token
- Bearer JWTs
- Bearer API tokens

Uploads require the `packages:write` scope.

### Package selection and creation

When the target package already exists, Publaryn reuses the shared package ownership model:

- user owners may upload
- organization members with package publish roles may upload
- delegated teams with `admin` or `publish` package access may upload

When the package does not exist yet, Publaryn auto-creates it in the publisher's first eligible user-owned repository.
This mirrors the current npm adapter behavior and avoids inventing a PyPI-specific repository selector in the legacy protocol.

Organization-targeted auto-create is intentionally deferred because the legacy upload protocol does not carry enough context to choose safely among multiple organization-owned repositories.

### Release and artifact lifecycle

Publaryn creates a release in `quarantine`, stores the uploaded file in shared object storage, records the artifact row, and then immediately promotes the release to its published status once the file is durable.

To match PyPI's one-file-at-a-time upload semantics, the adapter also allows new immutable wheel or sdist artifacts to be appended to an already published PyPI release version.
This exception is limited to adding additional immutable files; existing files are never overwritten.

### Metadata handling

The adapter validates the legacy upload protocol fields that are required for standard clients:

- `:action=file_upload`
- `protocol_version=1`
- one of `md5_digest`, `sha256_digest`, or `blake2_256_digest`
- `filetype`
- `pyversion`
- `metadata_version`
- `name`
- `version`
- `content`

Publaryn stores the upload's core metadata as release provenance and projects the supported subset into the shared package fields:

- summary → package description
- long description → package README
- homepage / project URLs → package homepage and repository URL where possible
- license expression or license → package license
- keywords → package keywords

### Explicit non-goals for this slice

The route currently rejects or defers:

- upload attestations
- detached signatures
- organization-targeted auto-create
- richer Python-specific metadata persistence beyond release provenance

## Consequences

### Positive

- Twine can publish to Publaryn without going through the management API first
- PyPI uploads reuse the shared package, release, audit, and artifact infrastructure
- the adapter remains stateless and safe to scale horizontally across multiple API replicas
- upload retries for the same filename and content are idempotent

### Trade-offs

- package auto-create is currently limited to user-owned repositories
- additional files may appear on an already published PyPI version over time, which is necessary for protocol compatibility but narrower than the current control-plane publish model
- Python-specific metadata is preserved in provenance rather than normalized into dedicated schema columns

## Follow-up work

- add organization-aware repository selection for native PyPI publishes
- persist Python-specific metadata such as `Requires-Python`, project URLs, signatures, and attestations in protocol-aware columns
- add trusted publishing support for PyPI uploads using the existing trusted publisher model
- consider emitting richer observability metrics for upload sizes, duplicate retries, and protocol-level conflicts
